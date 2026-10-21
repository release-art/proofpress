"""Best-effort colour sanity -- the things neither PDF/A nor PDF/X judge.

Four checks a printer cares about but no standard enforces:

* **Total area coverage** -- the summed ink of the four plates. Rasterise each
  page to DeviceCMYK with Ghostscript and take the peak; over the FOGRA51
  ceiling the ink will not dry and will set off. This is the objective one, so
  it is an ``ERROR``.
* **Stray RGB** -- a raster still in an RGB space in a job destined for a CMYK
  press. Every raster should already be converted to CMYK, so an
  RGB image here means one escaped the workflow: an ``ERROR``. Vector RGB fills
  are advisory by default, since a RIP will convert them against the output
  intent; ``--strict-cmyk`` promotes them when that is not wanted.
* **Overprint** -- an applied ``ExtGState`` turning overprint on. Usually
  deliberate, so advisory: it is flagged, not failed.
* **Rich-black small text** -- small text painted in a built black (K plus other
  plates) mis-registers. Advisory.

The raster step shells out to Ghostscript (its absence or failure is surfaced as
a failure by the caller, not silently skipped); the rest read the
:class:`pikepdf.Pdf` through :mod:`.content`, so they see nested Form XObjects.
"""

import asyncio
import logging
import pathlib
import shutil
import tempfile

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


# ------------------------------------------------------------- overprint ----
def check_overprint(pdf: pikepdf.Pdf) -> list[Finding]:
    """Note pages that *apply* an ExtGState enabling overprint (advisory).

    Fires on the ``gs`` operator rather than on the resource dictionary, so it
    sees overprint set inside a Form XObject, and does not fire on a graphics
    state that is declared but never invoked.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        if any(_sets_overprint(op) for op in content.iter_operations(page)):
            findings.append(
                Finding(
                    "overprint", Severity.WARN, "overprint is enabled -- confirm it is intentional, not knockout", i
                )
            )
    return findings


def _sets_overprint(op: content.Operation) -> bool:
    if op.operator != "gs":
        return False
    state = _lookup_resource(op.resources, "/ExtGState", op.operands)
    if state is None:
        return False
    return _truthy(state.get("/OP")) or _truthy(state.get("/op"))


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
        smallest = _smallest_rich_black_text(page, small_text_pt)
        if smallest is not None:
            findings.append(
                Finding(
                    "rich-black",
                    Severity.WARN,
                    f"text as small as {smallest:.1f}pt is set in rich black -- "
                    "use process black to avoid mis-registration",
                    i,
                )
            )
    return findings


def _smallest_rich_black_text(page: pikepdf.Page, small_text_pt: float) -> float | None:
    """Effective point size of the smallest rich-black run on the page, if any.

    Text state (font size, fill, text matrix) is tracked across the whole walk
    rather than reset per Form XObject: a form that shows text essentially always
    sets its own ``Tf`` and ``BT`` first, and this check is advisory, so the
    extra reach costs nothing and buys visibility into nested content.
    """
    text_matrix: content.Matrix = content.IDENTITY
    font_size = 0.0
    fill: tuple[float, float, float, float] | None = None
    smallest: float | None = None

    for op in content.iter_operations(page):
        name, args = op.operator, op.operands
        if name == "BT":
            text_matrix = content.IDENTITY
        elif name == "Tm":
            text_matrix = content.matrix(args)
        elif name in ("Td", "TD"):
            text_matrix = content.mul(_translation(args), text_matrix)
        elif name == "Tf":
            font_size = _number(args, 1)
        elif name == "k":
            fill = _cmyk(args)
        elif name in ("g", "rg"):
            fill = None  # grayscale / RGB black is never rich black
        elif name in _SHOW_TEXT and _is_rich_black(fill):
            size = font_size * content.vertical_scale(content.mul(text_matrix, op.ctm))
            if size <= small_text_pt and (smallest is None or size < smallest):
                smallest = size
    return smallest


def _translation(args: list) -> content.Matrix:
    return (1.0, 0.0, 0.0, 1.0, _number(args, 0), _number(args, 1))


def _number(args: list, index: int) -> float:
    try:
        return float(args[index])
    except IndexError, TypeError, ValueError:
        return 0.0


def _cmyk(args: list) -> tuple[float, float, float, float] | None:
    if len(args) < 4:
        return None
    return (_number(args, 0), _number(args, 1), _number(args, 2), _number(args, 3))


def _is_rich_black(fill: tuple[float, float, float, float] | None) -> bool:
    if fill is None:
        return False
    c, m, y, k = fill
    return k >= _RICH_K_MIN and (c + m + y) >= _RICH_OTHER_MIN
