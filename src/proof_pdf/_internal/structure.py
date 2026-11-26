"""Page boxes as the Python PDF/X layer (:mod:`.pdfx`) and the remaining Python checks read them.

The job's page geometry and page set checks run in the Rust core now (``rust/src/checks/page.rs`` and
``painted.rs``, through :mod:`.ported`).
"""

import pikepdf

# A box that should sit inside another may exceed it by this many points before
# it is a real error -- guards against rounding in the millipoint box values.
_BOX_TOLERANCE = 0.5

Rect = tuple[float, float, float, float]


def _rect(obj: object) -> Rect | None:
    """Normalise a PDF box object to ``(x0, y0, x1, y1)`` with sorted corners."""
    if obj is None:
        return None
    try:
        values = [float(v) for v in obj]  # pyright: ignore[reportGeneralTypeIssues]
    except TypeError, ValueError:
        return None
    if len(values) != 4:
        return None
    x0, y0, x1, y1 = values
    return (min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1))


def _contains(outer: Rect, inner: Rect) -> bool:
    """Whether *inner* sits within *outer*, allowing a sub-point tolerance."""
    return (
        inner[0] >= outer[0] - _BOX_TOLERANCE
        and inner[1] >= outer[1] - _BOX_TOLERANCE
        and inner[2] <= outer[2] + _BOX_TOLERANCE
        and inner[3] <= outer[3] + _BOX_TOLERANCE
    )


def own_box(page: pikepdf.Object, name: str) -> Rect | None:
    """A box defined directly on the page (Trim/Art/Bleed are not inheritable)."""
    return _rect(page.get(f"/{name}"))


def media_box(page: pikepdf.Object) -> Rect | None:
    """The MediaBox, resolving the one inheritable box up the page tree."""
    # Guard the walk by object identity (obj, gen), not id() of the wrapper --
    # pikepdf returns a fresh Python wrapper each `.get("/Parent")`, so id() never
    # repeats and could never catch a cyclic /Parent chain (an infinite loop).
    node: pikepdf.Object | None = page
    seen: set[tuple[int, int]] = set()
    while node is not None and node.objgen not in seen:
        seen.add(node.objgen)
        box = _rect(node.get("/MediaBox"))
        if box is not None:
            return box
        node = node.get("/Parent")
    return None
