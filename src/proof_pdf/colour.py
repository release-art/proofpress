"""Best-effort colour sanity -- the things neither PDF/A nor PDF/X judge.

Checks a printer cares about but no standard enforces:

* **Total area coverage** -- the summed ink of the four plates. Rasterise each
  page to DeviceCMYK with Ghostscript and take the peak; over the FOGRA51
  ceiling the ink will not dry and will set off. This is the objective one, so
  it is an ``ERROR``.
* **Stray RGB** -- a raster still in an RGB space in a job destined for a CMYK
  press. Every raster should already be converted to CMYK, so an
  RGB image here means one escaped the workflow: an ``ERROR``. Vector RGB fills
  are advisory by default, since a RIP will convert them against the output
  intent; ``--strict-cmyk`` promotes them when that is not wanted.
* **Overprint** -- content painted while an ``ExtGState`` has overprint on.
  Overprinting 100% K text is the normal, safe case and passes; anything else
  is usually a knockout that was meant, so it is advisory: flagged, not failed.
* **Rich-black small text** -- small text painted in a built black (K plus other
  plates) mis-registers. Advisory.
* **K-only text** (opt-in) -- text painted with any ink but K. One plate cannot
  mis-register with itself, so a house rule may require it: an error.

The raster step shells out to Ghostscript (its absence or failure is surfaced as
a failure by the caller, not silently skipped); the rest read the
:class:`pikepdf.Pdf` through :mod:`.content`, so they see nested Form XObjects.
"""

import asyncio
import collections.abc
import dataclasses
import logging
import pathlib
import shutil
import tempfile
import typing

import pikepdf
from PIL import Image, ImageMath

from . import content, proc
from .models import Finding, Severity

logger = logging.getLogger(__name__)


class GhostscriptError(RuntimeError):
    """Ghostscript exited non-zero, or timed out, while rasterising for ink coverage."""


# Rasterising DeviceCMYK: 150 dpi resolves solids and small type without the
# per-pixel cost of press resolution. TAC is an ink property, so more dpi buys
# nothing here.
_TAC_DPI = 150
# A built black: the K plate is heavy and at least this much other ink rides on
# top. Pure process black (0/0/0/K) never trips it.
_RICH_K_MIN = 0.5
_RICH_OTHER_MIN = 0.3
# An ink amount this close to zero is no ink, and this close to one is solid.
_INK_EPSILON = 0.01

# Colour-space families, as classified from a PDF colour space object.
_RGB = "rgb"
_CMYK = "cmyk"
_GRAY = "gray"
_OTHER = "other"

_DEVICE_FAMILIES = {
    "/DeviceRGB": _RGB,
    "/CalRGB": _RGB,
    "/Lab": _OTHER,
    "/DeviceCMYK": _CMYK,
    "/DeviceGray": _GRAY,
    "/CalGray": _GRAY,
    "/Pattern": _OTHER,
}
# Spot and multi-ink spaces carry an *alternate* space that is only a preview
# hint -- a Separation whose alternate is DeviceRGB is a correct spot colour, not
# stray RGB. Never look past these.
_SPOT_FAMILIES = ("/Separation", "/DeviceN")
_ICC_COMPONENTS = {1: _GRAY, 3: _RGB, 4: _CMYK}

# Text-showing operators: whatever fill is in force when one runs is the colour
# the glyphs are painted in.
_SHOW_TEXT = ("Tj", "TJ", "'", '"')


def gs_available(binary: str) -> bool:
    """Whether the Ghostscript *binary* resolves (TAC needs it, nothing else)."""
    return shutil.which(binary) is not None


# ------------------------------------------------------------ total ink ----
async def check_tac(binary: str, path: str, max_tac: float, *, timeout: float | None = None) -> list[Finding]:
    """Flag any page whose peak total ink coverage exceeds *max_tac* percent."""
    findings: list[Finding] = []
    for i, peak in enumerate(await _page_tac_peaks(binary, path, timeout=timeout), start=1):
        if peak > max_tac:
            findings.append(
                Finding("tac", Severity.ERROR, f"peak ink coverage {peak:.0f}% exceeds the {max_tac:.0f}% limit", i)
            )
    return findings


async def _page_tac_peaks(binary: str, path: str, *, timeout: float | None = None) -> list[float]:
    """The peak TAC (0-400%) of each page, via a DeviceCMYK Ghostscript raster."""
    with tempfile.TemporaryDirectory() as tmp:
        pattern = str(pathlib.Path(tmp) / "page-%03d.tif")
        try:
            result = await proc.run(
                [
                    binary,
                    "-q",
                    "-dNOPAUSE",
                    "-dBATCH",
                    "-dSAFER",
                    "-sDEVICE=tiff32nc",
                    f"-r{_TAC_DPI}",
                    f"-sOutputFile={pattern}",
                    path,
                ],
                timeout=timeout,
            )
        except proc.ProcTimeoutError as exc:
            raise GhostscriptError(f"ghostscript timed out after {timeout:.0f}s") from exc
        if result.returncode != 0:
            message = result.stderr.strip() or "ghostscript failed"
            logger.error(f"ghostscript failed (rc={result.returncode}): {message}")
            raise GhostscriptError(message)
        # Sort by the numeric page index, not lexicographically: `page-1000.tif`
        # must not sort before `page-999.tif` (wrong page labels past 999).
        pages = sorted(pathlib.Path(tmp).glob("page-*.tif"), key=lambda t: int(t.stem.rsplit("-", 1)[-1]))
        peaks: list[float] = []
        for tif in pages:
            peaks.append(await asyncio.to_thread(_peak_tac, tif))
            # A 50-page file at 150 dpi is ~150 MB of TIFF if it all stays on
            # disk; each raster is dead the moment its peak is read.
            tif.unlink(missing_ok=True)
        return peaks


def _peak_tac(tif: pathlib.Path) -> float:
    """Peak C+M+Y+K over every pixel of a CMYK TIFF, as a percentage."""
    with Image.open(tif) as img:
        img.load()
        c, m, y, k = (band.convert("I") for band in img.split())
        total = ImageMath.lambda_eval(lambda a: a["cc"] + a["mm"] + a["yy"] + a["kk"], cc=c, mm=m, yy=y, kk=k)
    peak = total.getextrema()[1]
    return peak / 255.0 * 100.0


# --------------------------------------------------------- painting walk ----
@dataclasses.dataclass(frozen=True)
class _Fill:
    """The current fill colour: its family and, for device spaces, its components."""

    family: str
    value: tuple[float, ...] = ()


# What a colour space's colour is straight after ``cs``, per the PDF spec.
_INITIAL_FILL = {
    _GRAY: _Fill(_GRAY, (0.0,)),
    _RGB: _Fill(_RGB, (0.0, 0.0, 0.0)),
    _CMYK: _Fill(_CMYK, (0.0, 0.0, 0.0, 1.0)),
    _OTHER: _Fill(_OTHER),
}


@dataclasses.dataclass(frozen=True)
class _State:
    """The slice of the graphics state these checks read. A new page starts in DeviceGray black."""

    fill: _Fill = _INITIAL_FILL[_GRAY]
    font_size: float = 0.0
    overprint: bool = False


@dataclasses.dataclass(frozen=True)
class _Paint:
    """One thing painted on the page, with the state it was painted in."""

    kind: str  # "text", "path", "image" or "shading"
    fill: _Fill
    overprint: bool
    size: float = 0.0  # effective point size; text only


_PATH_PAINT = frozenset(("f", "F", "f*", "B", "B*", "b", "b*", "S", "s"))


class _Walker:
    """Walks a page's operators keeping the :class:`_State` they would leave a viewer in.

    ``q``/``Q`` restore the state, and so does leaving a Form XObject (a form runs
    in its own graphics state), which is why this is not a flat scan of the
    operators. Painting operators are reported; everything else only updates state.
    """

    def __init__(self) -> None:
        self.state = _State()
        self.entered: list[_State] = []  # the state to restore when leaving each form
        self.saved: dict[int, list[_State]] = {0: []}  # q-stack per nesting depth
        self.depth = 0
        self.text_matrix: content.Matrix = content.IDENTITY

    def walk(self, page: pikepdf.Page) -> collections.abc.Iterator[_Paint]:
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

    def _paint(self, op: content.Operation) -> _Paint | None:
        state, name = self.state, op.operator
        if name in _SHOW_TEXT:
            size = state.font_size * content.vertical_scale(content.mul(self.text_matrix, op.ctm))
            return _Paint("text", state.fill, state.overprint, size)
        if name in _PATH_PAINT:
            return _Paint("path", state.fill, state.overprint)
        if name == "sh":
            return _Paint("shading", state.fill, state.overprint)
        if name == "Do":
            xobject = content.lookup_xobject(op.resources, op.operands)
            if xobject is not None and str(xobject.get("/Subtype")) == "/Image":
                return _Paint("image", state.fill, state.overprint)
        return None

    def _set(self, **changes: object) -> None:
        self.state = dataclasses.replace(self.state, **changes)  # pyright: ignore[reportArgumentType]

    def _save(self, op: content.Operation) -> None:
        self.saved[self.depth].append(self.state)

    def _restore(self, op: content.Operation) -> None:
        if self.saved[self.depth]:
            self.state = self.saved[self.depth].pop()

    def _begin_text(self, op: content.Operation) -> None:
        self.text_matrix = content.IDENTITY

    def _text_matrix(self, op: content.Operation) -> None:
        self.text_matrix = content.matrix(op.operands)

    def _move_text(self, op: content.Operation) -> None:
        self.text_matrix = content.mul(_translation(op.operands), self.text_matrix)

    def _font(self, op: content.Operation) -> None:
        self._set(font_size=_number(op.operands, 1))

    def _gray(self, op: content.Operation) -> None:
        self._set(fill=_Fill(_GRAY, (_number(op.operands, 0),)))

    def _rgb(self, op: content.Operation) -> None:
        self._set(fill=_Fill(_RGB, tuple(_number(op.operands, i) for i in range(3))))

    def _cmyk(self, op: content.Operation) -> None:
        self._set(fill=_Fill(_CMYK, tuple(_number(op.operands, i) for i in range(4))))

    def _colour_space(self, op: content.Operation) -> None:
        family = classify_colour_space(op.operands[0] if op.operands else None, op.resources)
        self._set(fill=_INITIAL_FILL[family])

    def _components(self, op: content.Operation) -> None:
        self._set(fill=_with_components(self.state.fill, op.operands))

    def _graphics_state(self, op: content.Operation) -> None:
        overprint = _overprint_in(op)
        if overprint is not None:
            self._set(overprint=overprint)

    _UPDATES: typing.ClassVar[dict[str, collections.abc.Callable[["_Walker", content.Operation], None]]] = {
        "q": _save,
        "Q": _restore,
        "BT": _begin_text,
        "Tm": _text_matrix,
        "Td": _move_text,
        "TD": _move_text,
        "Tf": _font,
        "g": _gray,
        "rg": _rgb,
        "k": _cmyk,
        "cs": _colour_space,
        "sc": _components,
        "scn": _components,
        "gs": _graphics_state,
    }


def _paint_events(page: pikepdf.Page) -> collections.abc.Iterator[_Paint]:
    """Everything *page* paints, with the fill, font size and overprint in force."""
    return _Walker().walk(page)


def _with_components(fill: _Fill, args: list) -> _Fill:
    """The fill after ``sc``/``scn``: new components in the same family, or a pattern/spot."""
    if args and isinstance(args[-1], pikepdf.Name):  # scn ... /Pattern
        return _Fill(_OTHER)
    if fill.family in (_GRAY, _RGB, _CMYK):
        return _Fill(fill.family, tuple(_number(args, i) for i in range(len(args))))
    return fill  # a spot or DeviceN tint: still not a device colour


def _overprint_in(op: content.Operation) -> bool | None:
    """Overprint as an applied ``gs`` sets it; ``None`` when it says nothing about overprint."""
    state = _lookup_resource(op.resources, "/ExtGState", op.operands)
    if state is None or (state.get("/OP") is None and state.get("/op") is None):
        return None
    return _truthy(state.get("/OP")) or _truthy(state.get("/op"))


# ------------------------------------------------------------- overprint ----
def check_overprint(pdf: pikepdf.Pdf) -> list[Finding]:
    """Note pages that paint anything but 100% K text with overprint on (advisory).

    Fires on what is painted, not on the ``gs`` that turns overprint on, so a
    graphics state that is applied but never painted under, or that only ever
    overprints black text, is silent.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        if any(p.overprint and not (p.kind == "text" and _is_full_k(p.fill)) for p in _paint_events(page)):
            findings.append(
                Finding(
                    "overprint", Severity.WARN, "overprint is enabled -- confirm it is intentional, not knockout", i
                )
            )
    return findings


def _lookup_resource(resources: pikepdf.Object | None, category: str, operands: list) -> pikepdf.Object | None:
    """Resolve a name operand against one category of a resource dictionary."""
    if resources is None or not operands:
        return None
    table = resources.get(category)
    if table is None:
        return None
    try:
        return table.get(str(operands[0]))
    except KeyError, TypeError:
        return None


def _truthy(value: object) -> bool:
    """A PDF boolean object read as a plain bool."""
    return value is not None and str(value) == "True"


# ---------------------------------------------------------- colour space ----
def check_colour_spaces(pdf: pikepdf.Pdf, *, strict: bool) -> list[Finding]:
    """Flag artwork still in an RGB colour space on a CMYK-destined page.

    Images are an ``ERROR``: every raster is expected to have gone through CMYK
    conversion, so one in RGB is an escape that will be converted by the
    RIP with whatever profile it feels like. Vector ``rg``/``RG`` fills are a
    ``WARN`` unless *strict*, since the document legitimately declares an RGB
    profile too.
    """
    findings: list[Finding] = []
    vector_severity = Severity.ERROR if strict else Severity.WARN
    for i, page in enumerate(pdf.pages, start=1):
        images: set[str] = set()
        vector = False
        for op in content.iter_operations(page):
            if op.operator in ("rg", "RG"):
                vector = True
            elif op.operator == "Do":
                name = _rgb_image_name(op)
                if name is not None:
                    images.add(name)
        for name in sorted(images):
            findings.append(
                Finding(
                    "colour-space",
                    Severity.ERROR,
                    f"image {name} is in an RGB colour space -- it must be converted to CMYK for a FOGRA51 press",
                    i,
                )
            )
        if vector:
            findings.append(
                Finding(
                    "colour-space",
                    vector_severity,
                    "vector artwork is filled in DeviceRGB -- confirm the RIP is meant to convert it",
                    i,
                )
            )
    return findings


def _rgb_image_name(op: content.Operation) -> str | None:
    """The name of the image this ``Do`` paints, if that image is in an RGB space."""
    xobject = content.lookup_xobject(op.resources, op.operands)
    if xobject is None or str(xobject.get("/Subtype")) != "/Image":
        return None
    if classify_colour_space(xobject.get("/ColorSpace"), op.resources) != _RGB:
        return None
    return str(op.operands[0])


def classify_colour_space(space: object, resources: pikepdf.Object | None, *, depth: int = 0) -> str:
    """Which colour family a PDF colour space object belongs to.

    Resolves named spaces through the resource dictionary and looks through
    ``/Indexed`` to its base, but deliberately stops at ``/Separation`` and
    ``/DeviceN``: their alternate space is a rendering hint for a spot ink, and
    treating it as the artwork's own space produces false positives on entirely
    correct print files.
    """
    if space is None or depth > 4:
        return _OTHER
    if isinstance(space, pikepdf.Array) or (hasattr(space, "__len__") and not isinstance(space, pikepdf.Name)):
        return _classify_array(space, resources, depth)
    name = str(space)
    if name in _DEVICE_FAMILIES:
        return _DEVICE_FAMILIES[name]
    named = _lookup_resource(resources, "/ColorSpace", [name])
    if named is not None:
        return classify_colour_space(named, resources, depth=depth + 1)
    return _OTHER


def _classify_array(space, resources: pikepdf.Object | None, depth: int) -> str:
    try:
        head = str(space[0])
    except IndexError, TypeError:
        return _OTHER
    if head in _SPOT_FAMILIES:
        return _OTHER
    if head == "/ICCBased":
        return _classify_icc(space)
    if head == "/Indexed":
        base = space[1] if len(space) > 1 else None
        return classify_colour_space(base, resources, depth=depth + 1)
    if head in _DEVICE_FAMILIES:
        return _DEVICE_FAMILIES[head]
    return _OTHER


def _classify_icc(space) -> str:
    """An ``[/ICCBased stream]`` space, classified by the stream's ``/N``."""
    try:
        components = int(space[1].get("/N"))
    except IndexError, TypeError, ValueError, AttributeError:
        return _OTHER
    return _ICC_COMPONENTS.get(components, _OTHER)


# ------------------------------------------------ rich-black small text ----
def check_rich_black(pdf: pikepdf.Pdf, small_text_pt: float) -> list[Finding]:
    """Flag small text painted in a built (rich) black on any page (advisory)."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        sizes = [
            p.size
            for p in _paint_events(page)
            if p.kind == "text" and _is_rich_black(p.fill) and p.size <= small_text_pt
        ]
        if sizes:
            findings.append(
                Finding(
                    "rich-black",
                    Severity.WARN,
                    f"text as small as {min(sizes):.1f}pt is set in rich black -- "
                    "use process black to avoid mis-registration",
                    i,
                )
            )
    return findings


def _is_rich_black(fill: _Fill) -> bool:
    if fill.family != _CMYK or len(fill.value) < 4:
        return False
    c, m, y, k = fill.value[:4]
    return k >= _RICH_K_MIN and (c + m + y) >= _RICH_OTHER_MIN


# ------------------------------------------------------------ K-only text ----
def check_k_only_text(pdf: pikepdf.Pdf) -> list[Finding]:
    """Flag text painted with any ink but K: a four-colour build, RGB, or a spot or pattern."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        bad = [p.fill for p in _paint_events(page) if p.kind == "text" and not _is_k_only(p.fill)]
        if bad:
            more = f" (and {len(bad) - 1} more runs)" if len(bad) > 1 else ""
            findings.append(
                Finding(
                    "k-only-text",
                    Severity.ERROR,
                    f"text is painted in {_describe(bad[0])}{more} -- text ink must be K only",
                    i,
                )
            )
    return findings


def _is_k_only(fill: _Fill) -> bool:
    """Gray, or CMYK with no C, M or Y: only the K plate (or no ink at all) is used."""
    if fill.family == _GRAY:
        return True
    return fill.family == _CMYK and max(fill.value[:3], default=0.0) <= _INK_EPSILON


def _is_full_k(fill: _Fill) -> bool:
    """Solid black on the K plate alone: DeviceGray 0, or CMYK 0/0/0/100."""
    if fill.family == _GRAY:
        return bool(fill.value) and fill.value[0] <= _INK_EPSILON
    return _is_k_only(fill) and len(fill.value) >= 4 and fill.value[3] >= 1.0 - _INK_EPSILON


def _describe(fill: _Fill) -> str:
    if fill.family == _CMYK and len(fill.value) >= 4:
        c, m, y, k = (round(v * 100) for v in fill.value[:4])
        return f"C{c} M{m} Y{y} K{k}"
    if fill.family == _RGB:
        return "RGB"
    return "a spot, pattern or other non-K colour"


def _translation(args: list) -> content.Matrix:
    return (1.0, 0.0, 0.0, 1.0, _number(args, 0), _number(args, 1))


def _number(args: list, index: int) -> float:
    try:
        return float(args[index])
    except IndexError, TypeError, ValueError:
        return 0.0
