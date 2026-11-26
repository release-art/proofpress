"""Cut a rectangle of a page out as a one-page PDF of its own.

For the illustration the extraction missed, or never could name: give the page and the box and get that part of the page
back as a standalone PDF, ready for ``\\includegraphics`` or to hand to an image model. It is the page's own content
shown through a smaller window, so it is exact (text stays text, vectors stay vectors, images keep their pixels), at the
price of the file also holding what lies outside the window. Where that matters, rasterise it.

Coordinates are PDF points with the origin at the page's bottom-left, in the page's unrotated space: the same numbers
``document.json`` gives every placement (``x``, ``y``, ``width``, ``height``), so an asset's place can be cut out as it
stands. From a picture of the page (a preview PNG), a pixel (px, py) at *dpi* is
``x = media_x0 + px / dpi * 72`` and ``y = media_y1 - py / dpi * 72``.
"""

import pathlib

from proof_pdf._internal import _core

Box = tuple[float, float, float, float]  # x0, y0, x1, y1

UnreadablePdfError = _core.UnreadablePdfError
"""Raised for a PDF that cannot be read: its structure is too damaged, or it is encrypted."""


def crop_page(path: pathlib.Path, page: int, box: Box, out: pathlib.Path, *, pad_pt: float = 0.0) -> pathlib.Path:
    """Write *box* of page number *page* (counting from 1) of *path* to *out* as a one-page PDF, and return *out*.

    *box* is ``(x0, y0, x1, y1)`` in points, bottom-left origin, in either corner order; *pad_pt* widens it on every
    side, and the result is kept within the page. The copy has no annotations (a link or a form field would point
    outside the window) and no trim, bleed or art boxes of its own, and leaves out resources its content never names.
    Raises :class:`ValueError` for a page that does not exist or a box that misses the page, and
    :class:`UnreadablePdfError` for a file that cannot be read.
    """
    try:
        piece = _core.crop(path.read_bytes(), page, box, pad_pt)
    except ValueError as exc:
        raise ValueError(f"{path.name}: {exc}") from None
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(piece)
    return out
