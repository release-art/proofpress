"""What is in a print PDF, as plain data: the facts a proofer or a downstream tool reads.

``verify`` judges; this reports. It lists the fonts (embedded or not, how many glyphs, any ``.notdef``), the text
colours and sizes, the spot inks, the images with their effective resolution, the thinnest line, and, when a profile is
available, the L* and CIEDE2000 difference of every text/background pair. Every value is a plain dataclass field, so a
caller can apply its own rules, or print it with ``proof-pdf inspect --json``.
"""

import asyncio
import dataclasses
import json
import pathlib

from ._internal import _core

UnreadablePdfError = _core.UnreadablePdfError


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
    """Read *path* and report what is in it. Raises :class:`UnreadablePdfError` for a file that will not open."""
    profile = proof_profile.read_bytes() if proof_profile else None
    facts = json.loads(_core.inspect(path.read_bytes(), profile, with_contrast))
    return Inventory(
        path=str(path),
        pages=[
            PageFact(**{**p, "media_mm": _pair(p["media_mm"]), "trim_mm": _pair(p["trim_mm"])}) for p in facts["pages"]
        ],
        fonts=[FontFact(**{**f, "pages": tuple(f["pages"])}) for f in facts["fonts"]],
        text_colours=[TextColourFact(**{**c, "pages": tuple(c["pages"])}) for c in facts["text_colours"]],
        spot_inks=facts["spot_inks"],
        images=[ImageFact(**{**i, "pixels": tuple(i["pixels"])}) for i in facts["images"]],
        contrast=[ContrastFact(**c) for c in facts["contrast"]],
        contrast_error=facts["contrast_error"],
    )


def _pair(value: list[float] | None) -> tuple[float, float] | None:
    return None if value is None else (value[0], value[1])


async def inspect_file(
    path: pathlib.Path, *, proof_profile: pathlib.Path | None = None, with_contrast: bool = True
) -> Inventory:
    """:func:`inspect_pdf` in a worker thread, so a batch of files does not block the event loop."""
    return await asyncio.to_thread(inspect_pdf, path, proof_profile=proof_profile, with_contrast=with_contrast)
