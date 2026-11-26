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

Overprint, white overprint, rich-black small text, K-only text, the blend space, spot
inks on the allowed list, the CMYK and RGB job rules and the ICC profile rules run in
the Rust core now (``rust/src/checks/ink.rs`` and ``modes.rs``, through :mod:`.ported`).

The raster step shells out to Ghostscript (its absence or failure is surfaced as
a failure by the caller, not silently skipped); the rest read the
:class:`pikepdf.Pdf` through :mod:`.content`, so they see nested Form XObjects.
"""

import asyncio
import logging
import pathlib
import shutil
import tempfile
import typing

import pikepdf
from PIL import Image, ImageMath

from ..models import Finding, Severity
from . import proc, spots
from .paint import Fill, Paint, paint_events
from .spaces import CMYK, GRAY, RGB, intent_family, reachable, spot_names

logger = logging.getLogger(__name__)


class GhostscriptError(RuntimeError):
    """Ghostscript exited non-zero, or timed out, while rasterising for ink coverage."""


# Rasterising DeviceCMYK: 150 dpi resolves solids and small type without the
# per-pixel cost of press resolution. TAC is an ink property, so more dpi buys
# nothing here.
_TAC_DPI = 150
# An ink amount this close to zero is no ink, and this close to one is solid.
_INK_EPSILON = 0.01


def gs_available(binary: str) -> bool:
    """Whether the Ghostscript *binary* resolves (TAC needs it, nothing else)."""
    return shutil.which(binary) is not None


# ------------------------------------------------------------ total ink ----
async def check_tac(
    binary: str, path: str, max_tac: float, *, warn_tac: float | None = None, timeout: float | None = None
) -> list[Finding]:
    """Flag any page whose peak total ink coverage exceeds *max_tac* percent (and, as a warning, *warn_tac*)."""
    findings: list[Finding] = []
    for i, peak in enumerate(await _page_tac_peaks(binary, path, timeout=timeout), start=1):
        if peak > max_tac:
            findings.append(
                Finding("tac", Severity.ERROR, f"peak ink coverage {peak:.0f}% exceeds the {max_tac:.0f}% limit", i)
            )
        elif warn_tac is not None and peak > warn_tac:
            findings.append(
                Finding(
                    "tac", Severity.WARN, f"peak ink coverage {peak:.0f}% exceeds the {warn_tac:.0f}% advisory level", i
                )
            )
    return findings


async def _page_tac_peaks(binary: str, path: str, *, timeout: float | None = None) -> list[float]:
    """The peak total ink (100% per ink) of each page, from Ghostscript's ``tiffsep``: one plate per ink.

    Each spot colour is its own plate at its tint, never its CMYK alternate, and a spot overprinting process ink adds to
    it, as on press (decided 2026-11-07). ``tiff32nc`` converted spots to CMYK and dropped overprinted ones.
    """
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
                    "-sDEVICE=tiffsep",
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
        # tiffsep writes page-NNN.tif (a composite, unused) and page-NNN(Ink).tif per plate. Sort by the numeric page
        # index, not lexicographically: `page-1000` must not sort before `page-999`.
        pages = sorted(pathlib.Path(tmp).glob("page-[0-9]*[0-9].tif"), key=lambda t: int(t.stem.rsplit("-", 1)[-1]))
        peaks: list[float] = []
        for composite in pages:
            plates = sorted(composite.parent.glob(f"{composite.stem}(*).tif"))
            peaks.append(await asyncio.to_thread(_peak_tac, plates))
            # A 50-page file at 150 dpi is hundreds of MB of plates if it all stays on disk; each page's plates are
            # dead the moment its peak is read.
            for tif in (composite, *plates):
                tif.unlink(missing_ok=True)
        return peaks


def _peak_tac(plates: list[pathlib.Path]) -> float:
    """Peak ink summed over every plate at each pixel, as a percentage. A ``tiffsep`` plate is 0 for full ink."""
    total = None
    for tif in plates:
        with Image.open(tif) as img:
            ink = ImageMath.lambda_eval(lambda a: 255 - a["v"], v=img.convert("I"))
        total = ink if total is None else ImageMath.lambda_eval(lambda a: a["t"] + a["i"], t=total, i=ink)
    return 0.0 if total is None else total.getextrema()[1] / 255.0 * 100.0


# ------------------------------------------------------------- gray proofing ----
def check_gray_only(pdf: pikepdf.Pdf, *, allow_k_only: bool) -> list[Finding]:
    """For a gray job: flag anything painted outside the gray family, and an RGB (or, strictly, CMYK) output intent.

    Gray means DeviceGray, CalGray or ICC gray. Spot colours are a warning, as in an RGB job: a spot has no gray value.
    With *allow_k_only*, DeviceCMYK that uses the K plate alone also counts: vector colours by their components,
    images by decoding their pixels once per image object. ICC-based and Indexed CMYK, CMYK shadings and patterns, and
    inline CMYK images are not accepted, since a K-only value there does not mean gray (or cannot be read).
    """
    findings: list[Finding] = []
    intent = intent_family(pdf)
    if intent == RGB or (intent == CMYK and not allow_k_only):
        findings.append(
            Finding("gray-only", Severity.ERROR, f"the output intent is {_PROFILE_NAME[intent]} in a gray job")
        )
    decoded: dict[tuple[int, int], str | None] = {}  # image object -> what is wrong with it, decoded once
    for i, page in enumerate(pdf.pages, start=1):
        images, colours, spots = _gray_paint(page, allow_k_only, decoded)
        for name, why in sorted(images.items()):
            findings.append(Finding("gray-only", Severity.ERROR, f"image {name} {why}", i))
        if colours:
            findings.append(
                Finding(
                    "gray-only",
                    Severity.ERROR,
                    f"artwork is painted outside the gray family ({', '.join(sorted(colours)[:3])}...)",
                    i,
                )
            )
        if spots:
            findings.append(
                Finding("gray-only", Severity.WARN, f"spot colour(s) {', '.join(sorted(spots))} have no gray value", i)
            )
    return findings


def _gray_paint(
    page: pikepdf.Page, allow_k_only: bool, decoded: dict[tuple[int, int], str | None]
) -> tuple[dict[str, str], set[str], set[str]]:
    """What a page paints outside gray: image name -> why, colour descriptions, and the spot ink names."""
    images: dict[str, str] = {}
    colours: set[str] = set()
    spots: set[str] = set()
    for p in paint_events(page):
        if p.kind in ("image", "inline-image", "shading") and not p.mask:
            names = spot_names(p.xobject.get("/ColorSpace"), None) if p.xobject is not None else []
            spots.update(names)
            if names or p.colour_space == GRAY:
                continue
            if p.kind == "shading":
                colours.add(f"shading {p.name}")
            elif (why := _gray_image_problem(p, allow_k_only, decoded)) is not None:
                images[p.name] = why
            continue
        for colour in p.colours():
            spots.update(colour.spot)
            if not colour.spot and (why := _gray_colour_problem(colour, allow_k_only)) is not None:
                colours.add(why)
    return images, colours, spots


_PROFILE_NAME = {RGB: "an RGB profile", CMYK: "a CMYK profile"}
_IMAGE_FILTERS = frozenset(("/DCTDecode", "/JPXDecode", "/CCITTFaxDecode", "/JBIG2Decode"))


def _gray_colour_problem(colour: Fill, allow_k_only: bool) -> str | None:
    """Why a painted colour is not gray, or ``None`` when it is (or is K-only CMYK and that is allowed)."""
    if colour.pattern:
        return None if colour.pattern == {GRAY} else _describe(colour)
    if colour.family == GRAY:
        return None
    if colour.family == CMYK:
        if not allow_k_only:
            return _describe(colour)
        if colour.profiled:
            return f"{_describe(colour)} in an ICC-based or indexed space (not treated as K-only)"
        return None if _is_k_only(colour) and len(colour.value) >= 4 else _describe(colour)
    return "RGB" if colour.family == RGB else "Lab or another non-gray colour"


def _gray_image_problem(p: Paint, allow_k_only: bool, decoded: dict[tuple[int, int], str | None]) -> str | None:
    """Why an image is not gray, as the rest of a sentence starting with its name, or ``None`` when it is."""
    if p.colour_space == GRAY:
        return None
    if p.colour_space != CMYK:
        return "is in an RGB colour space in a gray job" if p.colour_space == RGB else "is not in a gray colour space"
    if not allow_k_only:
        return "is in a CMYK colour space in a gray job"
    image = p.xobject
    if image is None or str(image.get("/ColorSpace")) != "/DeviceCMYK":
        return "is ICC-based or indexed CMYK, which is not treated as K-only"
    if p.kind == "inline-image":
        return "is an inline CMYK image: inline images are not decoded, so K-only cannot be confirmed"
    if image.objgen not in decoded:
        decoded[image.objgen] = _cmy_problem(image)
    return decoded[image.objgen]


def _cmy_problem(image: pikepdf.Object) -> str | None:
    """Why a DeviceCMYK image is not K-only: it has C, M or Y ink, or its pixels cannot be read."""
    unreadable = "could not be decoded, so K-only cannot be confirmed"
    try:
        if int(image.get("/BitsPerComponent", 0)) != 8:
            return unreadable
        if _IMAGE_FILTERS & {str(f) for f in _filters(image)}:
            pil = pikepdf.PdfImage(typing.cast(pikepdf.Stream, image)).as_pil_image()
            if pil.mode != "CMYK":
                return unreadable
            samples = pil.tobytes()
            # Pillow inverts the samples of an Adobe CMYK JPEG (APP14 marker); a PDF reader does not, and leaves
            # inversion to /Decode. Undo Pillow's so the samples are the ones Ghostscript prints from.
            if "adobe" in pil.info:
                samples = bytes(255 - b for b in samples)
        else:
            samples = bytes(image.read_bytes())
        used = _cmy_ink(image, samples)
    except pikepdf.PdfError, NotImplementedError, OSError, ValueError, SyntaxError, Image.DecompressionBombError:
        return unreadable
    if used is None:
        return unreadable
    return "has C, M or Y ink in a gray job" if any(used) else None


def _filters(image: pikepdf.Object) -> list:
    filters = image.get("/Filter")
    return [] if filters is None else list(filters) if isinstance(filters, pikepdf.Array) else [filters]


def _cmy_ink(image: pikepdf.Object, samples: bytes) -> list[int] | None:
    """The highest C, M and Y ink in 8-bit CMYK samples after the image's /Decode (0 where that plate is empty).

    ``None`` when /Decode is not a plain or inverted mapping. The test is for exactly zero ink: a sample one code
    from empty is ink, whatever a JPEG decoder's rounding (decoders differ by a code on the same file).
    """
    decode = [float(v) for v in image.get("/Decode", [0, 1] * 4)]
    if len(decode) != 8 or any(d not in (0.0, 1.0) for d in decode):
        return None
    # An inverted channel (/Decode [1 0]) is empty when every sample is 255, so measure its distance from that.
    return [max(((255 - b) if decode[2 * c] else b) for b in samples[c::4]) if samples else 0 for c in range(3)]


# --------------------------------------------------------- spot definitions ----
def check_spot_definitions(pdf: pikepdf.Pdf) -> list[Finding]:
    """Flag a spot ink defined more than one way: same name, different alternate space or full-tint look.

    The name is what a printer matches to an ink, but the alternate is what proofs and the screen show. Two
    definitions of PANTONE 165 C mean two different oranges in the proof, and usually mean one file came from another
    workflow. Only definitions whose tint transform could be evaluated are compared, to within 1%, and only against
    definitions in the same alternate space.
    """
    # ponytail: alternates in different spaces (Black as Gray 0 vs CMYK 0/0/0/100) are not compared, as that needs a
    # conversion; add one if mixed-space definitions turn out to matter.
    definitions: dict[tuple[tuple[str, ...], str | None], dict[tuple, str]] = {}
    for obj in reachable(pdf):
        if not (isinstance(obj, pikepdf.Array) and len(obj) >= 4 and str(obj[0]) in ("/Separation", "/DeviceN")):
            continue
        try:
            spot = spots.read(obj)
        except IndexError, KeyError, TypeError, ValueError, pikepdf.PdfError:
            continue
        if spot.alternate is None or any(n in ("All", "None") for n in spot.names):
            continue
        look = (spot.alternate_space, tuple(round(v, 2) for v in spot.alternate))
        definitions.setdefault((spot.names, spot.alternate_space), {}).setdefault(look, f"{spot.alternate}")
    return [
        Finding(
            "spot-definition",
            Severity.ERROR,
            f"spot ink {' + '.join(names)} is defined {len(looks)} different ways in {space}: "
            f"{'; '.join(looks.values())}",
        )
        for (names, space), looks in sorted(definitions.items(), key=str)
        if len(looks) > 1
    ]


# ---------------------------------------------- K-only colours (gray proofing) ----
def _is_k_only(fill: Fill) -> bool:
    """Gray, or CMYK with no C, M or Y: only the K plate (or no ink at all) is used."""
    if fill.family == GRAY:
        return True
    return fill.family == CMYK and max(fill.value[:3], default=0.0) <= _INK_EPSILON


def _describe(fill: Fill) -> str:
    if fill.pattern:
        return "a pattern using " + "/".join(sorted(f.upper() for f in fill.pattern))
    if fill.family == CMYK and len(fill.value) >= 4:
        c, m, y, k = (round(v * 100) for v in fill.value[:4])
        return f"C{c} M{m} Y{y} K{k}"
    if fill.family == RGB:
        return "RGB"
    return "a spot, pattern or other non-K colour"
