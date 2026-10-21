"""What is in a print PDF, as plain data: the facts a proofer or a downstream tool reads.

``verify`` judges; this reports. It lists the fonts (embedded or not, how many glyphs, any ``.notdef``), the text
colours and sizes, the spot inks, the images with their effective resolution, the thinnest line, and, when a profile is
available, the L* and CIEDE2000 difference of every text/background pair. Every value is a plain dataclass field, so a
caller can apply its own rules, or print it with ``proof-pdf inspect --json``.
"""

import asyncio
import dataclasses
import pathlib

import pikepdf

from ._internal import contrast, fonts, structure
from ._internal.paint import Paint, Walker
from ._internal.spaces import classify_colour_space
from .models import PT_PER_IN, pt_to_mm


@dataclasses.dataclass(frozen=True)
class FontFact:
    name: str
    subtype: str
    embedded: bool
    has_to_unicode: bool
    pages: tuple[int, ...]
    glyphs: int
    notdef: int


@dataclasses.dataclass(frozen=True)
class TextColourFact:
    colour: str
    pages: tuple[int, ...]
    runs: int
    smallest_pt: float


@dataclasses.dataclass(frozen=True)
class ImageFact:
    page: int
    name: str
    colour_space: str  # "rgb", "cmyk", "gray" or "other"
    pixels: tuple[int, int]
    dpi: float | None  # effective, from its placed footprint; None for a degenerate one


@dataclasses.dataclass(frozen=True)
class ContrastFact:
    page: int
    text: str
    background: str  # a colour, "paper" or "an image or shading"
    runs: int
    smallest_pt: float
    delta_l: float | None
    delta_e: float | None


@dataclasses.dataclass(frozen=True)
class PageFact:
    number: int
    media_mm: tuple[float, float] | None
    trim_mm: tuple[float, float] | None
    text_runs: int
    smallest_text_pt: float | None
    thinnest_stroke_pt: float | None


@dataclasses.dataclass(frozen=True)
class Inventory:
    path: str
    pages: list[PageFact]
    fonts: list[FontFact]
    text_colours: list[TextColourFact]
    spot_inks: list[str]
    images: list[ImageFact]
    contrast: list[ContrastFact]
    contrast_error: str | None = None  # why contrast could not be measured, e.g. no CMYK profile

    def to_dict(self) -> dict:
        return dataclasses.asdict(self)


def inspect_pdf(
    path: pathlib.Path, *, proof_profile: pathlib.Path | None = None, with_contrast: bool = True
) -> Inventory:
    """Read *path* and report what is in it. Raises :class:`pikepdf.PdfError` for a file that will not open."""
    with pikepdf.open(path) as pdf:
        walks = [(number, *_walk(page)) for number, page in enumerate(pdf.pages, start=1)]
        pages = [_page_fact(number, page, events) for (number, events, _), page in zip(walks, pdf.pages, strict=True)]
        contrast_facts, error = _contrast(pdf, proof_profile) if with_contrast else ([], None)
        return Inventory(
            path=str(path),
            pages=pages,
            fonts=_font_facts(walks),
            text_colours=_text_colours(walks),
            spot_inks=sorted({name for _, _, spots in walks for name in spots}),
            images=[_image_fact(number, e) for number, events, _ in walks for e in events if e.kind == "image"],
            contrast=contrast_facts,
            contrast_error=error,
        )


async def inspect_file(
    path: pathlib.Path, *, proof_profile: pathlib.Path | None = None, with_contrast: bool = True
) -> Inventory:
    """:func:`inspect_pdf` in a worker thread, so a batch of files does not block the event loop."""
    return await asyncio.to_thread(inspect_pdf, path, proof_profile=proof_profile, with_contrast=with_contrast)


def _walk(page: pikepdf.Page) -> tuple[list[Paint], set[str]]:
    walker = Walker()
    return list(walker.walk(page)), walker.spots


def _page_fact(number: int, page: pikepdf.Page, events: list[Paint]) -> PageFact:
    media, trim = structure.media_box(page.obj), structure.own_box(page.obj, "TrimBox")
    runs = [e for e in events if e.kind == "text" and e.glyphs]
    strokes = [e.stroke_width for e in events if e.stroke_width is not None]
    return PageFact(
        number=number,
        media_mm=_size_mm(media),
        trim_mm=_size_mm(trim),
        text_runs=len(runs),
        smallest_text_pt=min((r.size for r in runs if r.size > 0), default=None),
        thinnest_stroke_pt=min(strokes, default=None),
    )


def _size_mm(rect: structure.Rect | None) -> tuple[float, float] | None:
    if rect is None:
        return None
    return (round(pt_to_mm(rect[2] - rect[0]), 2), round(pt_to_mm(rect[3] - rect[1]), 2))


def _font_facts(walks: list[tuple[int, list[Paint], set[str]]]) -> list[FontFact]:
    grouped: dict[tuple[str, str], dict] = {}
    for number, events, _ in walks:
        for e in events:
            if e.kind != "text" or e.font is None or e.font is fonts.UNKNOWN:
                continue
            entry = grouped.setdefault(
                (e.font.name, e.font.subtype), {"font": e.font, "pages": set(), "glyphs": 0, "notdef": 0}
            )
            entry["pages"].add(number)
            entry["glyphs"] += e.glyphs
            entry["notdef"] += e.notdef
    return [
        FontFact(
            name=name,
            subtype=subtype,
            embedded=entry["font"].embedded,
            has_to_unicode=entry["font"].has_to_unicode,
            pages=tuple(sorted(entry["pages"])),
            glyphs=entry["glyphs"],
            notdef=entry["notdef"],
        )
        for (name, subtype), entry in sorted(grouped.items())
    ]


def _text_colours(walks: list[tuple[int, list[Paint], set[str]]]) -> list[TextColourFact]:
    grouped: dict[str, dict] = {}
    for number, events, _ in walks:
        for e in events:
            if e.kind == "text" and e.glyphs:
                entry = grouped.setdefault(contrast.describe(e.fill), {"pages": set(), "runs": 0, "sizes": []})
                entry["pages"].add(number)
                entry["runs"] += 1
                entry["sizes"].append(e.size)
    return [
        TextColourFact(colour, tuple(sorted(entry["pages"])), entry["runs"], min(entry["sizes"]))
        for colour, entry in sorted(grouped.items())
    ]


def _image_fact(page: int, event: Paint) -> ImageFact:
    xobject = event.xobject
    assert xobject is not None
    width, height = int(xobject.get("/Width", 0)), int(xobject.get("/Height", 0))
    dpi = None
    if event.bbox and width and height:
        inches = (event.bbox[2] - event.bbox[0]) / PT_PER_IN
        dpi = round(width / inches, 1) if inches > 0 else None
    return ImageFact(
        page=page,
        name=event.name,
        colour_space=classify_colour_space(xobject.get("/ColorSpace"), None),
        pixels=(width, height),
        dpi=dpi,
    )


def _contrast(pdf: pikepdf.Pdf, proof_profile: pathlib.Path | None) -> tuple[list[ContrastFact], str | None]:
    try:
        pairs = contrast.measure(pdf, contrast.ColourModel.for_pdf(pdf, proof_profile))
    except contrast.NoProfileError as exc:
        return [], str(exc)
    return [
        ContrastFact(
            page=p.page,
            text=contrast.describe(p.text),
            background="paper" if p.on_paper else contrast.describe(p.ground),
            runs=p.runs,
            smallest_pt=round(p.smallest_pt, 2),
            delta_l=None if p.delta_l is None else round(p.delta_l, 2),
            delta_e=None if p.delta_e is None else round(p.delta_e, 2),
        )
        for p in pairs
    ], None
