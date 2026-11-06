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

import dataclasses
import logging

import pikepdf

from ..models import PT_PER_IN, Finding, Severity, pt_to_mm
from . import content, structure

logger = logging.getLogger(__name__)

# Below this, a placed image is too small to measure meaningfully -- a hairline
# rule or a 1x1 spacer is not artwork, and dividing by its near-zero size
# produces a nonsense dpi.
_MIN_PLACED_PT = 1.0

# Placement matrices are written with a few decimals and computed in floats, so an image prepared for exactly the
# minimum resolution measures a hair under it (299.99999999999994 dpi). A shortfall this small is rounding.
_DPI_ROUNDING = 1e-3


@dataclasses.dataclass(frozen=True)
class _Placement:
    """One placement of an image: its resolution on each axis and where it sits (mm from the page's bottom-left)."""

    dpi_x: float
    dpi_y: float
    at: tuple[float, float]

    @property
    def low(self) -> float:
        return min(self.dpi_x, self.dpi_y)

    @property
    def high(self) -> float:
        return max(self.dpi_x, self.dpi_y)

    @property
    def stretch_pct(self) -> float:
        """How much finer one axis is than the other, as the larger over the smaller less one."""
        return (self.high / self.low - 1) * 100

    def describe(self) -> str:
        """``75 dpi (300 x 75 dpi) at 12.0, 30.5 mm from the page's bottom-left``: both axes only when they differ."""
        both = f" ({self.dpi_x:.0f} x {self.dpi_y:.0f} dpi)" if f"{self.dpi_x:.0f}" != f"{self.dpi_y:.0f}" else ""
        return f"{self.low:.0f} dpi{both} at {self.at[0]:.1f}, {self.at[1]:.1f} mm from the page's bottom-left"


def check_image_dpi(
    pdf: pikepdf.Pdf,
    min_dpi: float,
    *,
    warn_dpi: float | None = None,
    max_dpi: float | None = None,
    max_stretch_pct: float | None = None,
) -> list[Finding]:
    """Flag placed images whose effective resolution is out of bounds.

    Below *min_dpi* is an error; below *warn_dpi* (above the minimum) a warning; above *max_dpi* a warning (it only
    grows the file); an image whose two axes differ by more than *max_stretch_pct* percent (larger over smaller, less
    one) a warning. Each limit is off until given. One finding per image name per page and rule, reporting the worst
    placement of that image (the first, on a tie): a job repeats the same logo on every card, and a finding per
    occurrence would bury everything else.
    """
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        media = structure.media_box(page.obj)
        origin = (media[0], media[1]) if media is not None else (0.0, 0.0)
        placements: dict[str, list[_Placement]] = {}
        for op in content.iter_operations(page):
            if op.operator not in ("Do", "INLINE IMAGE"):
                continue
            if (measured := _effective_dpi(op, origin)) is not None:
                placements.setdefault(measured[0], []).append(measured[1])
        for name, found in sorted(placements.items()):
            findings += _judge(name, found, i, min_dpi, warn_dpi, max_dpi, max_stretch_pct)
    return findings


def _judge(
    name: str,
    found: list[_Placement],
    page: int,
    min_dpi: float,
    warn_dpi: float | None,
    max_dpi: float | None,
    max_stretch_pct: float | None,
) -> list[Finding]:
    findings: list[Finding] = []
    soft = min(found, key=lambda p: p.low)
    if soft.low < min_dpi * (1 - _DPI_ROUNDING):
        findings.append(
            Finding(
                "image-dpi",
                Severity.ERROR,
                f"image {name} is placed at {soft.describe()}, below the {min_dpi:.0f} dpi minimum",
                page,
            )
        )
    elif warn_dpi is not None and soft.low < warn_dpi * (1 - _DPI_ROUNDING):
        findings.append(
            Finding(
                "image-dpi",
                Severity.WARN,
                f"image {name} is placed at {soft.describe()}, below the {warn_dpi:.0f} dpi advisory level",
                page,
            )
        )
    sharp = max(found, key=lambda p: p.high)
    if max_dpi is not None and sharp.high > max_dpi * (1 + _DPI_ROUNDING):
        findings.append(
            Finding(
                "image-dpi-max",
                Severity.WARN,
                f"image {name} is placed at {sharp.high:.0f} dpi at {sharp.at[0]:.1f}, {sharp.at[1]:.1f} mm from the "
                f"page's bottom-left, above the {max_dpi:.0f} dpi limit: it only increases the file size",
                page,
            )
        )
    stretched = max(found, key=lambda p: p.stretch_pct)
    if max_stretch_pct is not None and stretched.stretch_pct > max_stretch_pct:
        findings.append(
            Finding(
                "image-stretch",
                Severity.WARN,
                f"image {name} is stretched {stretched.stretch_pct:.0f}%, past the {max_stretch_pct:g}% limit: "
                f"{stretched.describe()}",
                page,
            )
        )
    return findings


def _effective_dpi(op: content.Operation, origin: tuple[float, float]) -> tuple[str, _Placement] | None:
    """``(name, placement)`` for the image this ``Do`` paints, or ``None`` if not measurable."""
    if op.operator == "INLINE IMAGE":
        xobject, name = op.operands[0].obj, "(inline)"
    else:
        xobject, name = content.lookup_xobject(op.resources, op.operands), str(op.operands[0])
        if xobject is None or str(xobject.get("/Subtype")) != "/Image":
            return None
    pixels = _pixel_size(xobject)
    if pixels is None:
        return None
    width_pt = content.horizontal_scale(op.ctm)
    height_pt = content.vertical_scale(op.ctm)
    if width_pt < _MIN_PLACED_PT or height_pt < _MIN_PLACED_PT:
        return None
    a, b, c, d, e, f = op.ctm
    corners = [(e, f), (a + e, b + f), (c + e, d + f), (a + c + e, b + d + f)]
    at = (pt_to_mm(min(x for x, _ in corners) - origin[0]), pt_to_mm(min(y for _, y in corners) - origin[1]))
    return name, _Placement(pixels[0] / (width_pt / PT_PER_IN), pixels[1] / (height_pt / PT_PER_IN), at)


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
