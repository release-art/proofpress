"""Tables read back from their rules.

A PDF has no tables: only lines, rectangles and text at positions. Two kinds leave enough behind to recover:

* ``grid``: rules both ways. Connected horizontal and vertical rules give the row and column edges, a cell that lacks
  the rule on a side is merged with its neighbour (row and column spans), and text lines are dropped into the cells
  their centres fall in.
* ``ruled``: horizontal rules only, one under (or over) each row, as in a typeset report. Rows come from the rules;
  columns from where the text lines start.

A candidate is kept only if enough of it holds text (a graph's gridlines, a frame, a decorative box are not tables).
Tables with no rules at all, only whitespace, are not found: that would be guessing from alignment alone.
"""

import statistics
import typing

from ..layout import Cell, Line, Table

# Rules within TOUCH of each other are connected; edges within EDGE_TOL are one edge.
TOUCH = 1.5
EDGE_TOL = 1.5
# A candidate is a table when at least MIN_TEXT_CELLS cells hold text and MIN_FILLED of its cells do.
MIN_TEXT_CELLS = 4
MIN_FILLED = 0.4
# Row-ruled tables: rules form one table when they overlap this much sideways and sit within MAX_ROW_PT of each other;
# a column is a left edge that at least two lines start at, within COLUMN_TOL.
RULED_OVERLAP = 0.8
MAX_ROW_PT = 80.0
COLUMN_TOL = 3.0

Rule = tuple[float, float, float, float]  # x, y, width, height of a thin rectangle or line's extent


def detect(lines: typing.Sequence[Line], rules: typing.Sequence[Rule], page: int) -> list[Table]:
    """The tables on one page, from its text *lines* and its thin horizontal and vertical *rules*."""
    tables: list[Table] = []
    leftover: list[Rule] = []  # horizontal rules that are not part of a grid
    for component in _components(rules):
        horizontal = [r for r in component if r[2] > r[3]]
        vertical = [r for r in component if r[3] > r[2]]
        grid = _grid(horizontal, vertical, lines, page, len(tables)) if horizontal and vertical else None
        if grid is not None:
            tables.append(grid)
        else:
            leftover += horizontal
    for group in _row_groups(leftover):
        table = _ruled(group, lines, page, len(tables))
        if table is not None:
            tables.append(table)
    return tables


# ------------------------------------------------------------------------------------------------------ grid


def _grid(
    horizontal: list[Rule], vertical: list[Rule], lines: typing.Sequence[Line], page: int, serial: int
) -> Table | None:
    ys = _edges([r[1] + r[3] / 2 for r in horizontal], descending=True)
    xs = _edges([r[0] + r[2] / 2 for r in vertical])
    if len(ys) < 3 or len(xs) < 3:  # at least two rows and two columns
        return None
    h_by = _by_edge([(r[1] + r[3] / 2, r[0], r[0] + r[2]) for r in horizontal])
    v_by = _by_edge([(r[0] + r[2] / 2, r[1], r[1] + r[3]) for r in vertical])
    spans = _spans(ys, xs, h_by, v_by)
    return _table(f"p{page}-t{serial + 1}", page, "grid", ys, xs, spans, lines)


def _spans(ys: list[float], xs: list[float], h_by: dict, v_by: dict) -> list[tuple[int, int, int, int]]:
    """``(row, col, rowspan, colspan)`` of every cell: neighbours with no rule between them are one cell."""
    rows, cols = len(ys) - 1, len(xs) - 1
    taken: set[tuple[int, int]] = set()
    cells = []
    for r in range(rows):
        for c in range(cols):
            if (r, c) in taken:
                continue
            c2 = c
            while c2 + 1 < cols and not _covers(v_by, xs[c2 + 1], ys[r + 1], ys[r]):
                c2 += 1
            r2 = r
            while r2 + 1 < rows and not _covers(h_by, ys[r2 + 1], xs[c], xs[c2 + 1]):
                r2 += 1
            taken |= {(i, j) for i in range(r, r2 + 1) for j in range(c, c2 + 1)}
            cells.append((r, c, r2 - r + 1, c2 - c + 1))
    return cells


def _by_edge(rules: list[tuple[float, float, float]]) -> dict[float, list[tuple[float, float]]]:
    """Rule extents by position, positions within EDGE_TOL treated as one."""
    grouped: dict[float, list[tuple[float, float]]] = {}
    for position, lo, hi in sorted(rules):
        key = next((k for k in grouped if abs(k - position) <= EDGE_TOL), position)
        grouped.setdefault(key, []).append((lo, hi))
    return grouped


def _covers(by_edge: dict[float, list[tuple[float, float]]], position: float, lo: float, hi: float) -> bool:
    """Whether the rules at *position* together span from *lo* to *hi* (either order)."""
    lo, hi = min(lo, hi), max(lo, hi)
    key = next((k for k in by_edge if abs(k - position) <= EDGE_TOL), None)
    if key is None:
        return False
    reach = lo
    for start, end in sorted(by_edge[key]):
        if start <= reach + EDGE_TOL:
            reach = max(reach, end)
    return reach >= hi - EDGE_TOL


# ----------------------------------------------------------------------------------------------- row-ruled


def _row_groups(rules: list[Rule]) -> list[list[Rule]]:
    """Horizontal rules stacked one under another, about the same width: the rules of one report table."""
    groups: list[list[Rule]] = []
    for rule in sorted(rules, key=lambda r: -(r[1] + r[3] / 2)):
        for group in groups:
            last = group[-1]
            overlap = min(last[0] + last[2], rule[0] + rule[2]) - max(last[0], rule[0])
            gap = (last[1] + last[3] / 2) - (rule[1] + rule[3] / 2)
            if overlap >= RULED_OVERLAP * min(last[2], rule[2]) and gap <= MAX_ROW_PT:
                group.append(rule)
                break
        else:
            groups.append([rule])
    return [g for g in groups if len(g) >= 3]


def _ruled(group: list[Rule], lines: typing.Sequence[Line], page: int, serial: int) -> Table | None:
    ys = _edges([r[1] + r[3] / 2 for r in group], descending=True)
    if len(ys) < 3:  # rules that merge into one or two edges are not rows
        return None
    x0, x1 = min(r[0] for r in group), max(r[0] + r[2] for r in group)
    pitch = statistics.median(a - b for a, b in zip(ys, ys[1:], strict=False))
    inside = [
        ln for ln in lines if x0 - TOUCH <= ln.x + ln.width / 2 <= x1 + TOUCH and ys[-1] <= _mid(ln) <= ys[0] + pitch
    ]
    if any(ys[0] < _mid(ln) <= ys[0] + pitch for ln in inside):  # a header row above the first rule
        ys = [ys[0] + pitch, *ys]
    starts = _starts([ln.x for ln in inside if ys[-1] <= _mid(ln) <= ys[0]])
    if len(starts) < 2:
        return None
    xs = [min(starts[0], x0), *starts[1:], x1]
    spans = [(r, c, 1, 1) for r in range(len(ys) - 1) for c in range(len(xs) - 1)]
    return _table(f"p{page}-t{serial + 1}", page, "ruled", ys, xs, spans, lines)


def _starts(lefts: list[float]) -> list[float]:
    """Left edges at least two lines start at: the columns' starting positions."""
    clusters: list[list[float]] = []
    for x in sorted(lefts):
        if clusters and x - clusters[-1][-1] <= COLUMN_TOL:
            clusters[-1].append(x)
        else:
            clusters.append([x])
    return [statistics.median(c) for c in clusters if len(c) >= 2]


# ----------------------------------------------------------------------------------------------------- shared


def _mid(line: Line) -> float:
    return line.y + line.height / 2


def _edges(positions: list[float], *, descending: bool = False) -> list[float]:
    merged: list[list[float]] = []
    for value in sorted(positions, reverse=descending):
        if merged and abs(merged[-1][-1] - value) <= EDGE_TOL:
            merged[-1].append(value)
        else:
            merged.append([value])
    return [statistics.fmean(group) for group in merged]


def _table(
    ident: str,
    page: int,
    mode: str,
    ys: list[float],
    xs: list[float],
    spans: list[tuple[int, int, int, int]],
    lines: typing.Sequence[Line],
) -> Table | None:
    cells = [_cell(ys, xs, span, lines) for span in spans]
    filled = [c for c in cells if c.text or c.text is None]
    if len(filled) < MIN_TEXT_CELLS or len(filled) < MIN_FILLED * len(cells):
        return None
    return Table(
        ident, page, mode, xs[0], ys[-1], xs[-1] - xs[0], ys[0] - ys[-1], len(ys) - 1, len(xs) - 1, ys, xs, cells
    )


def _cell(ys: list[float], xs: list[float], span: tuple[int, int, int, int], lines: typing.Sequence[Line]) -> Cell:
    r, c, rowspan, colspan = span
    x0, x1, top, bottom = xs[c], xs[c + colspan], ys[r], ys[r + rowspan]
    held = sorted(
        (ln for ln in lines if x0 <= ln.x + ln.width / 2 <= x1 and bottom <= _mid(ln) <= top),
        key=lambda ln: (-ln.y, ln.x),
    )
    text = None if any(ln.text is None for ln in held) else " ".join(ln.text for ln in held if ln.text)
    return Cell(r, c, rowspan, colspan, x0, bottom, x1 - x0, top - bottom, text)


def _components(rules: typing.Sequence[Rule]) -> list[list[Rule]]:
    """Rules that touch, transitively."""
    items = list(rules)
    order = sorted(range(len(items)), key=lambda i: items[i][0])
    parent = list(range(len(items)))

    def find(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for a, i in enumerate(order):
        xi, yi, wi, hi = items[i]
        for j in order[a + 1 :]:
            xj, yj, wj, hj = items[j]
            if xj > xi + wi + TOUCH:
                break
            if yj <= yi + hi + TOUCH and yi <= yj + hj + TOUCH:
                parent[find(j)] = find(i)
    groups: dict[int, list[Rule]] = {}
    for i, rule in enumerate(items):
        groups.setdefault(find(i), []).append(rule)
    return list(groups.values())
