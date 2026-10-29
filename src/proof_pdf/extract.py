"""Recover what a PDF is made of: images, embedded fonts, ICC profiles and vector artwork, plus where it all sat.

For a customer's archive, where the source files are long gone and the PDF is all that is left. ``extract_pdf`` writes
the assets into a directory and a ``document.json`` beside them describing the document: page boxes, metadata, output
intents, every asset with its placements on the page, every text run with its font, size, colour and position, and the
``inspect`` inventory, the decoded XMP, the colour palette and the page layout (:mod:`proof_pdf.layout`). That is
enough to rebuild a template around the recovered pieces.

Images come out as they are stored where the format allows (JPEG and JPEG 2000 are not re-encoded), with a soft mask
as a separate ``.mask.png``. Fonts are the embedded programs, which for a subset font hold only the glyphs that were
used. Vector artwork is grouped by touching shapes, one PDF per group, cropped to it (see ``_internal/vector``).
Coordinates are PDF points, origin bottom-left, in the page's unrotated space; ``document.json`` gives each page's
height and rotation so a caller can flip them. Text is read through ``/ToUnicode`` where present, which is exact; a
font without one falls back to Latin-1 (simple fonts) or no text (composite fonts), and word gaps are a heuristic.
"""

import asyncio
import collections
import collections.abc
import dataclasses
import hashlib
import io
import json
import logging
import pathlib
import re
import shutil
import typing

import pikepdf
from PIL import ImageCms

from ._internal import (
    contrast,
    interactive,
    spots,
    structure,
    tables as table_finder,
    tagged as tagged_reader,
    xmp,
)
from ._internal.paint import Fill, Paint, Walker
from ._internal.previews import build as build_previews
from ._internal.spaces import CMYK, GRAY, RGB, classify_colour_space
from ._internal.vector import INVISIBLE, Group, artwork
from .inventory import inspect_pdf
from .layout import Figure, Layout, Line, Table, analyse

logger = logging.getLogger(__name__)

KINDS = ("images", "fonts", "icc", "vector")

# How close (pt) two shapes must be to count as one piece of artwork, and how much of the page a shape may cover before
# it is taken for a background. Both are heuristics: tune them per document with the CLI options.
DEFAULT_GAP_PT = 1.0
DEFAULT_MAX_FRACTION = 0.25
DEFAULT_PREVIEW_DPI = 100

# Heuristics for tidying what the content stream gives: a box thinner than HAIRLINE_PT is a rule rather than a panel,
# and rule pieces closer than JOIN_PT along their length are one rule (a dashed or segmented line is drawn in bits).
HAIRLINE_PT = 1.0
MAX_XMP_VALUE = 1000  # characters; an embedded thumbnail is longer, and is in xmp.xml
JOIN_PT = 0.5
# Text runs on one baseline are one line when the gap is under LINE_GAP_EM of the font size, with a space inserted
# when it is over SPACE_EM (a space glyph is about a quarter em; tracking and kerning are far less).
LINE_GAP_EM = 0.5
SPACE_EM = 0.15

_FONT_FILES = {"/FontFile": ".t1", "/FontFile2": ".ttf", "/FontFile3": ".cff"}
_OPENTYPE = "/OpenType"
_SUBSET_PREFIX = re.compile(r"^[A-Z]{6}\+")
_ERRORS = (pikepdf.PdfError, NotImplementedError, ValueError, OSError, TypeError, KeyError, IndexError)


@dataclasses.dataclass(frozen=True)
class Placement:
    """Where an asset sits on a page, in points from the page's bottom-left corner."""

    page: int
    x: float
    y: float
    width: float
    height: float
    matrix: list[float] | None = None  # an image's placement matrix (a b c d e f), which carries rotation and flips


@dataclasses.dataclass(frozen=True)
class Asset:
    kind: str  # "image", "font", "icc" or "vector"
    file: str  # relative to the output directory
    placements: list[Placement]
    details: dict[str, typing.Any]


@dataclasses.dataclass(frozen=True)
class TextRun:
    page: int
    text: str | None  # None when the font gives no way to read it
    codes: list[int] | None  # then the font's character codes, to decode against the recovered font program
    x: float
    y: float
    width: float
    height: float
    font: str
    size_pt: float
    colour: str
    rotation_deg: float = 0.0
    h_scale: float = 1.0
    letter_spacing_em: float = 0.0
    invisible: bool = False  # render mode 3: the hidden text layer of a scan
    mcid: int | None = None  # marked-content id, which the structure tree refers to
    tag: str | None = None  # the structure tag of a tagged PDF, standard role (H2, P, TD...)
    struct: int | None = None  # the paragraph-like structure element holding it (``structure.elements[].id``)
    artifact: str | None = None  # page furniture kind when the PDF marks it (Header, Footer...)


@dataclasses.dataclass(frozen=True)
class Box:
    """A bare rectangle or straight line: a panel, background or rule of the layout. Colours are content-stream form."""

    page: int
    kind: str  # "rectangle" or "line"
    x: float
    y: float
    width: float
    height: float
    fill: str | None  # e.g. "k 0 0 0 0.65"
    stroke: str | None
    stroke_width_pt: float | None
    points: list[list[float]] | None = None  # a line's two ends, [[x1, y1], [x2, y2]]
    fill_colour: str | None = None  # the fill as the inventory writes colours, e.g. "C0 M0 Y0 K65"
    stroke_colour: str | None = None


@dataclasses.dataclass
class Extraction:
    path: str
    out_dir: str
    pages: list[dict[str, typing.Any]]
    metadata: dict[str, typing.Any]
    output_intents: list[dict[str, typing.Any]]
    assets: list[Asset]
    text: list[TextRun]
    lines: list[Line]
    boxes: list[Box]
    layout: Layout
    palette: list[dict[str, typing.Any]]  # colours in use, with the panel area and text they colour
    spot_colours: list[dict[str, typing.Any]]  # named inks: alternate colour at full tint and the pages that use them
    structure: dict[
        str, typing.Any
    ]  # the tagged-PDF structure tree: each element's role, alt text, attributes, text, box
    interactive: dict[str, typing.Any]  # form fields, links, annotations, outline, page labels, layers, attachments
    inventory: dict[str, typing.Any]
    notes: list[str]  # what the extraction could not do for this document, in words
    failures: list[str]
    previews: str | None = None  # the review page, relative to the output directory, when asked for

    def to_dict(self) -> dict:
        return dataclasses.asdict(self)


def extract_pdf(
    path: pathlib.Path,
    out_dir: pathlib.Path,
    *,
    kinds: collections.abc.Collection[str] = KINDS,
    gap_pt: float = DEFAULT_GAP_PT,
    max_fraction: float = DEFAULT_MAX_FRACTION,
    vector_text: bool = True,
    previews: bool = False,
    preview_dpi: int = DEFAULT_PREVIEW_DPI,
    gs_bin: str = "gs",
) -> Extraction:
    """Write *path*'s assets of the given *kinds* into *out_dir* with ``document.json``, and return what was found.

    The boxes and tables are read from the page's rules, which the ``vector`` kind collects: without it there are none.

    With *vector_text*, text lying on a piece of vector artwork is drawn into the piece (and listed in its ``text``)
    as well as being recovered as text; without it the pieces are the drawn shapes alone.

    With *previews*, also render ``previews/index.html``, a review page of the pages with every asset boxed on them,
    which needs Ghostscript and raises :class:`proof_pdf.preview.GhostscriptError` without it. It runs its own event
    loop, so call it from a thread when already inside one.

    Raises :class:`pikepdf.PdfError` for a file that will not open. An asset that cannot be recovered is listed in
    :attr:`Extraction.failures` and the rest carry on.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    failures: list[str] = []
    with pikepdf.open(path) as pdf:
        tagged = tagged_reader.read(pdf)
        recovered = _Recovery(pdf, out_dir, failures, vector_text, tagged)
        for number, page in enumerate(pdf.pages, start=1):
            recovered.page(number, page, "images" in kinds, "vector" in kinds, gap_pt, max_fraction)
        if "images" in kinds:
            recovered.unplaced_images()
        if "fonts" in kinds:
            recovered.fonts()
        intents = recovered.profiles() if "icc" in kinds else []
        lines = _lines(recovered.text)
        spot_colours = _spot_colours(pdf, recovered.pages)
        boxes = _merge_boxes(recovered.boxes)
        tables = _tables(lines, boxes, recovered.pages)
        layout = analyse(lines, _figures(recovered.assets), recovered.pages, tables)
        result = Extraction(
            path=str(path),
            out_dir=str(out_dir),
            pages=recovered.pages,
            metadata=_metadata(pdf, out_dir, failures),
            output_intents=intents,
            assets=recovered.assets,
            text=recovered.text,
            lines=lines,
            boxes=boxes,
            layout=layout,
            palette=_palette(boxes, layout, spot_colours, recovered.artwork_colours),
            spot_colours=spot_colours,
            structure=_structure(tagged, recovered.text, recovered.marked_boxes),
            interactive=interactive.collect(pdf, out_dir, failures),
            inventory=inspect_pdf(path, with_contrast=False).to_dict(),
            notes=_notes(recovered.text),
            failures=failures,
        )
    if previews:
        shutil.rmtree(out_dir / "previews", ignore_errors=True)  # ours, and stale pages would be listed as current
        result.previews, problems = asyncio.run(
            build_previews(result.to_dict(), path, out_dir, dpi=preview_dpi, gs_bin=gs_bin)
        )
        failures += problems
    (out_dir / "document.json").write_text(json.dumps(result.to_dict(), indent=2, ensure_ascii=False), encoding="utf-8")
    return result


class _Recovery:
    def __init__(
        self,
        pdf: pikepdf.Pdf,
        out_dir: pathlib.Path,
        failures: list[str],
        vector_text: bool = True,
        tagged: tagged_reader.Tagged | None = None,
    ) -> None:
        self.pdf, self.out, self.failures = pdf, out_dir, failures
        self.vector_text = vector_text
        self.tagged = tagged
        self.marked_boxes: list[tuple[int, int, tuple[float, float, float, float]]] = []  # (page, MCID, box) of figures
        self.assets: list[Asset] = []
        self.text: list[TextRun] = []
        self.boxes: list[Box] = []
        self.artwork_colours: dict[str, float] = {}  # colour -> area of the vector artwork painted in it
        self.pages: list[dict[str, typing.Any]] = []
        self._by_key: dict[typing.Any, Asset] = {}  # the same image or artwork placed again adds a placement
        self._masks: set[tuple[int, int]] = set()  # soft masks, saved beside the image they belong to
        self._font_pages: dict[str, set[int]] = {}

    def page(self, number: int, page: pikepdf.Page, images: bool, vector: bool, gap_pt: float, fraction: float) -> None:
        media = structure.media_box(page.obj)
        self.pages.append(
            {
                "number": number,
                "rotate": int(page.obj.get("/Rotate", 0)),
                "width_pt": media[2] - media[0] if media else None,
                "height_pt": media[3] - media[1] if media else None,
                "media_pt": media,
                **{
                    f"{key}_pt": structure.own_box(page.obj, f"{key.capitalize()}Box")
                    for key in ("crop", "bleed", "trim", "art")
                },
            }
        )
        walker = Walker()
        texts: list[Paint] = []
        for event in walker.walk(page):
            if event.bbox is None:
                continue
            x0, y0, x1, y1 = event.bbox
            if event.kind == "text" and event.glyphs and event.font is not None and (event.text or "x").strip():
                self.text.append(self._text_run(number, event))
                self._font_pages.setdefault(event.font.name, set()).add(number)
                texts.append(event)
            elif event.kind == "image" and images and event.xobject is not None:
                placement = Placement(number, x0, y0, x1 - x0, y1 - y0, list(event.matrix) if event.matrix else None)
                self._image(number, typing.cast(pikepdf.Stream, event.xobject), placement, event)
        self.pages[-1]["spot_inks"] = sorted(walker.spots)
        if vector:
            self._vector(number, page, gap_pt, fraction, texts if self.vector_text else [])

    def _text_run(self, number: int, event: Paint) -> TextRun:
        assert event.bbox is not None and event.font is not None
        x0, y0, x1, y1 = event.bbox
        element = self.tagged.at(number, event.mcid) if self.tagged else None
        return TextRun(
            number,
            event.text,
            list(event.codes) if event.text is None else None,
            x0,
            y0,
            x1 - x0,
            y1 - y0,
            event.font.name,
            event.size,
            contrast.describe(event.fill),
            round(event.rotation, 2),
            round(event.h_scale, 4),
            round(event.char_spacing, 4),
            event.render_mode == INVISIBLE,
            event.mcid,
            self.tagged.elements[element.block].role if element and self.tagged else None,
            element.block if element else None,
            event.artifact,
        )

    def _annotate(
        self, asset: Asset | None, page: int, mcids: typing.Iterable[int | None], artifact: str | None
    ) -> None:
        """Say what the structure tree (and any artifact marking) says about an image or piece of artwork."""
        if asset is None:
            return
        if artifact:
            asset.details["artifact"] = artifact
        element = self.tagged.with_alt(page, mcids) if self.tagged else None
        if element is not None:
            asset.details.setdefault("alt", element.alt)
            asset.details.setdefault("tag", element.role)

    def _add(
        self, key: typing.Any, make: collections.abc.Callable[[], Asset | None], placement: Placement | None = None
    ) -> Asset | None:
        asset = self._by_key.get(key)
        if asset is None:
            asset = make()
            if asset is None:
                return None
            self._by_key[key] = asset
            self.assets.append(asset)
        if placement is not None:
            asset.placements.append(placement)
        return asset

    def _image(self, number: int, obj: pikepdf.Stream, placement: Placement, event: Paint) -> None:
        objnum = obj.objgen[0]
        key = ("image", obj.objgen if objnum else id(obj))
        asset = self._add(key, lambda: self._write_image(number, obj, objnum or len(self.assets)), placement)
        self._annotate(asset, number, [event.mcid], event.artifact)
        if event.mcid is not None and event.bbox is not None:
            self.marked_boxes.append((number, event.mcid, event.bbox))

    def unplaced_images(self) -> None:
        """Images stored in the file that no page paints (left over in a form or resource dictionary)."""
        for obj in _objects(self.pdf):
            if (
                isinstance(obj, pikepdf.Stream)
                and str(obj.get("/Subtype")) == "/Image"
                and obj.objgen not in self._masks | {(0, 0)}
            ):
                self._add(("image", obj.objgen), lambda obj=obj: self._write_image(0, obj, obj.objgen[0]))

    def _write_image(self, number: int, obj: pikepdf.Stream, serial: int) -> Asset | None:
        stem = self.out / "images" / f"{f'p{number:03d}' if number else 'unplaced'}-im{serial}"
        try:
            stem.parent.mkdir(exist_ok=True)
            image = pikepdf.PdfImage(obj)
            written = pathlib.Path(image.extract_to(fileprefix=str(stem)))
            mask = obj.get("/SMask")
            if mask is not None:
                self._masks.add(mask.objgen)
                pikepdf.PdfImage(typing.cast(pikepdf.Stream, mask)).as_pil_image().save(
                    stem.with_name(stem.name + ".mask.png")
                )
        except _ERRORS as exc:
            self.failures.append(f"page {number}: image {serial}: {exc}")
            return None
        return Asset(
            "image",
            written.relative_to(self.out).as_posix(),
            [],
            {
                "pixels": [int(obj.get("/Width", 0)), int(obj.get("/Height", 0))],
                "colour_space": classify_colour_space(obj.get("/ColorSpace"), None),
                "bits": int(obj.get("/BitsPerComponent", 0)),
                "filters": _filters(obj),
                "stencil": bool(obj.get("/ImageMask", False)),
                "soft_mask": mask is not None,
            },
        )

    def _vector(self, number: int, page: pikepdf.Page, gap_pt: float, fraction: float, texts: list[Paint]) -> None:
        try:
            found, boxes = artwork(page, gap_pt=gap_pt, max_fraction=fraction, texts=texts)
        except _ERRORS as exc:
            self.failures.append(f"page {number}: vector artwork: {exc}")
            return
        for box in boxes:
            x0, y0, x1, y1 = box.bbox
            self.boxes.append(
                Box(
                    number,
                    "line" if box.path[0][0] == "m" else "rectangle",
                    x0,
                    y0,
                    x1 - x0,
                    y1 - y0,
                    box.fill,
                    box.stroke,
                    box.stroke_width,
                    [list(end) for end in box.endpoints] if box.endpoints else None,
                    _describe(box.fill),
                    _describe(box.stroke),
                )
            )
        for index, group in enumerate(found, start=1):
            self._note_colours(group)
            x0, y0, x1, y1 = group.bbox
            try:
                digest, data = group.render()
            except _ERRORS as exc:
                self.failures.append(f"page {number}: vector group at ({x0:.1f}, {y0:.1f}): {exc}")
                continue
            asset = self._add(
                ("vector", digest),
                lambda data=data, digest=digest, group=group, name=f"p{number:03d}-v{index:02d}": self._write_vector(
                    name, digest, data, len(group.shapes), group.text
                ),
                Placement(number, x0, y0, x1 - x0, y1 - y0),
            )
            self._annotate(asset, number, group.mcids, None)
            self.marked_boxes += [(number, mcid, group.bbox) for mcid in group.mcids]

    def _note_colours(self, group: Group) -> None:
        for shape in group.shapes:
            area = (shape.bbox[2] - shape.bbox[0]) * (shape.bbox[3] - shape.bbox[1])
            for colour in (_describe(shape.fill), _describe(shape.stroke)):
                if colour:
                    self.artwork_colours[colour] = self.artwork_colours.get(colour, 0.0) + area

    def _write_vector(self, name: str, digest: str, data: bytes, shapes: int, text: list[str]) -> Asset:
        target = self.out / "vector" / f"{name}.pdf"
        target.parent.mkdir(exist_ok=True)
        target.write_bytes(data)
        return Asset(
            "vector", target.relative_to(self.out).as_posix(), [], {"shapes": shapes, "digest": digest, "text": text}
        )

    def fonts(self) -> None:
        for obj in _objects(self.pdf):
            if not isinstance(obj, pikepdf.Dictionary) or str(obj.get("/Type")) != "/FontDescriptor":
                continue
            for key, suffix in _FONT_FILES.items():
                program = obj.get(key)
                if program is None:
                    continue
                full = str(obj.get("/FontName", "unnamed")).lstrip("/")
                name = _SUBSET_PREFIX.sub("", full)
                kind = str(program.get("/Subtype", "")).lstrip("/")
                if key == "/FontFile3":
                    suffix = ".otf" if f"/{kind}" == _OPENTYPE else ".cff"
                self._add(
                    ("font", program.objgen),
                    lambda program=program, full=full, name=name, kind=kind, suffix=suffix, d=obj: self._write_font(
                        program, full, name, kind, suffix, _descriptor_facts(d, name)
                    ),
                )
        for asset in self.assets:
            if asset.kind == "font":  # a font has no position; the pages that set text in it go in the details
                asset.details["pages"] = sorted(self._font_pages.get(asset.details["name"], ()))

    def _write_font(
        self, program: pikepdf.Object, full: str, name: str, kind: str, suffix: str, descriptor: dict[str, typing.Any]
    ) -> Asset | None:
        target = self.out / "fonts" / f"{_slug(full)}{suffix}"
        try:
            target.parent.mkdir(exist_ok=True)
            target.write_bytes(bytes(program.read_bytes()))
        except _ERRORS as exc:
            self.failures.append(f"font {full}: {exc}")
            return None
        return Asset(
            "font",
            target.relative_to(self.out).as_posix(),
            [],
            {
                "name": name,
                "full_name": full,
                "subset": full != name,
                "program": kind or suffix.lstrip("."),
                "descriptor": descriptor,
            },
        )

    def profiles(self) -> list[dict[str, typing.Any]]:
        """Write every ICC profile (output intents and ICCBased spaces); return the output intents."""
        written: dict[str, dict[str, typing.Any]] = {}

        def save(stream: pikepdf.Object, role: str) -> dict[str, typing.Any] | None:
            try:
                data = bytes(stream.read_bytes())
            except _ERRORS as exc:
                self.failures.append(f"ICC profile: {exc}")
                return None
            digest = hashlib.sha1(data).hexdigest()
            if digest not in written:
                description = _profile_description(data)
                target = self.out / "icc" / f"{_slug(description or 'profile')}-{digest[:8]}.icc"
                target.parent.mkdir(exist_ok=True)
                target.write_bytes(data)
                written[digest] = {
                    "file": target.relative_to(self.out).as_posix(),
                    "description": description,
                    "components": int(stream.get("/N", 0)),
                    "roles": [],
                }
                self.assets.append(Asset("icc", written[digest]["file"], [], written[digest]))
            if role not in written[digest]["roles"]:
                written[digest]["roles"].append(role)
            return written[digest]

        intents = []
        for intent in self.pdf.Root.get("/OutputIntents", []):
            profile = intent.get("/DestOutputProfile")
            saved = save(profile, "output intent") if profile is not None else None
            intents.append(
                {
                    "subtype": str(intent.get("/S", "")).lstrip("/"),
                    "output_condition_identifier": _text(intent.get("/OutputConditionIdentifier")),
                    "output_condition": _text(intent.get("/OutputCondition")),
                    "registry": _text(intent.get("/RegistryName")),
                    "profile": saved["file"] if saved else None,
                }
            )
        for obj in _objects(self.pdf):
            if isinstance(obj, pikepdf.Array) and len(obj) == 2 and str(obj[0]) == "/ICCBased":
                save(obj[1], "colour space")
        return intents


def _notes(runs: list[TextRun]) -> list[str]:
    unreadable = collections.Counter(run.font for run in runs if run.text is None)
    if not unreadable:
        return []
    fonts = ", ".join(f"{name} ({count})" for name, count in unreadable.most_common())
    return [
        f"{sum(unreadable.values())} text run(s) could not be read: their fonts have no /ToUnicode map, so the PDF "
        f"does not say which characters they show ({fonts}). Their font codes are in `text[].codes`; decode them "
        "against the recovered font program, or read the pages in previews/ (--previews) by eye or with OCR."
    ]


def _lines(runs: list[TextRun]) -> list[Line]:
    """Runs of one style on one baseline, left to right, joined into lines; a line of unreadable runs has no text."""
    rows: dict[tuple, list[TextRun]] = {}
    for run in runs:
        rows.setdefault(
            (run.page, run.font, round(run.size_pt, 1), run.colour, round(run.y), round(run.rotation_deg), run.struct),
            [],
        ).append(run)
    lines: list[Line] = []
    for row in rows.values():
        current: list[TextRun] = []
        for run in sorted(row, key=lambda r: r.x):
            if current and run.x - (current[-1].x + current[-1].width) > LINE_GAP_EM * run.size_pt:
                lines.append(_join(current))
                current = []
            current.append(run)
        lines.append(_join(current))
    return sorted(lines, key=lambda line: (line.page, -line.y, line.x))


def _join(runs: list[TextRun]) -> Line:
    text = runs[0].text or ""
    for prev, run in zip(runs, runs[1:], strict=False):
        gap = run.x - (prev.x + prev.width)
        spaced = text.endswith(" ") or (run.text or "").startswith(" ")
        text += (" " if gap > SPACE_EM * run.size_pt and not spaced else "") + (run.text or "")
    unreadable = any(run.text is None for run in runs)
    first, last = runs[0], runs[-1]
    height = max(r.height for r in runs)
    return Line(
        first.page,
        None if unreadable else text.strip(),
        first.x,
        first.y,
        last.x + last.width - first.x,
        height,
        first.font,
        first.size_pt,
        first.colour,
        first.rotation_deg,
        first.h_scale,
        first.letter_spacing_em,
        first.tag,
        first.struct,
        first.artifact,
    )


def _merge_boxes(boxes: list[Box]) -> list[Box]:
    """Join touching collinear rules of one style (a rule drawn in pieces); panels stay as they are."""
    result: list[Box] = []
    rules: dict[tuple, list[Box]] = {}
    for box in boxes:
        horizontal = box.height <= HAIRLINE_PT and box.width > box.height
        vertical = box.width <= HAIRLINE_PT and box.height > box.width
        if not (horizontal or vertical):
            result.append(box)
            continue
        cross = round((box.y if horizontal else box.x) * 4)
        style = (box.page, horizontal, cross, box.fill, box.stroke, box.stroke_width_pt)
        rules.setdefault(style, []).append(box)
    for (_page, horizontal, *_), pieces in rules.items():
        pieces.sort(key=lambda b: b.x if horizontal else b.y)
        run = pieces[0]
        for box in pieces[1:]:
            end = run.x + run.width if horizontal else run.y + run.height
            if (box.x if horizontal else box.y) <= end + JOIN_PT:
                grown = max(end, box.x + box.width if horizontal else box.y + box.height)
                run = dataclasses.replace(
                    run, **({"width": grown - run.x} if horizontal else {"height": grown - run.y})
                )
            else:
                result.append(_rule(run, horizontal))
                run = box
        result.append(_rule(run, horizontal))
    return sorted(result, key=lambda b: (b.page, -b.y, b.x))


def _rule(box: Box, horizontal: bool) -> Box:
    """*box* with a line's end points recomputed from its (possibly grown) extent."""
    if box.kind != "line":
        return box
    mid = box.y + box.height / 2 if horizontal else box.x + box.width / 2
    ends = [[box.x, mid], [box.x + box.width, mid]] if horizontal else [[mid, box.y], [mid, box.y + box.height]]
    return dataclasses.replace(box, points=ends)


_FONT_FLAGS = {1: "fixed-pitch", 2: "serif", 3: "symbolic", 4: "script", 6: "nonsymbolic", 7: "italic"}
_FONT_FLAGS |= {
    17: "all-caps",
    18: "small-caps",
    19: "force-bold",
}  # bit numbers, counted from 1, as the PDF spec has them
_BOLD_WORDS = re.compile(r"bold|black|heavy|demi|semibold", re.IGNORECASE)
_ITALIC_WORDS = re.compile(r"italic|oblique|slanted", re.IGNORECASE)


def _descriptor_facts(descriptor: pikepdf.Object, name: str) -> dict[str, typing.Any]:
    """What a font descriptor says about the face: its flags, weight, slant and metrics (1000-unit em), with bold and
    italic worked out from the flags, the numbers and, failing those, the name."""
    flags = int(descriptor.get("/Flags", 0))
    named = [label for bit, label in _FONT_FLAGS.items() if flags & (1 << (bit - 1))]
    weight, angle = descriptor.get("/FontWeight"), float(descriptor.get("/ItalicAngle", 0) or 0)
    facts: dict[str, typing.Any] = {
        "family": _text(descriptor.get("/FontFamily")),
        "weight": int(weight) if weight is not None else None,
        "stretch": _text(descriptor.get("/FontStretch")),
        "italic_angle": angle,
        "flags": named,
        "bold": "force-bold" in named or (weight is not None and int(weight) >= 700) or bool(_BOLD_WORDS.search(name)),
        "italic": "italic" in named or angle != 0 or bool(_ITALIC_WORDS.search(name)),
        "ascent": _number(descriptor.get("/Ascent")),
        "descent": _number(descriptor.get("/Descent")),
        "cap_height": _number(descriptor.get("/CapHeight")),
        "x_height": _number(descriptor.get("/XHeight")),
        "stem_v": _number(descriptor.get("/StemV")),
        "bbox": [float(v) for v in descriptor["/FontBBox"]] if "/FontBBox" in descriptor else None,
    }
    return {k: v for k, v in facts.items() if v is not None}


def _number(value: typing.Any) -> float | None:
    try:
        return float(value)
    except TypeError, ValueError:
        return None


def _objects(pdf: pikepdf.Pdf) -> collections.abc.Iterator[pikepdf.Object]:
    """Every object reachable from the catalog, direct ones included (an ICCBased array is usually inside a dict)."""
    stack: list[pikepdf.Object] = [pdf.Root]
    seen: set[tuple[int, int]] = set()
    while stack:
        obj = stack.pop()
        if not isinstance(obj, pikepdf.Object):  # a number, bool or null comes back as a Python value
            continue
        if obj.objgen != (0, 0):
            if obj.objgen in seen:
                continue
            seen.add(obj.objgen)
        yield obj
        if isinstance(obj, (pikepdf.Dictionary, pikepdf.Stream)):
            stack += [value for key, value in obj.items() if key != "/Parent"]
        elif isinstance(obj, pikepdf.Array):
            stack += list(obj)


def _metadata(pdf: pikepdf.Pdf, out_dir: pathlib.Path, failures: list[str]) -> dict[str, typing.Any]:
    """The document info, and the XMP packet both saved as ``xmp.xml`` and decoded (long values, such as embedded
    thumbnails, are left in the file and named in the data)."""
    info = {str(k).lstrip("/"): str(v) for k, v in pdf.docinfo.items()}
    packet = pdf.Root.get("/Metadata")
    decoded: dict[str, typing.Any] | None = None
    saved = None
    if packet is not None:
        try:
            data = bytes(packet.read_bytes())
            (out_dir / "xmp.xml").write_bytes(data)
            saved = "xmp.xml"
            decoded = _elide(xmp.parse(data))
        except (*_ERRORS, ValueError) as exc:
            failures.append(f"XMP metadata: {exc}")
    return {"pdf_version": pdf.pdf_version, "info": info, "xmp": decoded, "xmp_file": saved}


def _elide(value: typing.Any) -> typing.Any:
    if isinstance(value, str) and len(value) > MAX_XMP_VALUE:
        return f"<{len(value)} characters; see xmp.xml>"
    if isinstance(value, dict):
        return {key: _elide(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_elide(item) for item in value]
    return value


def _figures(assets: list[Asset]) -> list[Figure]:
    return [
        Figure(p.page, a.kind, a.file, p.x, p.y, p.width, p.height, bool(a.details.get("text")))
        for a in assets
        if a.kind in ("image", "vector")
        for p in a.placements
    ]


_CONTENT_COLOURS = {"g": GRAY, "G": GRAY, "rg": RGB, "RG": RGB, "k": CMYK, "K": CMYK}


def _describe(raw: str | None) -> str | None:
    """A content-stream colour (``k 0 0 0 0.65``) as the inventory writes colours; anything else is returned as is."""
    if not raw:
        return raw
    if raw.startswith("spot "):  # "spot NAME 0.5", from the vector module, as the text colours write it
        name, _, tint = raw.removeprefix("spot ").rpartition(" ")
        return f"spot {name} {round(float(tint) * 100)}%" if name else raw
    operator, *operands = raw.split()
    family = _CONTENT_COLOURS.get(operator)
    try:
        values = tuple(float(v) for v in operands)
    except ValueError:
        return raw
    return contrast.describe(Fill(family, values)) if family else raw


def _structure(
    tagged: tagged_reader.Tagged | None, runs: list[TextRun], figures: list[tuple[int, int, tuple]]
) -> dict[str, typing.Any]:
    """The structure tree as data: every element with its roles, alt text and attributes, and, from the content the
    tree points at, the text it holds (for paragraph-like elements) and the box it covers on its page."""
    if tagged is None:
        return {"tagged": False, "language": None, "role_map": {}, "elements": []}
    held: dict[int, list[TextRun]] = collections.defaultdict(list)
    boxes: dict[int, list[tuple[float, float, float, float]]] = collections.defaultdict(list)

    def climb(start: tagged_reader.Element | None) -> typing.Iterator[tagged_reader.Element]:
        while start is not None:
            yield start
            start = tagged.elements[start.parent] if start.parent is not None else None

    for run in runs:
        for element in climb(tagged.at(run.page, run.mcid)):
            held[element.id].append(run)
            boxes[element.id].append((run.x, run.y, run.x + run.width, run.y + run.height))
    for page, mcid, box in figures:
        for element in climb(tagged.at(page, mcid)):
            boxes[element.id].append(box)
    out = []
    for element in tagged.elements:
        reads = element.marks or element.role in tagged_reader.LEAF_BLOCKS
        text = [ln.text for ln in _lines(held[element.id]) if ln.text] if reads else []
        record: dict[str, typing.Any] = {
            "id": element.id,
            "parent": element.parent,
            "tag": element.tag,
            "role": element.role,
            "page": element.page,
            "block": element.block,
            "alt": element.alt,
            "actual_text": element.actual_text,
            "title": element.title,
            "lang": element.lang,
            "attributes": element.attrs or None,
            "text": " ".join(text) if text else None,
            "box": list(_union_boxes(boxes[element.id])) if boxes[element.id] else None,
        }
        out.append({k: v for k, v in record.items() if v is not None})
    return {"tagged": tagged.marked, "language": tagged.language, "role_map": tagged.role_map, "elements": out}


def _union_boxes(rects: list[tuple[float, float, float, float]]) -> tuple[float, float, float, float]:
    return (min(r[0] for r in rects), min(r[1] for r in rects), max(r[2] for r in rects), max(r[3] for r in rects))


def _tables(lines: list[Line], boxes: list[Box], pages: list[dict[str, typing.Any]]) -> list[Table]:
    found: list[Table] = []
    for page in pages:
        number = page["number"]
        rules = [
            (b.x, b.y, b.width, b.height)
            for b in boxes
            if b.page == number and min(b.width, b.height) <= HAIRLINE_PT and b.width != b.height
        ]
        found += table_finder.detect([ln for ln in lines if ln.page == number], rules, number)
    return found


def _spot_colours(pdf: pikepdf.Pdf, pages: list[dict[str, typing.Any]]) -> list[dict[str, typing.Any]]:
    return [
        {
            "names": list(spot.names),
            "alternate_space": spot.alternate_space,
            "alternate": spot.alternate,  # components of that space at full tint, e.g. CMYK 0..1 or Lab
            "pages": [p["number"] for p in pages if set(spot.names) & set(p["spot_inks"])],
        }
        for spot in spots.find(_objects(pdf))
    ]


def _spot_of(colour: str, spot_colours: list[dict[str, typing.Any]]) -> dict[str, typing.Any] | None:
    """The spot colour a ``spot NAME 100%`` palette colour names."""
    name = colour.removeprefix("spot ").rsplit(" ", 1)[0] if colour.startswith("spot ") else None
    return next((s for s in spot_colours if name and "+".join(s["names"]) == name), None)


def _palette(
    boxes: list[Box], layout: Layout, spot_colours: list[dict[str, typing.Any]], artwork_colours: dict[str, float]
) -> list[dict[str, typing.Any]]:
    """The colours in use, each with what it colours: panel area, vector artwork area and text characters."""
    uses: dict[str, dict[str, float]] = {}

    def use(colour: str) -> dict[str, float]:
        return uses.setdefault(colour, {"panel_area_pt2": 0.0, "artwork_area_pt2": 0.0, "text_chars": 0})

    for box in boxes:
        if box.fill_colour and min(box.width, box.height) > HAIRLINE_PT:
            use(box.fill_colour)["panel_area_pt2"] += box.width * box.height
    for colour, area in artwork_colours.items():
        use(colour)["artwork_area_pt2"] += area
    for style in layout.styles:
        use(style.colour)["text_chars"] += style.chars
    palette = [
        {"colour": colour, **used, **({"spot": spot} if (spot := _spot_of(colour, spot_colours)) else {})}
        for colour, used in uses.items()
    ]
    return sorted(palette, key=lambda e: (-e["panel_area_pt2"] - e["artwork_area_pt2"], -e["text_chars"]))


def _filters(obj: pikepdf.Object) -> list[str]:
    value = obj.get("/Filter")
    if value is None:
        return []
    return [str(f).lstrip("/") for f in (value if isinstance(value, pikepdf.Array) else [value])]


def _profile_description(data: bytes) -> str | None:
    try:
        return ImageCms.getProfileDescription(ImageCms.ImageCmsProfile(io.BytesIO(data))).strip() or None
    except ImageCms.PyCMSError, OSError, ValueError, TypeError:
        return None


def _text(value: pikepdf.Object | None) -> str | None:
    return None if value is None else str(value)


def _slug(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", name).strip("_")[:80] or "unnamed"
