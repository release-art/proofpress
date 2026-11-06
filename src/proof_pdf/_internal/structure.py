"""The job's page geometry and page set, which no standard states.

The PDF/X rules (page boxes, ``/Trapped``, the output intent) are in :mod:`.pdfx`.

:func:`check_geometry` goes one step further than the standards do: it holds the
declared TrimBox and BleedBox to the size the artwork was *designed* for. PDF/X
only requires that the boxes exist and nest correctly, so a file can be a
perfectly valid PDF/X-4 and still be trimmed to the wrong size.
"""

import io

import pikepdf
from PIL import ImageCms

from ..models import Finding, PageGeometry, Severity, mm_to_pt, pt_to_mm
from .paint import Paint, paint_events
from .spaces import output_intents

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
            findings += _check_centring(media, trim, geometry, i)
    return findings


def _check_bleed(obj: pikepdf.Object, trim: Rect, geometry: PageGeometry, page: int) -> list[Finding]:
    """The BleedBox must stand off the trim by exactly the designed bleed, per side (the binding edge may have none)."""
    bleed = own_box(obj, "BleedBox")
    if bleed is None:
        return []  # absence is check_page_boxes' warning to give, not ours
    wants = geometry.bleeds_mm(page)
    tol = mm_to_pt(geometry.tolerance_mm)
    offsets = (trim[0] - bleed[0], trim[1] - bleed[1], bleed[2] - trim[2], bleed[3] - trim[3])
    if all(abs(off - mm_to_pt(want)) <= tol for off, want in zip(offsets, wants, strict=True)):
        return []
    got = ", ".join(f"{pt_to_mm(off):.2f}" for off in offsets)
    expected = (
        f"{geometry.bleed_mm:g} mm on every side"
        if len(set(wants)) == 1
        else f"{', '.join(f'{w:g}' for w in wants)} mm"
    )
    return [
        Finding("bleed", Severity.ERROR, f"bleed is {got} mm (left, bottom, right, top), expected {expected}", page)
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


def check_outside_bleed(pdf: pikepdf.Pdf, geometry: PageGeometry | None = None) -> list[Finding]:
    """Anything painted beyond the bleed region is printer's marks or a slug, which a bare-page printer rejects.

    The bleed region is the BleedBox; without one, the TrimBox grown by *bleed_mm* when the geometry layer gave one,
    else the TrimBox itself (a note says which, once). Judged on the visible extent after clipping, so an object
    clipped away does not count, and an object in the bleed zone is correct. Pages with no TrimBox are skipped; a
    rotated page is reported as not checked. Limits: a text run's extent is its font box (ascent to descent), a
    stroke's is its centre line, and a Form XObject's own /BBox clip is not applied.
    """
    findings: list[Finding] = []
    notes: dict[str, None] = {}
    for i, page in enumerate(pdf.pages, start=1):
        trim = own_box(page.obj, "TrimBox")
        if trim is None:
            continue
        if int(page.obj.get("/Rotate", 0)) % 360:
            findings.append(
                Finding("outside-bleed", Severity.WARN, "content outside the bleed not checked: the page is rotated", i)
            )
            continue
        region = own_box(page.obj, "BleedBox")
        if region is None:
            grow = [mm_to_pt(b) for b in geometry.bleeds_mm(i)] if geometry else [0.0] * 4
            region = (trim[0] - grow[0], trim[1] - grow[1], trim[2] + grow[2], trim[3] + grow[3])
            basis = (
                "the TrimBox grown by the expected bleed" if geometry else "the TrimBox (no BleedBox, no bleed given)"
            )
            notes[f"bleed region taken from {basis}: the page has no BleedBox"] = None
        if outside := [(p, over) for p in paint_events(page) if (over := _overshoot(p, region)) > _BOX_TOLERANCE]:
            worst = pt_to_mm(max(over for _, over in outside))
            findings.append(
                Finding(
                    "outside-bleed",
                    Severity.ERROR,
                    f"{len(outside)} object(s) lie outside the bleed region, by up to {worst:.1f} mm"
                    f"{_marks_hint([p for p, _ in outside])} -- remove printer's marks and slug text",
                    i,
                )
            )
    return findings + [Finding("outside-bleed", Severity.WARN, note) for note in notes]


def check_outside_media(pdf: pikepdf.Pdf) -> list[Finding]:
    """Warn of objects painted entirely beyond the MediaBox: hidden content that never prints but ships in the file."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        media = media_box(page.obj)
        if media is None:
            continue
        gone = [
            p
            for p in paint_events(page)
            if p.bbox is not None
            and not (p.kind == "text" and p.render_mode in (3, 7))
            and (p.bbox[2] < media[0] or p.bbox[0] > media[2] or p.bbox[3] < media[1] or p.bbox[1] > media[3])
        ]
        if gone:
            findings.append(
                Finding(
                    "outside-media",
                    Severity.WARN,
                    f"{len(gone)} object(s) are painted entirely outside the MediaBox: hidden content",
                    i,
                )
            )
    return findings


# ponytail: a path this thin (pt) and outside the bleed is called a likely mark; only a hint, add a real shape test if
# the hint misleads.
_MARK_THIN_PT = 1.0


def _overshoot(paint: Paint, region: Rect) -> float:
    """How far (points) the visible part of *paint* reaches beyond *region*; 0 for an unpainted or unseen one."""
    v = paint.visible
    if v is None or (paint.kind == "text" and paint.render_mode in (3, 7)):  # invisible or clip-only text
        return 0.0
    return max(region[0] - v[0], region[1] - v[1], v[2] - region[2], v[3] - region[3], 0.0)


def _marks_hint(objects: list[Paint]) -> str:
    thin = any(
        p.kind == "path" and p.bbox is not None and min(p.bbox[2] - p.bbox[0], p.bbox[3] - p.bbox[1]) <= _MARK_THIN_PT
        for p in objects
    )
    text = any(p.kind == "text" for p in objects)
    hints = (["thin shapes: likely printer's marks"] if thin else []) + (["text: likely a slug"] if text else [])
    return f" ({'; '.join(hints)})" if hints else ""


def _short_sides(page: pikepdf.Page, trim: Rect, zones: list[float]) -> list[dict[int, float]]:
    """Per side, the objects that reach the trim but stop inside the bleed zone: object -> points into the zone."""
    fills = [
        (n, p.visible)
        for n, p in enumerate(paint_events(page))
        if p.visible is not None and (p.kind in ("image", "inline-image", "shading") or p.filled)
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


def _check_centring(media: Rect, trim: Rect, geometry: PageGeometry, page: int) -> list[Finding]:
    """The finished page must sit centred on the sheet, give or take the asymmetry of the expected bleed.

    An off-centre trim means the crop-mark margin differs side to side, so the artwork is not where the imposition
    expects it even when its size is right. With bleed on every side the margins must match left to right and bottom to
    top; with none on the binding edge the left-right margins must differ by exactly the difference in bleed.
    """
    left, bottom = trim[0] - media[0], trim[1] - media[1]
    right, top = media[2] - trim[2], media[3] - trim[3]
    want_left, _, want_right, _ = (mm_to_pt(w) for w in geometry.bleeds_mm(page))
    tol = mm_to_pt(geometry.tolerance_mm)
    if abs((left - right) - (want_left - want_right)) <= tol and abs(bottom - top) <= tol:
        return []
    margins = ", ".join(f"{pt_to_mm(m):.2f}" for m in (left, bottom, right, top))
    skew = pt_to_mm(want_left - want_right)
    expected = f"; expected left minus right {skew:+.2f} mm" if abs(skew) > 1e-9 else ""
    return [
        Finding(
            "centring",
            Severity.ERROR,
            f"the trim box is not centred on the MediaBox: margins are {margins} mm (left, bottom, right, top)"
            f"{expected}",
            page,
        )
    ]


def check_output_condition(pdf: pikepdf.Pdf, expected: str) -> list[Finding]:
    """Check an output intent names the printing condition the job expects (case-insensitive substring).

    Matched against each intent's identifier, condition and info strings and its ICC profile's description, so
    ``FOGRA51`` matches an identifier that says so, or a profile called "PSO Coated v3 (FOGRA51)".
    """
    intents = output_intents(pdf)
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
