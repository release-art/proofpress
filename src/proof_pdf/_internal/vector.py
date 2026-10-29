"""Vector artwork recovered as standalone one-page PDFs: logos, pictograms, rules, any drawn shape.

A PDF has no notion of "this is the vest icon": the artwork is a flat run of path operators. This walks a page, keeps
every painted path with the graphics state it was painted in, and groups paths whose bounding boxes touch (within a
gap). Each group is rewritten as its own page, cropped to the group, with only its own paths, so ``\\includegraphics``
takes it. The same artwork placed twice, or on several pages, renders to identical bytes, so the caller can dedupe.

Text over the artwork (dimensions, pin names, captions) is shown again inside the piece, with its original font, and
takes part in the grouping; it is also still recovered as text. Known ceilings: clips are not applied (a clipped shape
comes out whole); placed images are not part of a group (``extract`` recovers them separately); a shape bigger than a
fraction of the page, usually a background, is dropped so it cannot glue everything to it; grouping is by touching
boxes, so a pictogram drawn in separated pieces comes out as several files.
"""

import bisect
import collections
import dataclasses
import hashlib
import io
import math
import typing

import pikepdf

from . import content
from .paint import Fill, Paint, TextShow
from .spaces import CMYK, GRAY, RGB, lookup_resource, spot_names

Rect = tuple[float, float, float, float]

_POINT_COUNT = {"m": 1, "l": 1, "c": 3, "v": 2, "y": 2}
_PATH_OPS = frozenset((*_POINT_COUNT, "re", "h"))
_PAINT_OPS = frozenset(("S", "s", "f", "F", "f*", "B", "B*", "b", "b*"))
_STROKES = frozenset(("S", "s", "B", "B*", "b", "b*"))
_FILL_COLOUR = frozenset(("g", "rg", "k"))
_STROKE_COLOUR = frozenset(("G", "RG", "K"))
_LINE_STYLE = frozenset(("J", "j", "M", "d", "ri", "i"))
MIN_PAGE_PT = 3.0
INVISIBLE = 3  # text render mode 3: an OCR layer, not something to draw
# A plain rule longer than this share of the page's longer side is layout (a table rule, a divider), never artwork;
# a cluster is artwork when at least ARTWORK_SHARE of its shapes are not plain rectangles or lines.
LONG_RULE_FRACTION = 0.25
ARTWORK_SHARE = 0.25
# A group with at least this share of its area inside a bigger group is part of it.
ABSORB_SHARE = 0.5
# A label reaches LABEL_REACH_EM of its height beyond its box (dimension text sits a little off its leader); shapes
# up to GLYPH_EM of its height standing within that reach are glyphs of the label, and extend it. A drawing may
# cover GROUP_FRACTION of the page; the per-shape cap for backgrounds is the caller's max_fraction.
LABEL_REACH_EM = 0.3
GLYPH_EM = 1.2
GROUP_FRACTION = 0.9
COMPOUND_SUBPATHS = 3  # a path of at least this many outlines is content, however big its box
# Short text within NEAR_EM of its own height from a drawing of at least MIN_DRAWING_SHAPES shapes is its label.
NEAR_EM = 1.5
MAX_LABEL_CHARS = 32  # of a whole line: pin names and dimensions are short, a paragraph's lines are not
MIN_DRAWING_SHAPES = 5
MIN_DRAWING_LABELS = 3
GATHER_PASSES = 6
# Text is running text when FLOWING_LINES lines of one size share a left edge (within STACK_EDGE_EM) and sit within
# STACK_PITCH_EM of each other; fragments of a line closer than ROW_GAP_EM are one line.
FLOWING_LINES = 3
STACK_EDGE_EM = 0.25
STACK_PITCH_EM = 2.2
ROW_GAP_EM = 0.6
_DEVICE_SPACES = frozenset(("/DeviceGray", "/DeviceRGB", "/DeviceCMYK", "/Pattern"))


@dataclasses.dataclass(frozen=True)
class _Ref:
    """A named resource a state operator points at, carried along because the new page has to hold it."""

    index: int  # which operand is the name
    category: str
    obj: pikepdf.Object


@dataclasses.dataclass(frozen=True)
class _StateOp:
    operator: str
    operands: tuple[typing.Any, ...]
    ref: _Ref | None = None


@dataclasses.dataclass(frozen=True)
class _State:
    fill: tuple[_StateOp, ...] = ()
    stroke: tuple[_StateOp, ...] = ()
    line: dict[typing.Any, _StateOp] = dataclasses.field(default_factory=dict)  # by operator; ExtGStates by object
    width: float = 1.0


@dataclasses.dataclass(frozen=True)
class Shape:
    path: tuple[tuple[str, tuple[typing.Any, ...]], ...]  # (operator, operands), the paint operator last
    ctm: content.Matrix
    state: _State
    bbox: Rect
    index: int = 0  # position in paint order on the page
    mcid: int | None = None  # the marked-content id it was painted under, for the structure tree

    @property
    def subpaths(self) -> int:
        """How many separate outlines the path holds: a compound path (all a drawing's hairlines in one) has many."""
        return sum(1 for name, _ in self.path if name in ("m", "re"))

    @property
    def paint(self) -> str:
        return self.path[-1][0]

    @property
    def plain(self) -> bool:
        """A bare rectangle or straight line: a panel, background or rule, which is not artwork and glues nothing."""
        ops = [name for name, _ in self.path[:-1]]
        return bool(ops) and (set(ops) == {"re"} or ops in (["m", "l"], ["m", "l", "h"]))

    @property
    def endpoints(self) -> tuple[tuple[float, float], tuple[float, float]] | None:
        """A straight line's two ends in page space (None for a rectangle)."""
        if self.path[0][0] != "m" or len(self.path) < 3:
            return None
        a, b = _points("m", list(self.path[0][1])), _points("l", list(self.path[1][1]))
        if not a or not b:
            return None
        m = self.ctm
        return tuple((m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) for x, y in (a[0], b[0]))  # type: ignore[return-value]

    @property
    def fill(self) -> str | None:
        return _colour(self.state.fill) if self.paint not in ("S", "s") else None

    @property
    def stroke(self) -> str | None:
        return _colour(self.state.stroke) if self.paint in _STROKES else None

    @property
    def pen(self) -> tuple[float, str] | None:
        """The stroke's effective width and colour, kept the same across a drawing's lines; None if unstroked."""
        return (round(self.stroke_width or 0.0, 2), self.stroke or "") if self.stroke else None

    @property
    def stroke_width(self) -> float | None:
        return (
            self.state.width * abs(self.ctm[0] * self.ctm[3] - self.ctm[1] * self.ctm[2]) ** 0.5
            if self.stroke
            else None
        )


@dataclasses.dataclass(frozen=True)
class Label:
    """A run of text overlapping artwork: a dimension, a pin name, a caption. It is shown again inside the piece."""

    bbox: Rect
    show: TextShow
    fill: Fill
    text: str | None
    reach: float = 0.0  # how far from its box it joins things; 0 means the grouping gap


@dataclasses.dataclass(frozen=True)
class Group:
    bbox: Rect  # page space, points, y up
    shapes: tuple[Shape, ...]
    labels: tuple[Label, ...] = ()

    @property
    def mcids(self) -> set[int]:
        return {s.mcid for s in self.shapes if s.mcid is not None}

    @property
    def text(self) -> list[str]:
        """The readable text the piece carries, in the order it was shown."""
        return [label.text for label in self.labels if label.text and label.text.strip()]

    def render(self) -> tuple[str, bytes]:
        """``(digest, pdf bytes)``: the group alone on a page cropped to it, moved to the origin.

        The digest is of the content stream and the resources it uses, so the same artwork gives the same digest
        wherever it sits, and artwork sharing a resource keeps one digest while a different one never collides.
        """
        x0, y0, x1, y1 = self.bbox
        pdf = pikepdf.new()
        resources: dict[str, pikepdf.Dictionary] = {}
        names: dict[tuple, str] = {}

        def named(ref: _Ref) -> pikepdf.Name:
            key = (ref.category, ref.obj.objgen if ref.obj.objgen != (0, 0) else id(ref.obj))
            if key not in names:
                names[key] = f"/R{len(names)}"
                resources.setdefault(ref.category, pikepdf.Dictionary())[names[key]] = pdf.copy_foreign(ref.obj)
            return pikepdf.Name(names[key])

        def emit(op: _StateOp) -> pikepdf.ContentStreamInstruction:
            operands = list(op.operands)
            if op.ref is not None:
                operands[op.ref.index] = named(op.ref)
            return pikepdf.ContentStreamInstruction(operands, pikepdf.Operator(op.operator))

        instructions: list[pikepdf.ContentStreamInstruction] = []
        for shape in self.shapes:
            instructions += _shape_ops(shape, (x0, y0), emit)
        for label in self.labels:
            instructions += _label_ops(label, (x0, y0), named)
        data = pikepdf.unparse_content_stream(instructions)
        # PDF forbids pages under 3 units: a smaller piece sits in the corner of a 3 pt page (see its placement).
        page = pdf.add_blank_page(page_size=(max(x1 - x0, MIN_PAGE_PT), max(y1 - y0, MIN_PAGE_PT)))
        page.Contents = pdf.make_stream(data)
        page.Resources = pikepdf.Dictionary(dict(resources))
        digest = hashlib.sha1(data + repr(sorted(names)).encode()).hexdigest()
        buffer = io.BytesIO()
        pdf.save(buffer)
        return digest, buffer.getvalue()


def _op(operator: str, *operands: typing.Any) -> pikepdf.ContentStreamInstruction:
    return pikepdf.ContentStreamInstruction(list(operands), pikepdf.Operator(operator))


def _shape_ops(shape: Shape, origin: tuple[float, float], emit) -> list[pikepdf.ContentStreamInstruction]:
    moved = content.mul(shape.ctm, (1.0, 0.0, 0.0, 1.0, -origin[0], -origin[1]))
    state = shape.state
    return [
        _op("q"),
        _op("cm", *[round(v, 4) + 0.0 for v in moved]),
        *[emit(op) for op in (*state.line.values(), *state.fill, *state.stroke)],
        *[_op(operator, *operands) for operator, operands in shape.path],
        _op("Q"),
    ]


_FILL_OPERATORS = {GRAY: "g", RGB: "rg", CMYK: "k"}


def _label_ops(label: Label, origin: tuple[float, float], named) -> list[pikepdf.ContentStreamInstruction]:
    show = label.show
    if show.font_resource is None:
        return []
    moved = (*show.trm[:4], show.trm[4] - origin[0], show.trm[5] - origin[1])
    colour = _FILL_OPERATORS.get(label.fill.family)
    return [
        _op("q"),
        *([_op(colour, *label.fill.value)] if colour and label.fill.value else []),
        _op("BT"),
        _op("Tf", named(_Ref(0, "/Font", show.font_resource)), show.size),
        _op("Tc", show.char_spacing),
        _op("Tw", show.word_spacing),
        _op("Tz", round(show.h_scale * 100, 4)),
        _op("Ts", show.rise),
        _op("Tr", show.render_mode),
        _op("Tm", *[round(v, 4) + 0.0 for v in moved]),
        _op(show.operator, *show.operands),
        _op("ET"),
        _op("Q"),
    ]


def artwork(
    page: pikepdf.Page, *, gap_pt: float, max_fraction: float, texts: typing.Sequence[Paint] = ()
) -> tuple[list[Group], list[Shape]]:
    """``(groups, boxes)``: the artwork on *page* as groups of touching shapes, and the plain rectangles and lines
    that are not part of any.

    Short plain shapes (wires, box outlines) take part in the clustering, so a diagram joined by lines is one piece.
    So does text (*texts*, the page's text runs): a label touching the drawing is part of it, and it bridges what it
    touches, but two labels never join each other, or a page of paragraphs would be one cluster. A cluster is artwork
    when at least ARTWORK_SHARE of its shapes are not plain; one of mostly rules is a table or a ruled layout, which
    would otherwise glue every pictogram in it together, so there only the non-plain shapes group. Long rules and
    backgrounds never join anything. What is left over are the panels and rules of the layout, which a caller may want
    as data. Text mostly inside a piece but not touching it is added afterwards.
    """
    media = page.mediabox
    width, height = abs(float(media[2]) - float(media[0])), abs(float(media[3]) - float(media[1]))
    limit = max_fraction * width * height  # for one shape: bigger than this is a background
    group_limit = GROUP_FRACTION * width * height  # for a whole drawing, which may fill most of a page
    longest_rule = LONG_RULE_FRACTION * max(width, height)
    everything = _collect(page)
    labels = [
        Label(t.bbox, t.show, t.fill, t.text)
        for t in texts
        if t.show is not None and t.bbox is not None and t.render_mode != INVISIBLE
    ]
    candidates: list[Shape | Label] = [
        s
        for s in everything
        if (
            _area(s.bbox) <= limit if not s.plain else max(s.bbox[2] - s.bbox[0], s.bbox[3] - s.bbox[1]) <= longest_rule
        )
    ]
    # Too big to be told from a background by its box, yet holding many outlines: a drawing's hairlines or
    # arrowheads drawn as one path. They cannot cluster by their boxes, so a drawing adopts them afterwards.
    sparse = [s for s in everything if not s.plain and _area(s.bbox) > limit and s.subpaths >= COMPOUND_SUBPATHS]
    labels = _reached(labels, candidates, gap_pt)
    groups: list[Group] = []
    used: set[int] = set()
    for cluster in _clusters([*candidates, *labels], gap_pt):
        parts = [n for n in cluster if isinstance(n, Shape)]
        art = [s for s in parts if not s.plain]
        if not art:
            continue
        box = _union([n.bbox for n in cluster])
        if (
            _artlike(parts) >= ARTWORK_SHARE * len(parts)
            and 0 < _area(box) <= group_limit
            and box[2] > box[0]
            and box[3] > box[1]
        ):
            carried = tuple(n for n in cluster if isinstance(n, Label))
            groups.append(Group(box, tuple(parts), carried))
            used |= {id(n) for n in cluster if isinstance(n, Label) or n.plain}
        else:
            groups += _group(art, gap_pt, group_limit)
    boxes = [s for s in everything if s.plain and id(s) not in used]
    groups = _absorb(_adopt(sparse, groups))
    groups = _attach([label for label in labels if id(label) not in used], groups)
    groups = _gather([label for label in labels if id(label) not in {id(x) for g in groups for x in g.labels}], groups)
    return sorted(groups, key=lambda g: (-g.bbox[3], g.bbox[0])), boxes  # reading order: top to bottom, then left


def _artlike(parts: list[Shape]) -> int:
    """How many of *parts* belong to a drawing: the shapes that are not plain, and the plain lines drawn with the same
    pen (stroke width and colour) as one of them. Wires joining a schematic's parts match, a table's rules do not."""
    pens = {s.pen for s in parts if not s.plain and s.pen}
    return sum(1 for s in parts if not s.plain or (s.pen is not None and s.pen in pens))


def _adopt(sparse: list[Shape], groups: list[Group]) -> list[Group]:
    """Add each compound path too big to cluster by its box to the smallest drawing it lies mostly inside."""
    extra: dict[int, list[Shape]] = collections.defaultdict(list)
    for shape in sparse:
        hosts = [i for i, g in enumerate(groups) if _mostly_inside(shape.bbox, g.bbox)]
        if hosts:
            extra[min(hosts, key=lambda i: _area(groups[i].bbox))].append(shape)
    return [
        Group(g.bbox, tuple(sorted((*g.shapes, *extra[i]), key=lambda s: s.index)), g.labels) if extra[i] else g
        for i, g in enumerate(groups)
    ]


def _gather(labels: list[Label], groups: list[Group]) -> list[Group]:
    """Short text standing within about a line of a drawing is its label: pin names, titles, dimensions.

    Repeats, each label that joins widening the drawing's reach, so a three-line title is drawn in line by line. Only
    drawings (not icons), only short text (a paragraph's lines are long), and each label goes to the nearest drawing.
    """
    boxes = [g.bbox for g in groups]
    extra: dict[int, list[Label]] = collections.defaultdict(list)
    flowing = _flowing(labels)
    pending = [label for label in labels if id(label) not in flowing]
    for _ in range(GATHER_PASSES):
        joined = False
        for label in list(pending):
            reach = NEAR_EM * (label.bbox[3] - label.bbox[1])
            near = [
                (_distance(label.bbox, boxes[i]), i)
                for i, g in enumerate(groups)
                if _is_drawing(g) and _distance(label.bbox, boxes[i]) <= reach
            ]
            if near:
                _, i = min(near)
                extra[i].append(label)
                boxes[i] = _union([boxes[i], label.bbox])
                pending.remove(label)
                joined = True
        if not joined:
            break
    return [Group(boxes[i], g.shapes, (*g.labels, *extra[i])) if extra[i] else g for i, g in enumerate(groups)]


def _flowing(labels: list[Label]) -> set[int]:
    """The labels that are running text rather than a drawing's: those on a long line, or in a stack of three or more
    lines, left-aligned at a line-like pitch.

    Fragments on one baseline are first joined into lines. Pin names, dimension texts and centred titles are short and
    not stacked this way, so they are not caught; a paragraph beside a figure is, and so stays text.
    """
    rows: dict[tuple[float, float], list[Label]] = collections.defaultdict(list)
    for label in labels:
        rows[(round(label.bbox[1] * 2) / 2, round(label.show.size, 1))].append(label)
    lines: list[tuple[Rect, list[Label]]] = []
    for members in rows.values():
        current: list[Label] = []
        for label in sorted(members, key=lambda m: m.bbox[0]):
            height = label.bbox[3] - label.bbox[1]
            if current and label.bbox[0] - current[-1].bbox[2] > ROW_GAP_EM * height:
                lines.append((_union([m.bbox for m in current]), current))
                current = []
            current.append(label)
        lines.append((_union([m.bbox for m in current]), current))
    flowing: set[int] = set()
    for _, members in lines:
        if sum(len(m.text or "") for m in members) + len(members) - 1 > MAX_LABEL_CHARS:
            flowing |= {id(m) for m in members}
    chain: list[tuple[Rect, list[Label]]] = []
    for line in sorted(lines, key=lambda ln: (round(ln[1][0].show.size, 1), round(ln[0][0]), -ln[0][1])):
        if chain and not _stacked(chain[-1][0], line[0], chain[-1][1][0].show.size, line[1][0].show.size):
            chain = []
        chain.append(line)
        if len(chain) >= FLOWING_LINES:
            flowing |= {id(label) for _, members in chain for label in members}
    return flowing


def _stacked(above: Rect, below: Rect, size_a: float, size_b: float) -> bool:
    height = above[3] - above[1]
    pitch = above[1] - below[1]
    return (
        abs(size_a - size_b) < 0.1
        and abs(above[0] - below[0]) <= STACK_EDGE_EM * height
        and 0 < pitch <= STACK_PITCH_EM * height
    )


def _is_drawing(group: Group) -> bool:
    """A piece of artwork with real content, rather than an icon: many shapes, or already carrying several labels."""
    return len(group.shapes) >= MIN_DRAWING_SHAPES or len(group.labels) >= MIN_DRAWING_LABELS


def _distance(a: Rect, b: Rect) -> float:
    """The gap between two boxes (0 when they touch or overlap)."""
    return math.hypot(max(a[0] - b[2], b[0] - a[2], 0.0), max(a[1] - b[3], b[1] - a[3], 0.0))


def _attach(labels: list[Label], groups: list[Group]) -> list[Group]:
    """Give each label that sits mostly inside a piece, without touching it, to the smallest such piece."""
    extra: dict[int, list[Label]] = collections.defaultdict(list)
    for label in labels:
        hosts = [i for i, g in enumerate(groups) if _mostly_inside(label.bbox, g.bbox)]
        if hosts:
            extra[min(hosts, key=lambda i: _area(groups[i].bbox))].append(label)
    return [
        Group(_union([g.bbox, *[label.bbox for label in extra[i]]]), g.shapes, (*g.labels, *extra[i]))
        if extra[i]
        else g
        for i, g in enumerate(groups)
    ]


def _absorb(groups: list[Group]) -> list[Group]:
    """Fold each group lying mostly inside a bigger one into it: a label inside a diagram's outline is part of it."""
    kept: list[Group] = []
    for group in sorted(groups, key=lambda g: -_area(g.bbox)):
        host = next((k for k in kept if _mostly_inside(group.bbox, k.bbox) and len(k.shapes) > len(group.shapes)), None)
        if host is None:
            kept.append(group)
        else:
            parts = sorted((*host.shapes, *group.shapes), key=lambda shape: shape.index)
            kept[kept.index(host)] = Group(_union([host.bbox, group.bbox]), tuple(parts), (*host.labels, *group.labels))
    return kept


def _mostly_inside(inner: Rect, outer: Rect) -> bool:
    shared = _area((max(inner[0], outer[0]), max(inner[1], outer[1]), min(inner[2], outer[2]), min(inner[3], outer[3])))
    return shared >= ABSORB_SHARE * _area(inner) > 0


def _group(shapes: list[Shape], gap_pt: float, limit: float) -> list[Group]:
    result = []
    for parts in _clusters(shapes, gap_pt):
        box = _union([s.bbox for s in parts])
        if 0 < _area(box) <= limit and box[2] - box[0] > 0 and box[3] - box[1] > 0:
            result.append(Group(box, tuple(p for p in parts if isinstance(p, Shape))))
    return result


def _reached(labels: list[Label], shapes: list[Shape | Label], gap_pt: float) -> list[Label]:
    """Give each label its reach, and let the glyph-like shapes standing next to it (a diameter sign, a degree mark
    drawn as a path) extend its box, so that the label's reach starts from the whole of what it says."""
    order = sorted((s for s in shapes if isinstance(s, Shape)), key=lambda s: s.bbox[0])
    starts = [s.bbox[0] for s in order]
    result = []
    for label in labels:
        height = label.bbox[3] - label.bbox[1]
        reach = max(gap_pt, LABEL_REACH_EM * height)
        box = label.bbox
        first = bisect.bisect_left(starts, box[0] - reach - GLYPH_EM * height)
        for shape in order[first : bisect.bisect_right(starts, box[2] + reach)]:
            size = max(shape.bbox[2] - shape.bbox[0], shape.bbox[3] - shape.bbox[1])
            if size <= GLYPH_EM * height and _touch(label.bbox, shape.bbox, reach):
                box = _union([box, shape.bbox])
        result.append(dataclasses.replace(label, bbox=box, reach=max(gap_pt, LABEL_REACH_EM * (box[3] - box[1]))))
    return result


def _clusters(nodes: list, gap_pt: float) -> list[list]:
    """Nodes (shapes and labels) whose boxes touch, transitively; two labels never join each other.

    Nodes touch within the grouping gap, and a label within its own, larger, reach.
    """
    reach = [getattr(n, "reach", 0.0) or gap_pt for n in nodes]
    widest = max(reach, default=gap_pt)
    # Sweep by left edge: a node can only touch nodes that start before its right edge plus the widest reach.
    order = sorted(range(len(nodes)), key=lambda i: nodes[i].bbox[0])
    parent = list(range(len(nodes)))

    def find(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for a, i in enumerate(order):
        for j in order[a + 1 :]:
            if nodes[j].bbox[0] > nodes[i].bbox[2] + widest:
                break
            if isinstance(nodes[i], Label) and isinstance(nodes[j], Label):
                continue
            if _touch(nodes[i].bbox, nodes[j].bbox, max(reach[i], reach[j])):
                parent[find(j)] = find(i)
    members: dict[int, list[int]] = {}
    for i in range(len(nodes)):
        members.setdefault(find(i), []).append(i)
    return [[nodes[i] for i in sorted(indexes)] for indexes in members.values()]


def _colour(ops: tuple[_StateOp, ...]) -> str:
    """The colour operators in force as written in the content stream, e.g. ``k 0 0 0 0.65``; black if none were set.

    A spot colour is named instead (``spot PANTONE 165 C 1``): its resource name means nothing outside the page.
    """
    if not ops:
        return "g 0"
    first = ops[0]
    if first.operator in ("cs", "CS") and first.ref is not None and (names := spot_names(first.ref.obj, None)):
        tint = _number(list(ops[1].operands)) if len(ops) > 1 else 1.0
        return f"spot {'+'.join(names)} {tint:g}"
    return " ".join(f"{op.operator} {' '.join(str(o) for o in op.operands)}".strip() for op in ops)


def _area(r: Rect) -> float:
    return max(r[2] - r[0], 0.0) * max(r[3] - r[1], 0.0)


def _touch(a: Rect, b: Rect, gap: float) -> bool:
    return a[0] <= b[2] + gap and b[0] <= a[2] + gap and a[1] <= b[3] + gap and b[1] <= a[3] + gap


def _union(rects: list[Rect]) -> Rect:
    return (min(r[0] for r in rects), min(r[1] for r in rects), max(r[2] for r in rects), max(r[3] for r in rects))


class _Collector:
    """Walks a page's operators keeping the state they leave, and gathers every painted path as a :class:`Shape`."""

    def __init__(self) -> None:
        self.shapes: list[Shape] = []
        self.state = _State()
        self.entered: list[_State] = []  # the state to give back when a Form XObject ends
        self.saved: dict[int, list[_State]] = {0: []}  # q-stack per nesting depth
        self.depth = 0
        self.path: list[tuple[str, tuple[typing.Any, ...]]] = []
        self.points: list[tuple[float, float]] = []
        self.marked: list[int | None] = []

    def feed(self, op: content.Operation) -> None:
        self._scope(op.depth)
        name, args = op.operator, op.operands
        if name == "q":
            self.saved[self.depth].append(self.state)
        elif name == "Q":
            self.state = self.saved[self.depth].pop() if self.saved[self.depth] else self.state
        elif name in _PATH_OPS:
            self.path.append((name, tuple(args)))
            self.points += _points(name, args)
        elif name in _PAINT_OPS:
            self._paint(op)
        elif name == "n":
            self.path, self.points = [], []
        elif name in ("BMC", "BDC", "EMC"):
            _marked(self.marked, op)
        else:
            self.state = _update(self.state, op)

    def _scope(self, depth: int) -> None:
        while self.depth < depth:  # a Form XObject runs in a copy of the state and gives it back
            self.entered.append(self.state)
            self.depth += 1
            self.saved[self.depth] = []
        while self.depth > depth:
            self.state = self.entered.pop()
            self.depth -= 1

    def _paint(self, op: content.Operation) -> None:
        if self.points:
            self.path.append((op.operator, ()))
            box = _bbox(op.ctm, self.points, op.operator, self.state.width)
            mcid = next((m for m in reversed(self.marked) if m is not None), None)
            self.shapes.append(Shape(tuple(self.path), op.ctm, self.state, box, len(self.shapes), mcid))
        self.path, self.points = [], []


def _collect(page: pikepdf.Page) -> list[Shape]:
    collector = _Collector()
    for op in content.iter_operations(page):
        collector.feed(op)
    return collector.shapes


def _put(line: dict[typing.Any, _StateOp], key: typing.Any, op: _StateOp) -> dict[typing.Any, _StateOp]:
    """*line* with *op* set under *key*, as the latest: a state set again later must take effect after the others."""
    return {**{k: v for k, v in line.items() if k != key}, key: op}


def _marked(marked: list[int | None], op: content.Operation) -> None:
    """Track BMC/BDC/EMC nesting, keeping each level's MCID (only the page's own stream has meaningful ones)."""
    if op.operator == "EMC":
        if marked:
            marked.pop()
        return
    mcid = None
    if op.operator == "BDC" and len(op.operands) > 1 and op.depth == 0:
        props = op.operands[1]
        if isinstance(props, pikepdf.Name):
            props = lookup_resource(op.resources, "/Properties", [props])
        if isinstance(props, pikepdf.Dictionary) and props.get("/MCID") is not None:
            mcid = int(props["/MCID"])
    marked.append(mcid)


def _update(state: _State, op: content.Operation) -> _State:
    name, args = op.operator, op.operands
    if name in _FILL_COLOUR:
        return dataclasses.replace(state, fill=(_StateOp(name, tuple(args)),))
    if name in _STROKE_COLOUR:
        return dataclasses.replace(state, stroke=(_StateOp(name, tuple(args)),))
    if name in ("cs", "CS"):
        colour = _StateOp(name, tuple(args), _ref(op, "/ColorSpace", 0) if _named(args, 0) else None)
        return dataclasses.replace(state, **{"fill" if name == "cs" else "stroke": (colour,)})
    if name in ("sc", "scn", "SC", "SCN"):
        pattern = _ref(op, "/Pattern", len(args) - 1) if args and isinstance(args[-1], pikepdf.Name) else None
        colour = _StateOp(name, tuple(args), pattern)
        field = "fill" if name in ("sc", "scn") else "stroke"
        current = getattr(state, field)
        base = current[:1] if current and current[0].operator in ("cs", "CS") else ()
        return dataclasses.replace(state, **{field: (*base, colour)})
    if name == "w":
        return dataclasses.replace(state, width=_number(args), line=_put(state.line, "w", _StateOp(name, tuple(args))))
    if name in _LINE_STYLE:
        return dataclasses.replace(state, line=_put(state.line, name, _StateOp(name, tuple(args))))
    if name == "gs":
        ref = _ref(op, "/ExtGState", 0)
        if ref is not None:
            key = ("gs", ref.obj.objgen if ref.obj.objgen != (0, 0) else id(ref.obj))
            return dataclasses.replace(state, line=_put(state.line, key, _StateOp(name, tuple(args), ref)))
    return state


def _named(args: list, index: int) -> bool:
    return len(args) > index and str(args[index]) not in _DEVICE_SPACES


def _ref(op: content.Operation, category: str, index: int) -> _Ref | None:
    obj = lookup_resource(op.resources, category, [op.operands[index]]) if len(op.operands) > index else None
    return None if obj is None else _Ref(index, category, obj)


def _number(args: list) -> float:
    try:
        return float(args[0])
    except IndexError, TypeError, ValueError:
        return 1.0


def _points(name: str, args: list) -> list[tuple[float, float]]:
    values = [_number([a]) for a in args]
    if name == "re" and len(values) == 4:
        return [(values[0], values[1]), (values[0] + values[2], values[1] + values[3])]
    count = _POINT_COUNT.get(name, 0)
    return [(values[2 * i], values[2 * i + 1]) for i in range(count) if 2 * i + 1 < len(values)]


def _bbox(ctm: content.Matrix, points: list[tuple[float, float]], paint: str, width: float) -> Rect:
    mapped = [(ctm[0] * x + ctm[2] * y + ctm[4], ctm[1] * x + ctm[3] * y + ctm[5]) for x, y in points]
    xs, ys = [p[0] for p in mapped], [p[1] for p in mapped]
    pad = width / 2 * (abs(ctm[0] * ctm[3] - ctm[1] * ctm[2]) ** 0.5) if paint in _STROKES else 0.0
    return (min(xs) - pad, min(ys) - pad, max(xs) + pad, max(ys) + pad)
