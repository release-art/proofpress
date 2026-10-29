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

import pikepdf

Box = tuple[float, float, float, float]  # x0, y0, x1, y1


def crop_page(path: pathlib.Path, page: int, box: Box, out: pathlib.Path, *, pad_pt: float = 0.0) -> pathlib.Path:
    """Write *box* of page number *page* (counting from 1) of *path* to *out* as a one-page PDF, and return *out*.

    *box* is ``(x0, y0, x1, y1)`` in points, bottom-left origin, in either corner order; *pad_pt* widens it on every
    side, and the result is kept within the page. The copy has no annotations (a link or a form field would point
    outside the window) and no trim, bleed or art boxes of its own. Raises :class:`ValueError` for a page that does not
    exist or a box that misses the page, and :class:`pikepdf.PdfError` for a file that will not open.
    """
    with pikepdf.open(path) as source:
        if not 1 <= page <= len(source.pages):
            raise ValueError(f"{path.name} has {len(source.pages)} page(s); there is no page {page}")
        original = source.pages[page - 1]
        media = [float(v) for v in original.mediabox]
        x0, y0 = max(min(box[0], box[2]) - pad_pt, media[0]), max(min(box[1], box[3]) - pad_pt, media[1])
        x1, y1 = min(max(box[0], box[2]) + pad_pt, media[2]), min(max(box[1], box[3]) + pad_pt, media[3])
        if x1 <= x0 or y1 <= y0:
            raise ValueError(
                f"the box {tuple(box)} lies outside page {page}, which spans "
                f"({media[0]:g}, {media[1]:g}) to ({media[2]:g}, {media[3]:g}) points"
            )
        piece = pikepdf.new()
        piece.pages.append(original)  # inherited boxes and resources are copied onto the page
        cut = piece.pages[0]
        for key in ("/TrimBox", "/BleedBox", "/ArtBox", "/Annots", "/StructParents"):
            if key in cut.obj:
                del cut.obj[key]
        window = pikepdf.Array([x0, y0, x1, y1])
        cut.obj.MediaBox = window
        cut.obj.CropBox = window
        try:
            cut.remove_unreferenced_resources()
        except pikepdf.PdfError:  # a content stream it cannot read: keep every resource rather than lose one
            pass
        out.parent.mkdir(parents=True, exist_ok=True)
        piece.save(out)
    return out
