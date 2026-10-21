"""Effective resolution of the artwork as it is actually placed on the page.

The upscale and CMYK stages size each image for its placement, but nothing
there knows what LaTeX then does with the file: a
raster placed larger than it was prepared for is soft on press regardless of how
many pixels it started with. The only place the truth exists is the finished
PDF, where the image's pixel dimensions and the matrix it is painted under are
both known.

A PDF image XObject always fills the unit square, so the CTM in force at its
``Do`` *is* its size on the page. Effective resolution is therefore pixels
divided by placed inches, per axis -- and because :mod:`.content` folds each
enclosing form's matrix into the CTM, this is correct for nested artwork too.
"""

import logging

import pikepdf

from . import content
from .models import PT_PER_IN, Finding, Severity

logger = logging.getLogger(__name__)

# Below this, a placed image is too small to measure meaningfully -- a hairline
# rule or a 1x1 spacer is not artwork, and dividing by its near-zero size
# produces a nonsense dpi.
_MIN_PLACED_PT = 1.0

# Placement matrices are written with a few decimals and computed in floats, so an image prepared for exactly the
# minimum resolution measures a hair under it (299.99999999999994 dpi). A shortfall this small is rounding.
_DPI_ROUNDING = 1e-3


def check_image_dpi(pdf: pikepdf.Pdf, min_dpi: float) -> list[Finding]:
    """Flag placed images whose effective resolution falls below *min_dpi*.

    One finding per image name per page, reporting the worst placement of that
    image: a job repeats the same logo on every card, and a finding per
    occurrence would bury everything else.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        worst: dict[str, float] = {}
        for op in content.iter_operations(page):
            if op.operator != "Do":
                continue
            measured = _effective_dpi(op)
            if measured is None:
                continue
            name, dpi = measured
            if dpi < worst.get(name, float("inf")):
                worst[name] = dpi
        for name, dpi in sorted(worst.items()):
            if dpi < min_dpi * (1 - _DPI_ROUNDING):
                findings.append(
                    Finding(
                        "image-dpi",
                        Severity.ERROR,
                        f"image {name} is placed at {dpi:.0f} dpi, below the {min_dpi:.0f} dpi minimum",
                        i,
                    )
                )
    return findings


def _effective_dpi(op: content.Operation) -> tuple[str, float] | None:
    """``(name, dpi)`` for the image this ``Do`` paints, or ``None`` if not measurable."""
    xobject = content.lookup_xobject(op.resources, op.operands)
    if xobject is None or str(xobject.get("/Subtype")) != "/Image":
        return None
    pixels = _pixel_size(xobject)
    if pixels is None:
        return None
    width_pt = content.horizontal_scale(op.ctm)
    height_pt = content.vertical_scale(op.ctm)
    if width_pt < _MIN_PLACED_PT or height_pt < _MIN_PLACED_PT:
        return None
    dpi_x = pixels[0] / (width_pt / PT_PER_IN)
    dpi_y = pixels[1] / (height_pt / PT_PER_IN)
    return (str(op.operands[0]), min(dpi_x, dpi_y))


def _pixel_size(xobject: pikepdf.Object) -> tuple[int, int] | None:
    raw_width, raw_height = xobject.get("/Width"), xobject.get("/Height")
    if raw_width is None or raw_height is None:
        return None
    try:
        width, height = int(raw_width), int(raw_height)
    except TypeError, ValueError:
        return None
    if width <= 0 or height <= 0:
        return None
    return (width, height)
