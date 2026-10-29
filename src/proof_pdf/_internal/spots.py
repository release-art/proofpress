"""Spot colours: the named inks a PDF paints with, and what they stand in for.

``/Separation`` (one ink) and ``/DeviceN`` (several) spaces say which inks are used (PANTONE 165 C, a die-line, a
varnish) and carry a tint transform to an alternate space, which gives the ink's look on screen and in proofs. For a
print job the name is the fact and the alternate is a hint: only the printer's ink book says what PANTONE 165 C is.
"""

import dataclasses
import typing

import pikepdf

from . import functions
from .spaces import CMYK, GRAY, RGB, classify_colour_space

_ALTERNATE = {GRAY: "DeviceGray", RGB: "DeviceRGB", CMYK: "DeviceCMYK"}


@dataclasses.dataclass(frozen=True)
class SpotColour:
    names: tuple[str, ...]  # one for a Separation, several for DeviceN
    alternate_space: str | None  # "DeviceCMYK", "DeviceRGB", "DeviceGray", "Lab", "ICCBased(4)"...
    alternate: list[float] | None  # the alternate's components at full tint; None when the transform is not understood


def find(objects: typing.Iterable[pikepdf.Object]) -> list[SpotColour]:
    """Every distinct Separation and DeviceN space among *objects*, in the order met."""
    found: dict[tuple, SpotColour] = {}
    for obj in objects:
        if isinstance(obj, pikepdf.Array) and len(obj) >= 4 and str(obj[0]) in ("/Separation", "/DeviceN"):
            try:
                spot = _read(obj)
            except IndexError, KeyError, TypeError, ValueError, pikepdf.PdfError:
                continue
            found.setdefault((spot.names, spot.alternate_space), spot)
    return list(found.values())


def _read(space: pikepdf.Array) -> SpotColour:
    separation = str(space[0]) == "/Separation"
    names = (str(space[1]).lstrip("/"),) if separation else tuple(str(n).lstrip("/") for n in space[1])
    alternate = space[2]
    family = classify_colour_space(alternate, None)
    label = _ALTERNATE.get(family) or _label(alternate)
    values = functions.evaluate(space[3], [1.0] * len(names))
    return SpotColour(names, label, [round(v, 4) for v in values] if values is not None else None)


def _label(space: typing.Any) -> str | None:
    if isinstance(space, pikepdf.Name):
        return str(space).lstrip("/")
    if isinstance(space, pikepdf.Array) and len(space):
        head = str(space[0]).lstrip("/")
        return f"{head}({int(space[1].get('/N', 0))})" if head == "ICCBased" else head
    return None
