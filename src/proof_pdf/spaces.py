"""PDF colour spaces, reduced to the family a check cares about.

Shared by the colour checks and the paint walk. Named spaces are resolved through the resource dictionary and
``/Indexed`` is looked through to its base, but the walk deliberately stops at ``/Separation`` and ``/DeviceN``: their
alternate space is a rendering hint for a spot ink, and treating it as the artwork's own space produces false positives
on entirely correct print files.
"""

import pikepdf

RGB = "rgb"
CMYK = "cmyk"
GRAY = "gray"
OTHER = "other"

_DEVICE_FAMILIES = {
    "/DeviceRGB": RGB,
    "/CalRGB": RGB,
    "/Lab": OTHER,
    "/DeviceCMYK": CMYK,
    "/DeviceGray": GRAY,
    "/CalGray": GRAY,
    "/Pattern": OTHER,
}
_SPOT_FAMILIES = ("/Separation", "/DeviceN")
_ICC_COMPONENTS = {1: GRAY, 3: RGB, 4: CMYK}
# The profile header's data colour space (bytes 16-20). /N alone cannot tell an RGB profile from a Lab one.
_ICC_SIGNATURES = {b"GRAY": GRAY, b"RGB ": RGB, b"CMYK": CMYK}


def lookup_resource(resources: pikepdf.Object | None, category: str, operands: list) -> pikepdf.Object | None:
    """Resolve a name operand against one category of a resource dictionary."""
    if resources is None or not operands:
        return None
    table = resources.get(category)
    if table is None:
        return None
    try:
        return table.get(str(operands[0]))
    except KeyError, TypeError:
        return None


def truthy(value: object) -> bool:
    """A PDF boolean object read as a plain bool."""
    return value is not None and str(value) == "True"


def classify_colour_space(space: object, resources: pikepdf.Object | None, *, depth: int = 0) -> str:
    """Which colour family a PDF colour space object belongs to."""
    if space is None or depth > 4:
        return OTHER
    if isinstance(space, pikepdf.Array) or (hasattr(space, "__len__") and not isinstance(space, pikepdf.Name)):
        return _classify_array(space, resources, depth)
    name = str(space)
    if name in _DEVICE_FAMILIES:
        return _DEVICE_FAMILIES[name]
    named = lookup_resource(resources, "/ColorSpace", [name])
    if named is not None:
        return classify_colour_space(named, resources, depth=depth + 1)
    return OTHER


def _classify_array(space, resources: pikepdf.Object | None, depth: int) -> str:
    try:
        head = str(space[0])
    except IndexError, TypeError:
        return OTHER
    if head in _SPOT_FAMILIES:
        return OTHER
    if head == "/ICCBased":
        return _classify_icc(space)
    if head == "/Indexed":
        base = space[1] if len(space) > 1 else None
        return classify_colour_space(base, resources, depth=depth + 1)
    if head in _DEVICE_FAMILIES:
        return _DEVICE_FAMILIES[head]
    return OTHER


def _classify_icc(space) -> str:
    """An ``[/ICCBased stream]`` space, by the profile's own colour space when readable, else by the stream's ``/N``."""
    try:
        components = int(space[1].get("/N"))
    except IndexError, TypeError, ValueError, AttributeError:
        return OTHER
    try:
        signature = bytes(space[1].read_bytes())[16:20]
    except pikepdf.PdfError:  # unreadable profile: /N is all there is
        return _ICC_COMPONENTS.get(components, OTHER)
    return _ICC_SIGNATURES.get(signature, OTHER) if len(signature) == 4 else _ICC_COMPONENTS.get(components, OTHER)


def spot_names(space: object, resources: pikepdf.Object | None, *, depth: int = 0) -> list[str]:
    """The ink names a ``/Separation`` or ``/DeviceN`` space paints with; empty for any other space."""
    if space is None or depth > 4:
        return []
    if not isinstance(space, pikepdf.Array):
        name = str(space)
        named = lookup_resource(resources, "/ColorSpace", [name]) if name.startswith("/") else None
        return spot_names(named, resources, depth=depth + 1) if named is not None else []
    try:
        head = str(space[0])
        if head == "/Separation":
            return [str(space[1]).lstrip("/")]
        if head == "/DeviceN":
            return [str(n).lstrip("/") for n in space[1]]
        if head == "/Indexed":
            return spot_names(space[1], resources, depth=depth + 1)
    except IndexError, TypeError:
        return []
    return []


def intent_family(pdf: pikepdf.Pdf) -> str | None:
    """The colour family of the PDF's first output-intent profile (by its ``/N``), or ``None`` without one."""
    for intent in pdf.Root.get("/OutputIntents", []):
        profile = intent.get("/DestOutputProfile")
        if profile is not None and "/N" in profile:
            return _ICC_COMPONENTS.get(int(profile["/N"]))
    return None
