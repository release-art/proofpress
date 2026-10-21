"""Rasterise a print PDF to PNG proofs, optionally with the page-box guides.

A proofing companion to ``verify``: it renders each page with Ghostscript and,
when asked, draws the finished (trim) edge, the bleed edge and a safe-area inset
straight from the PDF's own ``/TrimBox`` and ``/BleedBox`` -- so the guides are
whatever the file actually declares, never a hand-drawn guess. That is the point
of the overlay: a wrong TrimBox is a number in a report, but a red rectangle in
the wrong place is obvious.

Ghostscript is required (as for ink coverage); its absence or failure is a hard
error, not a silent skip.
"""

import asyncio
import dataclasses
import logging
import pathlib

import pikepdf
from PIL import Image, ImageDraw, ImageFont

from . import proc, structure
from .colour import GhostscriptError, gs_available
from .models import MM_PER_IN, PT_PER_IN

logger = logging.getLogger(__name__)

# Guide colours (RGB): the finished edge is the one that matters, so it is the
# boldest; bleed and safe area support it.
_TRIM_RGB = (214, 34, 34)
_BLEED_RGB = (0, 110, 210)
_SAFE_RGB = (0, 150, 70)

# Re-exported so the CLI can gate on the tool without reaching into colour.
gs_available = gs_available
GhostscriptError = GhostscriptError


@dataclasses.dataclass(frozen=True)
class PageBoxes:
    """The boxes a preview cares about for one page (points; trim/bleed optional)."""

    media: structure.Rect | None
    trim: structure.Rect | None
    bleed: structure.Rect | None


def page_boxes(pdf: pikepdf.Pdf) -> list[PageBoxes]:
    """Read the media/trim/bleed box of every page, resolving inherited media."""
    return [
        PageBoxes(
            media=structure.media_box(page.obj),
            trim=structure.own_box(page.obj, "TrimBox"),
            bleed=structure.own_box(page.obj, "BleedBox"),
        )
        for page in pdf.pages
    ]


async def preview_file(
    pdf_path: pathlib.Path,
    out_dir: pathlib.Path,
    *,
    dpi: int,
    guides: bool,
    safe_mm: float,
    gs_bin: str,
    timeout: float | None = None,
) -> list[pathlib.Path]:
    """Render *pdf_path* to one PNG per page in *out_dir*; overlay guides if asked."""
    pngs = await _render_pages(gs_bin, pdf_path, out_dir, dpi, timeout=timeout)
    if guides:
        await asyncio.to_thread(_draw_guides, pdf_path, pngs, dpi, safe_mm)
    return pngs


def _draw_guides(pdf_path: pathlib.Path, pngs: list[pathlib.Path], dpi: int, safe_mm: float) -> None:
    with pikepdf.open(pdf_path) as pdf:
        boxes = page_boxes(pdf)
    for png, box in zip(pngs, boxes, strict=False):
        _overlay_guides(png, box, dpi, safe_mm)


async def _render_pages(
    gs_bin: str,
    pdf_path: pathlib.Path,
    out_dir: pathlib.Path,
    dpi: int,
    *,
    timeout: float | None = None,
) -> list[pathlib.Path]:
    """Ghostscript-render each page to ``<stem>-p<N>.png`` and return the paths."""
    out_dir.mkdir(parents=True, exist_ok=True)
    pattern = str(out_dir / f"{pdf_path.stem}-p%d.png")
    try:
        result = await proc.run(
            [
                gs_bin,
                "-q",
                "-dNOPAUSE",
                "-dBATCH",
                "-dSAFER",
                "-sDEVICE=png16m",
                f"-r{dpi}",
                f"-sOutputFile={pattern}",
                str(pdf_path),
            ],
            timeout=timeout,
        )
    except proc.ProcTimeoutError as exc:
        raise GhostscriptError(f"ghostscript timed out after {timeout:.0f}s") from exc
    if result.returncode != 0:
        message = result.stderr.strip() or "ghostscript failed"
        logger.error(f"ghostscript failed (rc={result.returncode}): {message}")
        raise GhostscriptError(message)
    return sorted(out_dir.glob(f"{pdf_path.stem}-p*.png"), key=_page_number)


def _page_number(png: pathlib.Path) -> int:
    return int(png.stem.rsplit("-p", 1)[1])


def _overlay_guides(png: pathlib.Path, boxes: PageBoxes, dpi: int, safe_mm: float) -> None:
    """Draw the trim/bleed/safe rectangles onto a rendered page, in place."""
    if boxes.media is None:  # cannot map points to pixels without the media origin
        return
    with Image.open(png) as opened:
        img = opened.convert("RGB")
    draw = ImageDraw.Draw(img)
    to_px = _mapper(boxes.media, img.height, dpi)
    line = max(1, round(dpi / 150))
    font = ImageFont.load_default(size=max(10, round(dpi / 15)))

    if boxes.bleed is not None and boxes.bleed != boxes.media:
        _guide(draw, to_px(boxes.bleed), _BLEED_RGB, line, "bleed", font)
    if boxes.trim is not None:
        _guide(draw, to_px(boxes.trim), _TRIM_RGB, line + 1, "trim", font)
        _guide(draw, to_px(_inset(boxes.trim, safe_mm)), _SAFE_RGB, line, "safe", font)
    img.save(png)


def _guide(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], rgb, width: int, label: str, font) -> None:
    """A labelled guide rectangle: the outline plus its name at the top-left corner."""
    draw.rectangle(box, outline=rgb, width=width)
    draw.text(
        (box[0] + width + 2, box[1] + width + 1),
        label,
        fill=rgb,
        font=font,
        stroke_width=max(1, width),
        stroke_fill=(255, 255, 255),  # white halo keeps the label legible over artwork
    )


def _mapper(media: structure.Rect, height_px: int, dpi: int):
    """A function turning a points rectangle into a pixel box on the raster."""
    mllx, mlly = media[0], media[1]
    scale = dpi / PT_PER_IN

    def to_px(rect: structure.Rect) -> tuple[int, int, int, int]:
        x0 = round((rect[0] - mllx) * scale)
        x1 = round((rect[2] - mllx) * scale)
        y_top = round(height_px - (rect[3] - mlly) * scale)  # PDF y is bottom-up
        y_bot = round(height_px - (rect[1] - mlly) * scale)
        return (x0, y_top, x1, y_bot)

    return to_px


def _inset(rect: structure.Rect, mm: float) -> structure.Rect:
    d = mm / MM_PER_IN * PT_PER_IN
    return (rect[0] + d, rect[1] + d, rect[2] - d, rect[3] - d)
