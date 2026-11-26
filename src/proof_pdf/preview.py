"""Rasterise a print PDF to PNG proofs, optionally with the page-box guides.

A proofing companion to ``verify``: it renders each page, and when asked draws the finished (trim) edge, the bleed
edge and a safe-area inset straight from the PDF's own ``/TrimBox`` and ``/BleedBox``, so the guides are whatever the
file actually declares, never a hand-drawn guess. That is the point of the overlay: a wrong TrimBox is a number in a
report, but a red rectangle in the wrong place is obvious.

Pages are rendered by the Rust core with ``hayro``, which reads files leniently, as a viewer does: a damaged file still
gets a picture. The whole MediaBox is rendered; anything outside the CropBox is left blank. No external tool is needed.
"""

import asyncio
import pathlib

from ._internal import _core

UnreadablePdfError = _core.UnreadablePdfError

# Guide colours (RGB), as the core draws them: the finished edge is the one that matters, so it is the boldest; bleed
# and safe area support it.
_TRIM_RGB = (214, 34, 34)
_BLEED_RGB = (0, 110, 210)
_SAFE_RGB = (0, 150, 70)


def render_pdf(
    pdf_path: pathlib.Path, out_dir: pathlib.Path, *, dpi: int, guides: bool, safe_mm: float | None
) -> list[pathlib.Path]:
    """Render *pdf_path* to ``<stem>-p<N>.png`` in *out_dir*, one per page, and return their paths in page order.

    Raises :class:`UnreadablePdfError` for a file that cannot be read, and :class:`ValueError` for a page too large to
    render at *dpi*.
    """
    pngs = _core.render(pdf_path.read_bytes(), dpi, guides, safe_mm)
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for number, data in enumerate(pngs, start=1):
        target = out_dir / f"{pdf_path.stem}-p{number}.png"
        target.write_bytes(data)
        written.append(target)
    return written


async def preview_file(
    pdf_path: pathlib.Path, out_dir: pathlib.Path, *, dpi: int, guides: bool, safe_mm: float | None
) -> list[pathlib.Path]:
    """:func:`render_pdf` in a worker thread, so a batch of files does not block the event loop."""
    return await asyncio.to_thread(render_pdf, pdf_path, out_dir, dpi=dpi, guides=guides, safe_mm=safe_mm)
