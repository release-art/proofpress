"""The PDF/X-4 structural layer veraPDF does not check.

PDF/A has no opinion on page boxes, ``/Trapped`` or the PDF/X output intent, so
dual PDF/X-4 + PDF/A files get zero coverage on exactly the things a printer
bounces a job for. These checks close that gap against a :class:`pikepdf.Pdf`.

:func:`check_geometry` goes one step further than the standards do: it holds the
declared TrimBox and BleedBox to the size the artwork was *designed* for. PDF/X
only requires that the boxes exist and nest correctly, so a file can be a
perfectly valid PDF/X-4 and still be trimmed to the wrong size.
"""

import io

import pikepdf
from PIL import ImageCms

from .models import Finding, PageGeometry, Severity, mm_to_pt, pt_to_mm
from .paint import paint_events

# A box that should sit inside another may exceed it by this many points before
# it is a real error -- guards against rounding in the millipoint box values.
_BOX_TOLERANCE = 0.5

Rect = tuple[float, float, float, float]

# Two pages whose finished sizes differ by less than this are the same size.
_UNIFORM_TOLERANCE_MM = 0.1


def _rect(obj: object) -> Rect | None:
    """Normalise a PDF box object to ``(x0, y0, x1, y1)`` with sorted corners."""
    if obj is None:
        return None
    try:
        values = [float(v) for v in obj]  # pyright: ignore[reportGeneralTypeIssues]
    except TypeError, ValueError:
        return None
    if len(values) != 4:
        return None
    x0, y0, x1, y1 = values
    return (min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1))


def _contains(outer: Rect, inner: Rect) -> bool:
    """Whether *inner* sits within *outer*, allowing a sub-point tolerance."""
    return (
        inner[0] >= outer[0] - _BOX_TOLERANCE
        and inner[1] >= outer[1] - _BOX_TOLERANCE
        and inner[2] <= outer[2] + _BOX_TOLERANCE
        and inner[3] <= outer[3] + _BOX_TOLERANCE
    )


def own_box(page: pikepdf.Object, name: str) -> Rect | None:
    """A box defined directly on the page (Trim/Art/Bleed are not inheritable)."""
    return _rect(page.get(f"/{name}"))


def media_box(page: pikepdf.Object) -> Rect | None:
    """The MediaBox, resolving the one inheritable box up the page tree."""
    # Guard the walk by object identity (obj, gen), not id() of the wrapper --
    # pikepdf returns a fresh Python wrapper each `.get("/Parent")`, so id() never
    # repeats and could never catch a cyclic /Parent chain (an infinite loop).
    node: pikepdf.Object | None = page
    seen: set[tuple[int, int]] = set()
    while node is not None and node.objgen not in seen:
        seen.add(node.objgen)
        box = _rect(node.get("/MediaBox"))
        if box is not None:
            return box
        node = node.get("/Parent")
    return None


def _size_mm(rect: Rect) -> tuple[float, float]:
    return (pt_to_mm(rect[2] - rect[0]), pt_to_mm(rect[3] - rect[1]))


def check_page_boxes(pdf: pikepdf.Pdf) -> list[Finding]:
    """Verify each page carries the finished-page geometry PDF/X-4 demands."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        obj = page.obj
        media = media_box(obj)
        trim = own_box(obj, "TrimBox")
        art = own_box(obj, "ArtBox")
        bleed = own_box(obj, "BleedBox")

        if media is None:
            findings.append(Finding("boxes", Severity.ERROR, "no MediaBox", i))
            continue

        finished = trim or art
        if trim is None and art is None:
            findings.append(
                Finding(
                    "trimbox",
                    Severity.ERROR,
                    "no TrimBox or ArtBox -- PDF/X-4 needs one to mark the finished page",
                    i,
                )
            )
        elif trim is not None and art is not None:
            findings.append(
                Finding("trimbox", Severity.ERROR, "both TrimBox and ArtBox present -- PDF/X-4 allows only one", i)
            )

        if finished is not None and not _contains(media, finished):
            findings.append(Finding("trimbox", Severity.ERROR, "the trim/art box extends beyond the MediaBox", i))
        if bleed is not None:
            if not _contains(media, bleed):
                findings.append(Finding("bleedbox", Severity.ERROR, "the BleedBox extends beyond the MediaBox", i))
            if finished is not None and not _contains(bleed, finished):
                findings.append(
                    Finding("bleedbox", Severity.ERROR, "the BleedBox does not contain the trim/art box", i)
                )
        elif finished is not None:
            findings.append(
                Finding(
                    "bleedbox",
                    Severity.WARN,
                    "no BleedBox -- add bleed if the artwork runs to the trim edge",
                    i,
                )
            )
    return findings


def check_trapped(pdf: pikepdf.Pdf) -> list[Finding]:
    """PDF/X requires ``/Trapped`` in the Info dict to be True or False."""
    info = pdf.docinfo
    trapped = info.get("/Trapped") if info is not None else None
    if trapped is None:
        return [Finding("trapped", Severity.ERROR, "/Trapped missing from Info -- PDF/X requires True or False")]
    value = str(trapped)
    if value not in ("/True", "/False"):
        return [Finding("trapped", Severity.ERROR, f"/Trapped is {value.lstrip('/')} -- PDF/X requires True or False")]
    return []


def check_pdfx_identity(pdf: pikepdf.Pdf) -> list[Finding]:
    """Verify the PDF/X identity: the XMP version key and a GTS_PDFX output intent."""
    findings: list[Finding] = []
    if b"GTS_PDFXVersion" not in _xmp_bytes(pdf):
        findings.append(
            Finding("pdfx-xmp", Severity.ERROR, "XMP lacks pdfxid:GTS_PDFXVersion -- the PDF/X version identifier")
        )
    if not _has_pdfx_output_intent(pdf):
        findings.append(
            Finding(
                "pdfx-intent",
                Severity.ERROR,
                "no OutputIntent with subtype GTS_PDFX -- PDF/X-4 needs a print output intent",
            )
        )
    return findings


def _xmp_bytes(pdf: pikepdf.Pdf) -> bytes:
    metadata = pdf.Root.get("/Metadata")
    if metadata is None:
        return b""
    return bytes(metadata.read_bytes())


def _has_pdfx_output_intent(pdf: pikepdf.Pdf) -> bool:
    intents = pdf.Root.get("/OutputIntents")
    if intents is None:
        return False
    return any(str(intent.get("/S")) == "/GTS_PDFX" for intent in intents)


def check_geometry(pdf: pikepdf.Pdf, geometry: PageGeometry) -> list[Finding]:
    """Hold each page's declared trim and bleed to the size it was designed for.

    Three separate things go wrong independently, so each is its own finding:
    the finished size, the amount of bleed, and whether the finished page sits
    centred on the sheet. The size comparison is orientation-agnostic -- a
    landscape page declares the same two numbers the other way round -- so
    nothing here needs to be told which way up the artwork is.
    """
    findings: list[Finding] = []
    expected = sorted((geometry.trim_w_mm, geometry.trim_h_mm))
    tol = geometry.tolerance_mm
    for i, page in enumerate(pdf.pages, start=1):
        obj = page.obj
        media = media_box(obj)
        trim = own_box(obj, "TrimBox") or own_box(obj, "ArtBox")
        if trim is None:
            continue  # already reported by check_page_boxes; nothing to measure against
        w_mm, h_mm = _size_mm(trim)
        if any(abs(a - b) > tol for a, b in zip(sorted((w_mm, h_mm)), expected, strict=True)):
            findings.append(
                Finding(
                    "geometry",
                    Severity.ERROR,
                    f"finished size is {w_mm:.2f} x {h_mm:.2f} mm, expected "
                    f"{geometry.trim_w_mm:g} x {geometry.trim_h_mm:g} mm",
                    i,
                )
            )
        findings += _check_bleed(obj, trim, geometry, i)
        if media is not None:
            findings += _check_centring(media, trim, tol, i)
    return findings


def _check_bleed(obj: pikepdf.Object, trim: Rect, geometry: PageGeometry, page: int) -> list[Finding]:
    """The BleedBox must stand off the trim by exactly the designed bleed."""
    bleed = own_box(obj, "BleedBox")
    if bleed is None:
        return []  # absence is check_page_boxes' warning to give, not ours
    want = mm_to_pt(geometry.bleed_mm)
    tol = mm_to_pt(geometry.tolerance_mm)
    offsets = (trim[0] - bleed[0], trim[1] - bleed[1], bleed[2] - trim[2], bleed[3] - trim[3])
    if all(abs(off - want) <= tol for off in offsets):
        return []
    got = ", ".join(f"{pt_to_mm(off):.2f}" for off in offsets)
    return [
        Finding(
            "bleed",
            Severity.ERROR,
            f"bleed is {got} mm (left, bottom, right, top), expected {geometry.bleed_mm:g} mm on every side",
            page,
        )
    ]


def check_artwork_bleed(pdf: pikepdf.Pdf) -> list[Finding]:
    """Artwork that reaches the trim edge must run on to the bleed edge, or cutting leaves a sliver of paper.

    Judged per side, per page, on what is visible after clipping: filled paths, images and shadings (white fills
    included; text and strokes are not backgrounds). An object inset from the trim is ignored. The bleed edge is the
    BleedBox, else the MediaBox. Pages with no TrimBox or no bleed zone are skipped (other checks report those). A
    rotated page is reported as not checked rather than guessed at. Limits: this is geometry, not the printed result,
    so transparency and soft masks are taken as opaque; a non-rectangular clip counts as its bounding extent; a
    Form XObject's own /BBox clip is not applied; and an object is judged alone, so a trim-sized panel over a
    full-bleed background is still flagged.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        trim = own_box(page.obj, "TrimBox")
        bleed = own_box(page.obj, "BleedBox") or media_box(page.obj)
        if trim is None or bleed is None:
            continue
        # Per side (left, bottom, right, top): the bleed zone's width, or 0 when there is none to fill.
        zones = [
            max(trim[0] - bleed[0], 0),
            max(trim[1] - bleed[1], 0),
            max(bleed[2] - trim[2], 0),
            max(bleed[3] - trim[3], 0),
        ]
        if all(z <= _BOX_TOLERANCE for z in zones):
            continue
        if int(page.obj.get("/Rotate", 0)) % 360:
            findings.append(
                Finding(
                    "artwork-bleed", Severity.WARN, "bleed coverage not checked: the page is rotated with /Rotate", i
                )
            )
            continue
        short = _short_sides(page, trim, zones)
        if any(short):
            parts = [
                f"{_SIDES[s]}: {len(objs)} object(s) reach the trim but extend only "
                f"{pt_to_mm(min(objs.values())):.1f} mm of the {pt_to_mm(zones[s]):.1f} mm bleed"
                for s, objs in enumerate(short)
                if objs
            ]
            total = len({n for objs in short for n in objs})
            findings.append(
                Finding(
                    "artwork-bleed",
                    Severity.ERROR,
                    f"{total} object(s) stop short of the bleed edge ({'; '.join(parts)}) -- "
                    "artwork must run to the bleed edge",
                    i,
                )
            )
    return findings


_SIDES = ("left", "bottom", "right", "top")


def _short_sides(page: pikepdf.Page, trim: Rect, zones: list[float]) -> list[dict[int, float]]:
    """Per side, the objects that reach the trim but stop inside the bleed zone: object -> points into the zone."""
    fills = [
        (n, p.visible)
        for n, p in enumerate(paint_events(page))
        if p.visible is not None and (p.kind in ("image", "shading") or p.filled)
    ]
    return [_short_on_side(fills, trim, side, zone) for side, zone in enumerate(zones)]


def _short_on_side(fills: list[tuple[int, Rect]], trim: Rect, side: int, zone: float) -> dict[int, float]:
    """One side (0 left, 1 bottom, 2 right, 3 top) of :func:`_short_sides`.

    An object that stops short is excused where other artwork covers the bleed strip beside it: a white panel at the
    trim over a full-bleed background leaves no sliver. Covering objects run from the bleed edge at least to the trim.
    """
    if zone <= _BOX_TOLERANCE:
        return {}
    lo = 1 if side in (0, 2) else 0  # the axis along this edge
    out = lambda v: (trim[0] - v[0], trim[1] - v[1], v[2] - trim[2], v[3] - trim[3])[side]  # noqa: E731
    inward = lambda v: (v[2] - trim[0], v[3] - trim[1], trim[2] - v[0], trim[3] - v[1])[side]  # noqa: E731
    covers = sorted(
        (v[lo], v[lo + 2]) for _, v in fills if out(v) >= zone - _BOX_TOLERANCE and inward(v) >= -_BOX_TOLERANCE
    )
    short: dict[int, float] = {}
    for n, v in fills:
        touches = all(v[i + 2] > trim[i] + _BOX_TOLERANCE and v[i] < trim[i + 2] - _BOX_TOLERANCE for i in (0, 1))
        if not touches or not -_BOX_TOLERANCE <= out(v) < zone - _BOX_TOLERANCE:
            continue
        if not _spanned((max(v[lo], trim[lo]), min(v[lo + 2], trim[lo + 2])), covers):
            short[n] = max(out(v), 0.0)
    return short


def _spanned(span: tuple[float, float], covers: list[tuple[float, float]]) -> bool:
    """Whether the sorted *covers* intervals leave no gap in *span*."""
    reached = span[0]
    for start, end in covers:
        if start > reached + _BOX_TOLERANCE:
            break
        reached = max(reached, end)
    return reached >= span[1] - _BOX_TOLERANCE


def _check_centring(media: Rect, trim: Rect, tolerance_mm: float, page: int) -> list[Finding]:
    """The finished page must sit centred on the sheet.

    An off-centre trim means the crop-mark margin differs side to side, so the
    artwork is not where the imposition expects it even when its size is right.
    """
    left, bottom = trim[0] - media[0], trim[1] - media[1]
    right, top = media[2] - trim[2], media[3] - trim[3]
    tol = mm_to_pt(tolerance_mm)
    if abs(left - right) <= tol and abs(bottom - top) <= tol:
        return []
    margins = ", ".join(f"{pt_to_mm(m):.2f}" for m in (left, bottom, right, top))
    return [
        Finding(
            "centring",
            Severity.ERROR,
            f"the trim box is not centred on the MediaBox: margins are {margins} mm (left, bottom, right, top)",
            page,
        )
    ]


def check_output_condition(pdf: pikepdf.Pdf, expected: str) -> list[Finding]:
    """Check an output intent names the printing condition the job expects (case-insensitive substring).

    Matched against each intent's identifier, condition and info strings and its ICC profile's description, so
    ``FOGRA51`` matches an identifier that says so, or a profile called "PSO Coated v3 (FOGRA51)".
    """
    intents = list(pdf.Root.get("/OutputIntents", []))
    if not intents:
        return [
            Finding("output-condition", Severity.ERROR, f"no output intent to compare with the expected {expected!r}")
        ]
    named: list[str] = []
    for intent in intents:
        texts = [str(intent.get(key, "")) for key in ("/OutputConditionIdentifier", "/OutputCondition", "/Info")]
        texts.append(_profile_description(intent))
        if any(expected.lower() in text.lower() for text in texts if text):
            return []
        named.append(next((t for t in texts if t), "unnamed"))
    return [
        Finding(
            "output-condition",
            Severity.ERROR,
            f"the output intent is {', '.join(repr(n) for n in named)}, expected {expected!r}",
        )
    ]


def _profile_description(intent: pikepdf.Object) -> str:
    profile = intent.get("/DestOutputProfile")
    if profile is None:
        return ""
    try:
        return ImageCms.getProfileDescription(ImageCms.ImageCmsProfile(io.BytesIO(profile.read_bytes()))).strip()
    except Exception:  # an unreadable profile just has no description; the other fields still match
        return ""


def check_pages(pdf: pikepdf.Pdf, *, uniform: bool, multiple_of: int | None) -> list[Finding]:
    """Page-set rules: every page the same finished size, rotation absent, and a page count a booklet can fold to."""
    findings: list[Finding] = []
    count = len(pdf.pages)
    if multiple_of and count % multiple_of:
        findings.append(Finding("page-count", Severity.ERROR, f"{count} pages is not a multiple of {multiple_of}"))
    if not uniform:
        return findings
    reference: tuple[float, float] | None = None
    for i, page in enumerate(pdf.pages, start=1):
        rect = own_box(page.obj, "TrimBox") or media_box(page.obj)
        if rect is None:
            continue
        size = _size_mm(rect)
        reference = reference or size
        if abs(size[0] - reference[0]) > _UNIFORM_TOLERANCE_MM or abs(size[1] - reference[1]) > _UNIFORM_TOLERANCE_MM:
            findings.append(
                Finding(
                    "page-size",
                    Severity.ERROR,
                    f"the page is {size[0]:g} x {size[1]:g} mm but the first page is "
                    f"{reference[0]:g} x {reference[1]:g} mm",
                    i,
                )
            )
        if int(page.obj.get("/Rotate", 0)) % 360:
            findings.append(Finding("page-rotate", Severity.WARN, "the page is rotated with /Rotate", i))
    return findings
