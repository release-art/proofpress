"""One content-stream walk, shared by every check that reads page content.

Reading only a page's *own* content stream and its page-level
``/Resources`` is a large blind spot in a LaTeX build: ``tikz``,
``tcolorbox`` and ``adjustbox`` put most of what they draw inside Form
XObjects, so overprint settings, small text and placed images nested one
level down would be invisible.

:func:`iter_operations` covers that once, for all of the checks: it yields
every operator on a page together with the live CTM and the resource dictionary
in scope, descending into Form XObjects with their ``/Matrix`` concatenated onto
the CTM. Everything downstream (overprint, rich black, image DPI, colour space)
is then a filter over one stream of events.
"""

import collections.abc
import dataclasses
import logging
import math
import typing

import pikepdf

logger = logging.getLogger(__name__)

# How deep to follow nested Form XObjects. Real artwork nests two or three deep;
# the cap is a backstop against a pathological or hostile file, alongside the
# objgen cycle guard.
DEFAULT_MAX_DEPTH = 12

# A 2-D affine matrix as PDF stores it: (a, b, c, d, e, f).
Matrix = tuple[float, float, float, float, float, float]
IDENTITY: Matrix = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


@dataclasses.dataclass(frozen=True)
class Operation:
    """One content-stream operator, with the state in force when it runs.

    *resources* is the dictionary that names in *operands* resolve against --
    the page's for top-level content, the form's for nested content -- so a
    consumer never has to track scope itself.
    """

    operator: str
    operands: list[typing.Any]
    ctm: Matrix
    resources: pikepdf.Object | None
    depth: int


def matrix(operands: collections.abc.Iterable[typing.Any]) -> Matrix:
    """Six numeric operands as a :data:`Matrix`; identity if malformed."""
    try:
        a, b, c, d, e, f = (float(v) for v in operands)
    except TypeError, ValueError:
        return IDENTITY
    return (a, b, c, d, e, f)


def mul(m1: Matrix, m2: Matrix) -> Matrix:
    """The matrix product ``m1 x m2`` in PDF's row-vector convention."""
    a1, b1, c1, d1, e1, f1 = m1
    a2, b2, c2, d2, e2, f2 = m2
    return (
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    )


def vertical_scale(m: Matrix) -> float:
    """The factor a unit of height is scaled by under *m*."""
    _a, _b, c, d, _e, _f = m
    return math.hypot(c, d)


def horizontal_scale(m: Matrix) -> float:
    """The factor a unit of width is scaled by under *m*."""
    a, b, _c, _d, _e, _f = m
    return math.hypot(a, b)


def iter_operations(
    page: pikepdf.Page,
    *,
    max_depth: int = DEFAULT_MAX_DEPTH,
) -> collections.abc.Iterator[Operation]:
    """Yield every operator on *page*, descending into Form XObjects.

    The CTM carried by each :class:`Operation` reflects the ``q``/``Q``/``cm``
    state at that point, with each enclosing form's ``/Matrix`` already folded
    in -- so a consumer can convert directly from user space to page points
    without knowing how deeply nested it is.
    """
    resources = page.obj.get("/Resources")
    yield from _walk(page, resources, IDENTITY, 0, frozenset(), max_depth)


def _walk(
    source: pikepdf.Page | pikepdf.Object,
    resources: pikepdf.Object | None,
    base_ctm: Matrix,
    depth: int,
    seen: frozenset[tuple[int, int]],
    max_depth: int,
) -> collections.abc.Iterator[Operation]:
    ctm = base_ctm
    stack: list[Matrix] = []
    for instruction in pikepdf.parse_content_stream(source):
        op = str(instruction.operator)
        operands = list(instruction.operands)
        if op == "q":
            stack.append(ctm)
        elif op == "Q":
            # An unbalanced Q restores to where this stream started, which is
            # what a viewer does too -- a malformed stream must not desync the
            # CTM for everything after it.
            ctm = stack.pop() if stack else base_ctm
        elif op == "cm":
            ctm = mul(matrix(operands), ctm)
        yield Operation(op, operands, ctm, resources, depth)
        if op == "Do":
            yield from _descend(operands, resources, ctm, depth, seen, max_depth)


def _descend(
    operands: list[typing.Any],
    resources: pikepdf.Object | None,
    ctm: Matrix,
    depth: int,
    seen: frozenset[tuple[int, int]],
    max_depth: int,
) -> collections.abc.Iterator[Operation]:
    """Walk into the Form XObject a ``Do`` names, if it is one and is safe to enter."""
    if depth >= max_depth:
        logger.debug(f"content: not descending past depth {max_depth}")
        return
    form = lookup_xobject(resources, operands)
    if form is None or str(form.get("/Subtype")) != "/Form":
        return
    # Guard the recursion by object identity, not by the Python wrapper: pikepdf
    # hands back a fresh wrapper per lookup, so id() never repeats and could
    # never catch a form that (directly or transitively) draws itself.
    key = form.objgen
    indirect = key != (0, 0)
    if indirect and key in seen:
        logger.debug(f"content: skipping self-referential form {key}")
        return
    form_ctm = mul(matrix(form.get("/Matrix") or IDENTITY), ctm)
    # A form without its own /Resources inherits the enclosing scope.
    form_resources = form.get("/Resources") or resources
    yield from _walk(
        form,
        form_resources,
        form_ctm,
        depth + 1,
        seen | {key} if indirect else seen,
        max_depth,
    )


def lookup_xobject(resources: pikepdf.Object | None, operands: list[typing.Any]) -> pikepdf.Object | None:
    """The XObject a ``Do``'s name operand resolves to in *resources*, if any."""
    if resources is None or not operands:
        return None
    xobjects = resources.get("/XObject")
    if xobjects is None:
        return None
    name = str(operands[0])
    try:
        return xobjects.get(name)
    except KeyError, TypeError:  # a name that is not in the dict, or not a dict
        return None
