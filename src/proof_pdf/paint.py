"""One walk over a page's content stream, reporting everything painted and the state it was painted in.

The overprint, colour, K-only, text and contrast checks, and ``inspect``, are all filters over :class:`Paint` events.
The walk keeps the part of the graphics state they read (fill, font, line width, overprint, text spacing) correct
across ``q``/``Q`` and Form XObject scope, and tracks the text matrices so every text run has a real page-space box.
A clip (``W``/``W*``) narrows :attr:`Paint.visible` but never :attr:`Paint.bbox`, so checks that want where the geometry
is keep it and checks that want what survives the clip can have that. Transparency and dash patterns are not modelled.
"""

import collections.abc
import dataclasses
import math
import typing

import pikepdf

from . import content, fonts
from .spaces import (
    CMYK,
    GRAY,
    OTHER,
    RGB,
    classify_colour_space,
    lookup_resource,
    spot_names,
    truthy,
)

# x0, y0, x1, y1 in page points, y up.
Rect = tuple[float, float, float, float]

# Text-showing operators: whatever fill is in force when one runs is the colour the glyphs are painted in.
SHOW_TEXT = ("Tj", "TJ", "'", '"')
_FILL_PATH = frozenset(("f", "F", "f*"))
_STROKE_PATH = frozenset(("S", "s"))
_FILL_STROKE_PATH = frozenset(("B", "B*", "b", "b*"))


@dataclasses.dataclass(frozen=True)
class Fill:
    """A fill colour: its family and, for device spaces, its components."""

    family: str
    value: tuple[float, ...] = ()


# What a colour space's colour is straight after ``cs``, per the PDF spec.
INITIAL_FILL = {
    GRAY: Fill(GRAY, (0.0,)),
    RGB: Fill(RGB, (0.0, 0.0, 0.0)),
    CMYK: Fill(CMYK, (0.0, 0.0, 0.0, 1.0)),
    OTHER: Fill(OTHER),
}


@dataclasses.dataclass(frozen=True)
class State:
    """The slice of the graphics state the checks read. A new page starts in DeviceGray black."""

    fill: Fill = INITIAL_FILL[GRAY]
    font: fonts.Font = fonts.UNKNOWN
    font_size: float = 0.0
    overprint: bool = False
    line_width: float = 1.0
    char_spacing: float = 0.0
    word_spacing: float = 0.0
    h_scale: float = 1.0
    rise: float = 0.0
    leading: float = 0.0
    clip: Rect | None = None  # page-space extent of the clip in force; None until a clip is set (then it only narrows)


@dataclasses.dataclass(frozen=True)
class Paint:
    """One thing painted on the page, with the state it was painted in."""

    kind: str  # "text", "path", "image" or "shading"
    fill: Fill
    overprint: bool
    size: float = 0.0  # effective point size; text only
    bbox: Rect | None = None  # page space; None when the extent is unknown (a shading, an empty path)
    visible: Rect | None = None  # bbox cut to the clip; None when bbox is unknown or the clip leaves nothing
    font: fonts.Font | None = None
    glyphs: int = 0
    notdef: int = 0
    stroke_width: float | None = None  # effective width in points, for stroked paths
    filled: bool = False  # a path painted with its fill (not only stroked)
    xobject: pikepdf.Object | None = None  # the image, for kind == "image"
    name: str = ""  # the image's resource name


def number(args: list, index: int) -> float:
    try:
        return float(args[index])
    except IndexError, TypeError, ValueError:
        return 0.0


def _translation(tx: float, ty: float) -> content.Matrix:
    return (1.0, 0.0, 0.0, 1.0, tx, ty)


def _aabb(m: content.Matrix, points: collections.abc.Iterable[tuple[float, float]]) -> Rect | None:
    mapped = [(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) for x, y in points]
    if not mapped:
        return None
    xs, ys = [p[0] for p in mapped], [p[1] for p in mapped]
    return (min(xs), min(ys), max(xs), max(ys))


def _intersect(a: Rect | None, b: Rect) -> Rect:
    """*b* cut to *a* (None is unbounded). An empty result keeps inverted corners, which stay empty when cut again."""
    if a is None:
        return b
    return (max(a[0], b[0]), max(a[1], b[1]), min(a[2], b[2]), min(a[3], b[3]))


def _non_empty(rect: Rect | None) -> Rect | None:
    return rect if rect is not None and rect[0] <= rect[2] and rect[1] <= rect[3] else None


def _scale(m: content.Matrix) -> float:
    """The factor a matrix scales lengths by (square root of its determinant)."""
    return math.sqrt(abs(m[0] * m[3] - m[1] * m[2]))


def _with_components(fill: Fill, args: list) -> Fill:
    """The fill after ``sc``/``scn``: new components in the same family, or a pattern/spot."""
    if args and isinstance(args[-1], pikepdf.Name):  # scn ... /Pattern
        return Fill(OTHER)
    if fill.family in (GRAY, RGB, CMYK):
        return Fill(fill.family, tuple(number(args, i) for i in range(len(args))))
    return fill  # a spot or DeviceN tint: still not a device colour


def _extgstate(op: content.Operation) -> pikepdf.Object | None:
    return lookup_resource(op.resources, "/ExtGState", op.operands)


class Walker:
    """Walks a page's operators keeping the :class:`State` they would leave a viewer in.

    ``q``/``Q`` restore the state, and so does leaving a Form XObject (a form runs in its own graphics state), which
    is why this is not a flat scan of the operators. Painting operators are reported; everything else only updates
    state. After the walk, :attr:`spots` holds the names of the spot inks the page selected.
    """

    def __init__(self) -> None:
        self.state = State()
        self.entered: list[State] = []  # the state to restore when leaving each form
        self.saved: dict[int, list[State]] = {0: []}  # q-stack per nesting depth
        self.depth = 0
        self.text_matrix: content.Matrix = content.IDENTITY
        self.line_matrix: content.Matrix = content.IDENTITY
        self.path: list[tuple[float, float]] = []
        self.clip_pending = False  # a W/W* has been seen; the path-ending operator that follows applies it
        self.spots: set[str] = set()
        self.transparency: set[str] = set()  # why the page needs a blend space: opacity, blend mode, soft mask
        self._fonts: dict[tuple[int, int] | int, fonts.Font] = {}

    def walk(self, page: pikepdf.Page) -> collections.abc.Iterator[Paint]:
        for op in content.iter_operations(page):
            self._scope(op.depth)
            paint = self._paint(op)
            if paint is not None:
                yield paint
            elif (update := self._UPDATES.get(op.operator)) is not None:
                update(self, op)

    def _scope(self, depth: int) -> None:
        while self.depth < depth:
            self.entered.append(self.state)
            self.depth += 1
            self.saved[self.depth] = []
        while self.depth > depth:
            self.state = self.entered.pop()
            self.depth -= 1

    # ------------------------------------------------------------ painting
    def _paint(self, op: content.Operation) -> Paint | None:
        name = op.operator
        if name in SHOW_TEXT:
            return self._show_text(op)
        if name in _FILL_PATH:
            return self._paint_path(op, filled=True, stroked=False)
        if name in _STROKE_PATH:
            return self._paint_path(op, filled=False, stroked=True)
        if name in _FILL_STROKE_PATH:
            return self._paint_path(op, filled=True, stroked=True)
        state = self.state
        if name == "sh":
            # A shading fills the whole clip; unclipped, its extent is unknown (it covers the page).
            return Paint("shading", state.fill, state.overprint, visible=_non_empty(state.clip))
        if name == "Do":
            xobject = content.lookup_xobject(op.resources, op.operands)
            if xobject is not None and str(xobject.get("/Subtype")) == "/Image":
                if xobject.get("/SMask") is not None:
                    self.transparency.add("soft mask")
                bbox = _aabb(op.ctm, [(0, 0), (1, 0), (0, 1), (1, 1)])
                return Paint(
                    "image",
                    state.fill,
                    state.overprint,
                    bbox=bbox,
                    visible=self._visible(bbox),
                    xobject=xobject,
                    name=str(op.operands[0]),
                )
        return None

    def _paint_path(self, op: content.Operation, *, filled: bool, stroked: bool) -> Paint:
        state = self.state
        bbox = _aabb(op.ctm, self.path)
        paint = Paint(
            "path",
            state.fill,
            state.overprint,
            bbox=bbox,
            visible=self._visible(bbox),
            stroke_width=state.line_width * _scale(op.ctm) if stroked else None,
            filled=filled,
        )
        self._end_path(op, bbox)
        return paint

    def _show_text(self, op: content.Operation) -> Paint:
        name, args, state = op.operator, op.operands, self.state
        if name in ("'", '"'):
            if name == '"':
                self._set(word_spacing=number(args, 0), char_spacing=number(args, 1))
            self._next_line()
            state = self.state
        items = list(args[0]) if name == "TJ" and args else [args[-1]] if args else []
        font, size = state.font, state.font_size
        advance, glyphs, notdef = 0.0, 0, 0
        for item in items:
            if isinstance(item, pikepdf.String):
                for code in font.decode(bytes(item)):
                    advance += (font.width(code) / 1000.0 * size + state.char_spacing) * state.h_scale
                    if font.code_bytes == 1 and code == 32:
                        advance += state.word_spacing * state.h_scale
                    glyphs += 1
                    notdef += font.is_notdef(code)
            else:
                advance -= number([item], 0) / 1000.0 * size * state.h_scale
        trm = content.mul(self.text_matrix, op.ctm)
        low = font.descent / 1000.0 * size + state.rise
        high = font.ascent / 1000.0 * size + state.rise
        bbox = _aabb(trm, [(0, low), (advance, low), (0, high), (advance, high)])
        self.text_matrix = content.mul(_translation(advance, 0.0), self.text_matrix)
        return Paint(
            "text",
            state.fill,
            state.overprint,
            size=size * content.vertical_scale(trm),
            bbox=bbox,
            visible=self._visible(bbox),
            font=font,
            glyphs=glyphs,
            notdef=notdef,
        )

    # --------------------------------------------------------------- state
    def _set(self, **changes: object) -> None:
        self.state = dataclasses.replace(self.state, **changes)  # pyright: ignore[reportArgumentType]

    def _save(self, op: content.Operation) -> None:
        self.saved[self.depth].append(self.state)

    def _restore(self, op: content.Operation) -> None:
        if self.saved[self.depth]:
            self.state = self.saved[self.depth].pop()

    def _begin_text(self, op: content.Operation) -> None:
        self.text_matrix = self.line_matrix = content.IDENTITY

    def _set_matrix(self, op: content.Operation) -> None:
        self.text_matrix = self.line_matrix = content.matrix(op.operands)

    def _move_line(self, op: content.Operation) -> None:
        tx, ty = number(op.operands, 0), number(op.operands, 1)
        if op.operator == "TD":
            self._set(leading=-ty)
        self.line_matrix = self.text_matrix = content.mul(_translation(tx, ty), self.line_matrix)

    def _next_line(self, op: content.Operation | None = None) -> None:
        self.line_matrix = self.text_matrix = content.mul(_translation(0.0, -self.state.leading), self.line_matrix)

    def _leading(self, op: content.Operation) -> None:
        self._set(leading=number(op.operands, 0))

    def _char_spacing(self, op: content.Operation) -> None:
        self._set(char_spacing=number(op.operands, 0))

    def _word_spacing(self, op: content.Operation) -> None:
        self._set(word_spacing=number(op.operands, 0))

    def _h_scale(self, op: content.Operation) -> None:
        self._set(h_scale=number(op.operands, 0) / 100.0)

    def _rise(self, op: content.Operation) -> None:
        self._set(rise=number(op.operands, 0))

    def _font(self, op: content.Operation) -> None:
        resource = lookup_resource(op.resources, "/Font", op.operands)
        font = self._load_font(resource)
        self._set(font=font, font_size=number(op.operands, 1))

    def _load_font(self, resource: pikepdf.Object | None) -> fonts.Font:
        if resource is None:
            return fonts.UNKNOWN
        key = resource.objgen if resource.objgen != (0, 0) else id(resource)
        if key not in self._fonts:
            self._fonts[key] = fonts.load(resource)
        return self._fonts[key]

    def _gray(self, op: content.Operation) -> None:
        self._set(fill=Fill(GRAY, (number(op.operands, 0),)))

    def _rgb(self, op: content.Operation) -> None:
        self._set(fill=Fill(RGB, tuple(number(op.operands, i) for i in range(3))))

    def _cmyk(self, op: content.Operation) -> None:
        self._set(fill=Fill(CMYK, tuple(number(op.operands, i) for i in range(4))))

    def _colour_space(self, op: content.Operation) -> None:
        space = op.operands[0] if op.operands else None
        family = classify_colour_space(space, op.resources)
        if family == OTHER:
            self.spots.update(spot_names(space, op.resources))
        self._set(fill=INITIAL_FILL[family])

    def _components(self, op: content.Operation) -> None:
        self._set(fill=_with_components(self.state.fill, op.operands))

    def _graphics_state(self, op: content.Operation) -> None:
        gs = _extgstate(op)
        if gs is None:
            return
        if gs.get("/OP") is not None or gs.get("/op") is not None:
            self._set(overprint=truthy(gs.get("/OP")) or truthy(gs.get("/op")))
        if gs.get("/LW") is not None:
            self._set(line_width=number([gs.get("/LW")], 0))
        self._note_transparency(gs)

    def _note_transparency(self, gs: pikepdf.Object) -> None:
        if any(gs.get(key) is not None and number([gs.get(key)], 0) < 1.0 for key in ("/ca", "/CA")):
            self.transparency.add("opacity")
        blend = gs.get("/BM")
        if blend is not None:
            modes = list(blend) if isinstance(blend, pikepdf.Array) else [blend]
            if any(str(m) not in ("/Normal", "/Compatible") for m in modes):
                self.transparency.add("blend mode")
        mask = gs.get("/SMask")
        if mask is not None and not (isinstance(mask, pikepdf.Name) and str(mask) == "/None"):
            self.transparency.add("soft mask")

    def _line_width(self, op: content.Operation) -> None:
        self._set(line_width=number(op.operands, 0))

    # ---------------------------------------------------------------- paths
    def _points(self, op: content.Operation) -> None:
        args = op.operands
        count = {"m": 1, "l": 1, "c": 3, "v": 2, "y": 2}[op.operator]
        self.path += [(number(args, 2 * i), number(args, 2 * i + 1)) for i in range(count)]

    def _rect(self, op: content.Operation) -> None:
        x, y, w, h = (number(op.operands, i) for i in range(4))
        self.path += [(x, y), (x + w, y + h)]

    def _visible(self, bbox: Rect | None) -> Rect | None:
        return None if bbox is None else _non_empty(_intersect(self.state.clip, bbox))

    def _clip_next(self, op: content.Operation) -> None:
        self.clip_pending = True

    def _end_path(self, op: content.Operation, bbox: Rect | None = None) -> None:
        """Finish the path; a pending clip takes its page-space extent (exact for a rectangle, else its bounds)."""
        if self.clip_pending:
            self.clip_pending = False
            bbox = bbox or _aabb(op.ctm, self.path)
            if bbox is not None:  # an empty path leaves the clip alone rather than hiding everything after it
                self._set(clip=_intersect(self.state.clip, bbox))
        self.path = []

    _UPDATES: typing.ClassVar[dict[str, collections.abc.Callable[["Walker", content.Operation], None]]] = {
        "q": _save,
        "Q": _restore,
        "BT": _begin_text,
        "Tm": _set_matrix,
        "Td": _move_line,
        "TD": _move_line,
        "T*": _next_line,
        "TL": _leading,
        "Tc": _char_spacing,
        "Tw": _word_spacing,
        "Tz": _h_scale,
        "Ts": _rise,
        "Tf": _font,
        "g": _gray,
        "rg": _rgb,
        "k": _cmyk,
        "cs": _colour_space,
        "sc": _components,
        "scn": _components,
        "gs": _graphics_state,
        "w": _line_width,
        "m": _points,
        "l": _points,
        "c": _points,
        "v": _points,
        "y": _points,
        "re": _rect,
        "n": _end_path,
        "W": _clip_next,
        "W*": _clip_next,
    }


def paint_events(page: pikepdf.Page) -> collections.abc.Iterator[Paint]:
    """Everything *page* paints, with the state in force when it was painted."""
    return Walker().walk(page)
