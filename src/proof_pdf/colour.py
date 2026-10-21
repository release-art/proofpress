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
import logging
import pathlib
import shutil
import tempfile

import pikepdf
from PIL import Image, ImageMath

from . import content, proc
from .models import Finding, Severity
from .paint import Fill, Walker, paint_events
from .spaces import CMYK, GRAY, RGB, classify_colour_space, intent_family

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
    """Note pages that paint anything but 100% K text with overprint on (advisory).

    Fires on what is painted, not on the ``gs`` that turns overprint on, so a
    graphics state that is applied but never painted under, or that only ever
    overprints black text, is silent.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        if any(p.overprint and not (p.kind == "text" and _is_full_k(p.fill)) for p in paint_events(page)):
            findings.append(
                Finding(
                    "overprint", Severity.WARN, "overprint is enabled -- confirm it is intentional, not knockout", i
                )
            )
    return findings


def check_white_overprint(pdf: pikepdf.Pdf) -> list[Finding]:
    """Flag white text or shapes set to overprint: overprinting white adds no ink, so it vanishes on press."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        whites = [
            p
            for p in paint_events(page)
            if p.overprint and _is_white(p.fill) and (p.kind == "text" or (p.kind == "path" and p.filled))
        ]
        if whites:
            findings.append(
                Finding(
                    "white-overprint",
                    Severity.ERROR,
                    f"{len(whites)} white object(s) are set to overprint and will disappear on press: "
                    "white must knock out",
                    i,
                )
            )
    return findings


def _is_white(fill: Fill) -> bool:
    if fill.family == GRAY:
        return bool(fill.value) and fill.value[0] >= 1.0 - _INK_EPSILON
    if fill.family == RGB:
        return len(fill.value) >= 3 and min(fill.value[:3]) >= 1.0 - _INK_EPSILON
    if fill.family == CMYK:
        return len(fill.value) >= 4 and max(fill.value[:4]) <= _INK_EPSILON
    return False


# ------------------------------------------------------------- spot inks ----
def check_spot_inks(pdf: pikepdf.Pdf, allowed: collections.abc.Collection[str]) -> list[Finding]:
    """Flag any spot ink a page selects that is not on the caller's list (names match exactly).

    A stray spot adds a plate and a charge. ``All`` is the registration colour: it prints on every plate, which is
    right for printer's marks and wrong for artwork, so it must be allowed by name.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        walker = Walker()
        for _ in walker.walk(page):
            pass
        unexpected = sorted(name for name in walker.spots - set(allowed) if name != "None")
        if unexpected:
            note = "; All is the registration colour and prints on every plate" if "All" in unexpected else ""
            findings.append(
                Finding(
                    "spot-ink",
                    Severity.ERROR,
                    f"spot ink(s) not on the allowed list: {', '.join(unexpected)}{note}",
                    i,
                )
            )
    return findings


# ---------------------------------------------------------- transparency ----
def check_transparency(pdf: pikepdf.Pdf) -> list[Finding]:
    """Check that a page using transparency blends in the colour space the job prints in.

    PDF/X-4 keeps live transparency, so the blend colour space decides how overlaps are computed. One that differs
    from the output intent's (RGB blending in a CMYK job) shifts colours at overlaps; one that is not declared leaves
    the choice to the RIP.
    """
    expected = intent_family(pdf)
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        walker = Walker()
        for _ in walker.walk(page):
            pass
        if not walker.transparency:
            continue
        reasons = ", ".join(sorted(walker.transparency))
        group = page.obj.get("/Group")
        space = group.get("/CS") if group is not None else None
        if space is None:
            findings.append(
                Finding(
                    "blend-space",
                    Severity.WARN,
                    f"the page uses transparency ({reasons}) but declares no blend colour space: the RIP chooses one",
                    i,
                )
            )
            continue
        family = classify_colour_space(space, page.obj.get("/Resources"))
        if expected is not None and family != expected:
            findings.append(
                Finding(
                    "blend-space",
                    Severity.ERROR,
                    f"transparency ({reasons}) blends in {family.upper()} but the output intent is "
                    f"{expected.upper()}: overlapping colours will shift",
                    i,
                )
            )
    return findings


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
    if classify_colour_space(xobject.get("/ColorSpace"), op.resources) != RGB:
        return None
    return str(op.operands[0])


# ------------------------------------------------ rich-black small text ----
def check_rich_black(pdf: pikepdf.Pdf, small_text_pt: float) -> list[Finding]:
    """Flag small text painted in a built (rich) black on any page (advisory)."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        sizes = [
            p.size
            for p in paint_events(page)
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


def _is_rich_black(fill: Fill) -> bool:
    if fill.family != CMYK or len(fill.value) < 4:
        return False
    c, m, y, k = fill.value[:4]
    return k >= _RICH_K_MIN and (c + m + y) >= _RICH_OTHER_MIN


# ------------------------------------------------------------ K-only text ----
def check_k_only_text(pdf: pikepdf.Pdf) -> list[Finding]:
    """Flag text painted with any ink but K: a four-colour build, RGB, or a spot or pattern."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        bad = [p.fill for p in paint_events(page) if p.kind == "text" and not _is_k_only(p.fill)]
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


def _is_k_only(fill: Fill) -> bool:
    """Gray, or CMYK with no C, M or Y: only the K plate (or no ink at all) is used."""
    if fill.family == GRAY:
        return True
    return fill.family == CMYK and max(fill.value[:3], default=0.0) <= _INK_EPSILON


def _is_full_k(fill: Fill) -> bool:
    """Solid black on the K plate alone: DeviceGray 0, or CMYK 0/0/0/100."""
    if fill.family == GRAY:
        return bool(fill.value) and fill.value[0] <= _INK_EPSILON
    return _is_k_only(fill) and len(fill.value) >= 4 and fill.value[3] >= 1.0 - _INK_EPSILON


def _describe(fill: Fill) -> str:
    if fill.family == CMYK and len(fill.value) >= 4:
        c, m, y, k = (round(v * 100) for v in fill.value[:4])
        return f"C{c} M{m} Y{y} K{k}"
    if fill.family == RGB:
        return "RGB"
    return "a spot, pattern or other non-K colour"
