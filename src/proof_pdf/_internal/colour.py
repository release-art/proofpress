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
import io
import logging
import pathlib
import shutil
import tempfile
import typing

import pikepdf
from PIL import Image, ImageCms, ImageMath

from ..models import ColourMode, Finding, Severity
from . import proc, spots
from .paint import Fill, Paint, Walker, paint_events
from .spaces import CMYK, GRAY, RGB, classify_colour_space, intent_family, output_intents, reachable, spot_names

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
        images, colours, _ = _paint_in(page, RGB)
        for name in sorted(images):
            findings.append(
                Finding(
                    "colour-space",
                    Severity.ERROR,
                    f"image {name} is in an RGB colour space -- it must be converted to CMYK for a FOGRA51 press",
                    i,
                )
            )
        if colours:
            findings.append(
                Finding(
                    "colour-space",
                    vector_severity,
                    "vector artwork is painted in RGB -- confirm the RIP is meant to convert it",
                    i,
                )
            )
    return findings


# ------------------------------------------------------------ RGB proofing ----
def check_rgb_only(pdf: pikepdf.Pdf) -> list[Finding]:
    """For a screen job: flag CMYK images and fills, a CMYK output intent, and (as advisory) spot colours.

    Gray is colour-neutral and passes. Shadings and patterns are looked through to the colour space they paint in.
    """
    findings: list[Finding] = []
    if intent_family(pdf) == CMYK:
        findings.append(Finding("rgb-only", Severity.ERROR, "the output intent is a CMYK profile in an RGB job"))
    for i, page in enumerate(pdf.pages, start=1):
        images, fills, spots = _paint_in(page, CMYK)
        for name in sorted(images):
            findings.append(Finding("rgb-only", Severity.ERROR, f"image {name} is in a CMYK colour space", i))
        if fills:
            findings.append(
                Finding(
                    "rgb-only", Severity.ERROR, f"artwork is painted in CMYK ({', '.join(sorted(fills)[:3])}...)", i
                )
            )
        if spots:
            findings.append(
                Finding("rgb-only", Severity.WARN, f"spot colour(s) {', '.join(sorted(spots))} have no RGB value", i)
            )
    return findings


def _paint_in(page: pikepdf.Page, family: str) -> tuple[set[str], set[str], set[str]]:
    """The image names and colour descriptions in *family*, and the spot ink names, a page paints by fill or stroke."""
    images: set[str] = set()
    colours: set[str] = set()
    spots: set[str] = set()
    for p in paint_events(page):
        if p.kind in ("image", "inline-image") and not p.mask:
            if p.colour_space == family:
                images.add(p.name)
            continue
        if p.kind == "shading":
            if p.colour_space == family:
                colours.add(f"shading {p.name}")
            continue
        for colour in p.colours():
            if colour.family == family or family in colour.pattern:
                colours.add(_describe(colour))
            elif colour.spot:
                spots.update(colour.spot)
    return images, colours, spots


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
        if _IMAGE_FILTERS & {str(f) for f in _filters(image)}:
            pil = pikepdf.PdfImage(typing.cast(pikepdf.Stream, image)).as_pil_image()
            if pil.mode != "CMYK":
                return unreadable
            used = [int(band[1]) for band in typing.cast(tuple, pil.getextrema())[:3]]
        else:
            used = _raw_ink(image)
    except pikepdf.PdfError, NotImplementedError, OSError, ValueError, SyntaxError, Image.DecompressionBombError:
        return unreadable
    if used is None:
        return unreadable
    return "has C, M or Y ink in a gray job" if any(used) else None


def _filters(image: pikepdf.Object) -> list:
    filters = image.get("/Filter")
    return [] if filters is None else list(filters) if isinstance(filters, pikepdf.Array) else [filters]


def _raw_ink(image: pikepdf.Object) -> list[int] | None:
    """The highest C, M and Y sample of an 8-bit image with no image filter (0 where that plate is empty), else None."""
    if int(image.get("/BitsPerComponent", 0)) != 8:
        return None
    data = bytes(image.read_bytes())
    decode = [float(v) for v in image.get("/Decode", [0, 1] * 4)]
    if len(decode) != 8 or any(d not in (0.0, 1.0) for d in decode):
        return None
    # An inverted channel (/Decode [1 0]) is empty when every sample is 255, so measure its distance from that.
    return [max(((255 - b) if decode[2 * c] else b) for b in data[c::4]) if data else 0 for c in range(3)]


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


# ------------------------------------------------------------ ICC profiles ----
# The data colour space in an ICC header (bytes 16-20) and how many components it has.
_ICC_SPACES = {b"GRAY": (GRAY, 1), b"RGB ": (RGB, 3), b"CMYK": (CMYK, 4), b"Lab ": ("lab", 3)}
_ICC_HEADER = 128


def check_icc_profiles(pdf: pikepdf.Pdf, mode: ColourMode) -> list[Finding]:
    """Check the ICC profiles the file embeds for colour spaces: sound, and consistent with the job.

    Every profile must be readable and its header must agree with the space's ``/N``. Then, by mode: in a CMYK job a
    CMYK profile other than the output intent's is a warning (legal in PDF/X-4, but the RIP converts through it); in an
    RGB job every RGB profile, the output intent's included, must be the same one, since mixed working spaces shift
    colour between objects.
    """
    findings: list[Finding] = []
    seen: dict[str, tuple[str, str]] = {}  # profile digest -> (family, description)
    for obj in reachable(pdf):
        if not (isinstance(obj, pikepdf.Array) and len(obj) > 1 and str(obj[0]) == "/ICCBased"):
            continue
        try:
            data = bytes(obj[1].read_bytes())
            components = int(obj[1].get("/N", 0))
        except pikepdf.PdfError, AttributeError, TypeError, ValueError:
            findings.append(
                Finding("icc-profile", Severity.ERROR, "an ICCBased colour space has an unreadable profile")
            )
            continue
        problem, family = _icc_problem(data, components)
        label = _icc_label(data)
        if problem:
            findings.append(Finding("icc-profile", Severity.ERROR, f"ICC profile {label}: {problem}"))
        elif family is not None:
            seen.setdefault(_profile_key(data, label), (family, label))
    return findings + _profile_consistency(seen, _intent_profile(pdf), mode)


def _profile_consistency(
    seen: dict[str, tuple[str, str]], intent: tuple[str, str, str] | None, mode: ColourMode
) -> list[Finding]:
    """The mode's rule over the distinct profiles *seen* and the output *intent*'s."""
    if mode is not ColourMode.CMYK:
        family = RGB if mode is ColourMode.RGB else GRAY
        same = {d: v[1] for d, v in seen.items() if v[0] == family}
        if intent is not None and intent[1] == family:
            same.setdefault(intent[0], intent[2])
        if len(same) > 1:
            label = family.upper() if family == RGB else family
            return [
                Finding(
                    "icc-profile", Severity.ERROR, f"the {label} job mixes {len(same)} {label} profiles: {_names(same)}"
                )
            ]
    elif intent is not None and intent[1] == CMYK:
        other = {d: v[1] for d, v in seen.items() if v[0] == CMYK and d != intent[0]}
        if other:
            return [
                Finding(
                    "icc-profile",
                    Severity.WARN,
                    f"CMYK profile(s) {_names(other)} differ from the output intent {intent[2]!r}: the RIP converts",
                )
            ]
    return []


def _icc_problem(data: bytes, components: int) -> tuple[str | None, str | None]:
    """``(problem, family)``: what is wrong with the profile bytes against ``/N``, and the family it is for."""
    if len(data) < _ICC_HEADER or data[36:40] != b"acsp":
        return "is not a valid ICC profile", None
    space = _ICC_SPACES.get(data[16:20])
    if space is None:
        return None, None
    if space[1] != components:
        return f"is a {data[16:20].decode().strip()} profile but the colour space declares /N {components}", None
    return None, space[0]


def _profile_key(data: bytes, label: str) -> str:
    """Which profile this is: its description and size. The same profile is often embedded byte-different (a rewritten
    header or profile ID), so a content hash would call it two."""
    return f"{label}:{len(data)}"


def _icc_label(data: bytes) -> str:
    try:
        return ImageCms.getProfileDescription(ImageCms.ImageCmsProfile(io.BytesIO(data))).strip() or "(unnamed)"
    except ImageCms.PyCMSError, OSError, AttributeError:
        return "(unreadable)"


def _intent_profile(pdf: pikepdf.Pdf) -> tuple[str, str, str] | None:
    """``(digest, family, description)`` of the first output intent's profile, or ``None``."""
    for intent in output_intents(pdf):
        profile = intent.get("/DestOutputProfile")
        if profile is None:
            continue
        try:
            data = bytes(profile.read_bytes())
        except pikepdf.PdfError:
            continue
        space = _ICC_SPACES.get(data[16:20])
        if space is not None:
            return _profile_key(data, _icc_label(data)), space[0], _icc_label(data)
    return None


def _names(profiles: dict[str, str]) -> str:
    return ", ".join(sorted(profiles.values()))


# ------------------------------------------------ rich-black small text ----
def check_rich_black(pdf: pikepdf.Pdf, small_text_pt: float) -> list[Finding]:
    """Flag small text painted in a built (rich) black on any page (advisory)."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        sizes = [
            p.size
            for p in paint_events(page)
            if p.kind == "text" and any(_is_rich_black(c) for c in p.colours()) and p.size <= small_text_pt
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
        bad = [c for p in paint_events(page) if p.kind == "text" for c in p.colours() if not _is_k_only(c)]
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
    if fill.pattern:
        return "a pattern using " + "/".join(sorted(f.upper() for f in fill.pattern))
    if fill.family == CMYK and len(fill.value) >= 4:
        c, m, y, k = (round(v * 100) for v in fill.value[:4])
        return f"C{c} M{m} Y{y} K{k}"
    if fill.family == RGB:
        return "RGB"
    return "a spot, pattern or other non-K colour"
