"""Vector artwork recovered as standalone one-page PDFs: logos, pictograms, rules, any drawn shape.

A PDF has no notion of "this is the vest icon": the artwork is a flat run of path operators. This walks a page, keeps
every painted path with the graphics state it was painted in, and groups paths whose bounding boxes touch (within a
gap). Each group is rewritten as its own page, cropped to the group, with only its own paths, so ``\\includegraphics``
takes it. The same artwork placed twice, or on several pages, renders to identical bytes, so the caller can dedupe.

Text over the artwork (dimensions, pin names, captions) is shown again inside the piece, with its original font, and
takes part in the grouping; it is also still recovered as text. Known ceilings: clips are not applied (a clipped shape
comes out whole); placed images are not part of a group (``extract`` recovers them separately); a shape bigger than a
fraction of the page, usually a background, is dropped so it cannot glue everything to it; grouping is by touching
boxes, so a pictogram drawn in separated pieces comes out as several files.
"""

import bisect
import collections
import dataclasses
import math
import typing

Rect = tuple[float, float, float, float]

Matrix = tuple[float, float, float, float, float, float]

_POINT_COUNT = {"m": 1, "l": 1, "c": 3, "v": 2, "y": 2}
_STROKES = frozenset(("S", "s", "B", "B*", "b", "b*"))
INVISIBLE = 3  # text render mode 3: an OCR layer, not something to draw
# A plain rule longer than this share of the page's longer side is layout (a table rule, a divider), never artwork;
# a cluster is artwork when at least ARTWORK_SHARE of its shapes are not plain rectangles or lines.
LONG_RULE_FRACTION = 0.25
ARTWORK_SHARE = 0.25
# A group with at least this share of its area inside a bigger group is part of it.
ABSORB_SHARE = 0.5
# A label reaches LABEL_REACH_EM of its height beyond its box (dimension text sits a little off its leader); shapes
# up to GLYPH_EM of its height standing within that reach are glyphs of the label, and extend it. A drawing may
# cover GROUP_FRACTION of the page; the per-shape cap for backgrounds is the caller's max_fraction.
LABEL_REACH_EM = 0.3
GLYPH_EM = 1.2
GROUP_FRACTION = 0.9
COMPOUND_SUBPATHS = 3  # a path of at least this many outlines is content, however big its box
# Short text within NEAR_EM of its own height from a drawing of at least MIN_DRAWING_SHAPES shapes is its label.
NEAR_EM = 1.5
MAX_LABEL_CHARS = 32  # of a whole line: pin names and dimensions are short, a paragraph's lines are not
MIN_DRAWING_SHAPES = 5
MIN_DRAWING_LABELS = 3
GATHER_PASSES = 6
# Text is running text when FLOWING_LINES lines of one size share a left edge (within STACK_EDGE_EM) and sit within
# STACK_PITCH_EM of each other; fragments of a line closer than ROW_GAP_EM are one line.
FLOWING_LINES = 3
STACK_EDGE_EM = 0.25
STACK_PITCH_EM = 2.2
ROW_GAP_EM = 0.6


@dataclasses.dataclass(frozen=True)
class Shape:
    """A painted path as the Rust core read it (``_core.extract``'s ``shapes``)."""

    path: tuple[tuple[str, tuple[float, ...]], ...]  # (operator, operands), the paint operator last
    ctm: Matrix
    bbox: Rect
    index: int = 0  # position in paint order on the page
    mcid: int | None = None  # the marked-content id it was painted under, for the structure tree
    width: float = 1.0  # the line width in force
    fill_ops: str = "g 0"  # the fill colour operators in force, as written (a spot colour by name)
    stroke_ops: str = "g 0"

    @property
    def subpaths(self) -> int:
        """How many separate outlines the path holds: a compound path (all a drawing's hairlines in one) has many."""
        return sum(1 for name, _ in self.path if name in ("m", "re"))

    @property
    def paint(self) -> str:
        return self.path[-1][0]

    @property
    def plain(self) -> bool:
        """A bare rectangle or straight line: a panel, background or rule, which is not artwork and glues nothing."""
        ops = [name for name, _ in self.path[:-1]]
        return bool(ops) and (set(ops) == {"re"} or ops in (["m", "l"], ["m", "l", "h"]))

    @property
    def endpoints(self) -> tuple[tuple[float, float], tuple[float, float]] | None:
        """A straight line's two ends in page space (None for a rectangle)."""
        if self.path[0][0] != "m" or len(self.path) < 3:
            return None
        a, b = _points("m", list(self.path[0][1])), _points("l", list(self.path[1][1]))
        if not a or not b:
            return None
        m = self.ctm
        return tuple((m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) for x, y in (a[0], b[0]))  # type: ignore[return-value]

    @property
    def fill(self) -> str | None:
        return self.fill_ops if self.paint not in ("S", "s") else None

    @property
    def stroke(self) -> str | None:
        return self.stroke_ops if self.paint in _STROKES else None

    @property
    def pen(self) -> tuple[float, str] | None:
        """The stroke's effective width and colour, kept the same across a drawing's lines; None if unstroked."""
        return (round(self.stroke_width or 0.0, 2), self.stroke or "") if self.stroke else None

    @property
    def stroke_width(self) -> float | None:
        return self.width * abs(self.ctm[0] * self.ctm[3] - self.ctm[1] * self.ctm[2]) ** 0.5 if self.stroke else None


@dataclasses.dataclass(frozen=True)
class Label:
    """A run of text overlapping artwork: a dimension, a pin name, a caption. It is shown again inside the piece."""

    bbox: Rect
    size: float  # the font size Tf set
    paint: int  # its index among the page walk's paints (``_core.extract``'s ``paints``), to show it again
    text: str | None
    reach: float = 0.0  # how far from its box it joins things; 0 means the grouping gap


@dataclasses.dataclass(frozen=True)
class Group:
    bbox: Rect  # page space, points, y up
    shapes: tuple[Shape, ...]
    labels: tuple[Label, ...] = ()

    @property
    def mcids(self) -> set[int]:
        return {s.mcid for s in self.shapes if s.mcid is not None}

    @property
    def text(self) -> list[str]:
        """The readable text the piece carries, in the order it was shown."""
        return [label.text for label in self.labels if label.text and label.text.strip()]

    def request(self, page: int) -> dict[str, typing.Any]:
        """What the core needs to write this piece alone on a page cropped to it (``_core.vector_pieces``)."""
        return {
            "page": page,
            "bbox": list(self.bbox),
            "shapes": [s.index for s in self.shapes],
            "labels": [label.paint for label in self.labels],
        }


def artwork(
    media: typing.Sequence[float] | None,
    everything: list[Shape],
    *,
    gap_pt: float,
    max_fraction: float,
    labels: typing.Sequence[Label] = (),
) -> tuple[list[Group], list[Shape]]:
    """``(groups, boxes)``: the artwork on *page* as groups of touching shapes, and the plain rectangles and lines
    that are not part of any.

    Short plain shapes (wires, box outlines) take part in the clustering, so a diagram joined by lines is one piece.
    So does text (*texts*, the page's text runs): a label touching the drawing is part of it, and it bridges what it
    touches, but two labels never join each other, or a page of paragraphs would be one cluster. A cluster is artwork
    when at least ARTWORK_SHARE of its shapes are not plain; one of mostly rules is a table or a ruled layout, which
    would otherwise glue every pictogram in it together, so there only the non-plain shapes group. Long rules and
    backgrounds never join anything. What is left over are the panels and rules of the layout, which a caller may want
    as data. Text mostly inside a piece but not touching it is added afterwards.
    """
    media = media or (0.0, 0.0, 612.0, 792.0)
    width, height = abs(float(media[2]) - float(media[0])), abs(float(media[3]) - float(media[1]))
    limit = max_fraction * width * height  # for one shape: bigger than this is a background
    group_limit = GROUP_FRACTION * width * height  # for a whole drawing, which may fill most of a page
    longest_rule = LONG_RULE_FRACTION * max(width, height)
    labels = list(labels)
    candidates: list[Shape | Label] = [
        s
        for s in everything
        if (
            _area(s.bbox) <= limit if not s.plain else max(s.bbox[2] - s.bbox[0], s.bbox[3] - s.bbox[1]) <= longest_rule
        )
    ]
    # Too big to be told from a background by its box, yet holding many outlines: a drawing's hairlines or
    # arrowheads drawn as one path. They cannot cluster by their boxes, so a drawing adopts them afterwards.
    sparse = [s for s in everything if not s.plain and _area(s.bbox) > limit and s.subpaths >= COMPOUND_SUBPATHS]
    labels = _reached(labels, candidates, gap_pt)
    groups: list[Group] = []
    used: set[int] = set()
    for cluster in _clusters([*candidates, *labels], gap_pt):
        parts = [n for n in cluster if isinstance(n, Shape)]
        art = [s for s in parts if not s.plain]
        if not art:
            continue
        box = _union([n.bbox for n in cluster])
        if (
            _artlike(parts) >= ARTWORK_SHARE * len(parts)
            and 0 < _area(box) <= group_limit
            and box[2] > box[0]
            and box[3] > box[1]
        ):
            carried = tuple(n for n in cluster if isinstance(n, Label))
            groups.append(Group(box, tuple(parts), carried))
            used |= {id(n) for n in cluster if isinstance(n, Label) or n.plain}
        else:
            groups += _group(art, gap_pt, group_limit)
    boxes = [s for s in everything if s.plain and id(s) not in used]
    groups = _absorb(_adopt(sparse, groups))
    groups = _attach([label for label in labels if id(label) not in used], groups)
    groups = _gather([label for label in labels if id(label) not in {id(x) for g in groups for x in g.labels}], groups)
    return sorted(groups, key=lambda g: (-g.bbox[3], g.bbox[0])), boxes  # reading order: top to bottom, then left


def _artlike(parts: list[Shape]) -> int:
    """How many of *parts* belong to a drawing: the shapes that are not plain, and the plain lines drawn with the same
    pen (stroke width and colour) as one of them. Wires joining a schematic's parts match, a table's rules do not."""
    pens = {s.pen for s in parts if not s.plain and s.pen}
    return sum(1 for s in parts if not s.plain or (s.pen is not None and s.pen in pens))


def _adopt(sparse: list[Shape], groups: list[Group]) -> list[Group]:
    """Add each compound path too big to cluster by its box to the smallest drawing it lies mostly inside."""
    extra: dict[int, list[Shape]] = collections.defaultdict(list)
    for shape in sparse:
        hosts = [i for i, g in enumerate(groups) if _mostly_inside(shape.bbox, g.bbox)]
        if hosts:
            extra[min(hosts, key=lambda i: _area(groups[i].bbox))].append(shape)
    return [
        Group(g.bbox, tuple(sorted((*g.shapes, *extra[i]), key=lambda s: s.index)), g.labels) if extra[i] else g
        for i, g in enumerate(groups)
    ]


def _gather(labels: list[Label], groups: list[Group]) -> list[Group]:
    """Short text standing within about a line of a drawing is its label: pin names, titles, dimensions.

    Repeats, each label that joins widening the drawing's reach, so a three-line title is drawn in line by line. Only
    drawings (not icons), only short text (a paragraph's lines are long), and each label goes to the nearest drawing.
    """
    boxes = [g.bbox for g in groups]
    extra: dict[int, list[Label]] = collections.defaultdict(list)
    flowing = _flowing(labels)
    pending = [label for label in labels if id(label) not in flowing]
    for _ in range(GATHER_PASSES):
        joined = False
        for label in list(pending):
            reach = NEAR_EM * (label.bbox[3] - label.bbox[1])
            near = [
                (_distance(label.bbox, boxes[i]), i)
                for i, g in enumerate(groups)
                if _is_drawing(g) and _distance(label.bbox, boxes[i]) <= reach
            ]
            if near:
                _, i = min(near)
                extra[i].append(label)
                boxes[i] = _union([boxes[i], label.bbox])
                pending.remove(label)
                joined = True
        if not joined:
            break
    return [Group(boxes[i], g.shapes, (*g.labels, *extra[i])) if extra[i] else g for i, g in enumerate(groups)]


def _flowing(labels: list[Label]) -> set[int]:
    """The labels that are running text rather than a drawing's: those on a long line, or in a stack of three or more
    lines, left-aligned at a line-like pitch.

    Fragments on one baseline are first joined into lines. Pin names, dimension texts and centred titles are short and
    not stacked this way, so they are not caught; a paragraph beside a figure is, and so stays text.
    """
    rows: dict[tuple[float, float], list[Label]] = collections.defaultdict(list)
    for label in labels:
        rows[(round(label.bbox[1] * 2) / 2, round(label.size, 1))].append(label)
    lines: list[tuple[Rect, list[Label]]] = []
    for members in rows.values():
        current: list[Label] = []
        for label in sorted(members, key=lambda m: m.bbox[0]):
            height = label.bbox[3] - label.bbox[1]
            if current and label.bbox[0] - current[-1].bbox[2] > ROW_GAP_EM * height:
                lines.append((_union([m.bbox for m in current]), current))
                current = []
            current.append(label)
        lines.append((_union([m.bbox for m in current]), current))
    flowing: set[int] = set()
    for _, members in lines:
        if sum(len(m.text or "") for m in members) + len(members) - 1 > MAX_LABEL_CHARS:
            flowing |= {id(m) for m in members}
    chain: list[tuple[Rect, list[Label]]] = []
    for line in sorted(lines, key=lambda ln: (round(ln[1][0].size, 1), round(ln[0][0]), -ln[0][1])):
        if chain and not _stacked(chain[-1][0], line[0], chain[-1][1][0].size, line[1][0].size):
            chain = []
        chain.append(line)
        if len(chain) >= FLOWING_LINES:
            flowing |= {id(label) for _, members in chain for label in members}
    return flowing


def _stacked(above: Rect, below: Rect, size_a: float, size_b: float) -> bool:
    height = above[3] - above[1]
    pitch = above[1] - below[1]
    return (
        abs(size_a - size_b) < 0.1
        and abs(above[0] - below[0]) <= STACK_EDGE_EM * height
        and 0 < pitch <= STACK_PITCH_EM * height
    )


def _is_drawing(group: Group) -> bool:
    """A piece of artwork with real content, rather than an icon: many shapes, or already carrying several labels."""
    return len(group.shapes) >= MIN_DRAWING_SHAPES or len(group.labels) >= MIN_DRAWING_LABELS


def _distance(a: Rect, b: Rect) -> float:
    """The gap between two boxes (0 when they touch or overlap)."""
    return math.hypot(max(a[0] - b[2], b[0] - a[2], 0.0), max(a[1] - b[3], b[1] - a[3], 0.0))


def _attach(labels: list[Label], groups: list[Group]) -> list[Group]:
    """Give each label that sits mostly inside a piece, without touching it, to the smallest such piece."""
    extra: dict[int, list[Label]] = collections.defaultdict(list)
    for label in labels:
        hosts = [i for i, g in enumerate(groups) if _mostly_inside(label.bbox, g.bbox)]
        if hosts:
            extra[min(hosts, key=lambda i: _area(groups[i].bbox))].append(label)
    return [
        Group(_union([g.bbox, *[label.bbox for label in extra[i]]]), g.shapes, (*g.labels, *extra[i]))
        if extra[i]
        else g
        for i, g in enumerate(groups)
    ]


def _absorb(groups: list[Group]) -> list[Group]:
    """Fold each group lying mostly inside a bigger one into it: a label inside a diagram's outline is part of it."""
    kept: list[Group] = []
    for group in sorted(groups, key=lambda g: -_area(g.bbox)):
        host = next((k for k in kept if _mostly_inside(group.bbox, k.bbox) and len(k.shapes) > len(group.shapes)), None)
        if host is None:
            kept.append(group)
        else:
            parts = sorted((*host.shapes, *group.shapes), key=lambda shape: shape.index)
            kept[kept.index(host)] = Group(_union([host.bbox, group.bbox]), tuple(parts), (*host.labels, *group.labels))
    return kept


def _mostly_inside(inner: Rect, outer: Rect) -> bool:
    shared = _area((max(inner[0], outer[0]), max(inner[1], outer[1]), min(inner[2], outer[2]), min(inner[3], outer[3])))
    return shared >= ABSORB_SHARE * _area(inner) > 0


def _group(shapes: list[Shape], gap_pt: float, limit: float) -> list[Group]:
    result = []
    for parts in _clusters(shapes, gap_pt):
        box = _union([s.bbox for s in parts])
        if 0 < _area(box) <= limit and box[2] - box[0] > 0 and box[3] - box[1] > 0:
            result.append(Group(box, tuple(p for p in parts if isinstance(p, Shape))))
    return result


def _reached(labels: list[Label], shapes: list[Shape | Label], gap_pt: float) -> list[Label]:
    """Give each label its reach, and let the glyph-like shapes standing next to it (a diameter sign, a degree mark
    drawn as a path) extend its box, so that the label's reach starts from the whole of what it says."""
    order = sorted((s for s in shapes if isinstance(s, Shape)), key=lambda s: s.bbox[0])
    starts = [s.bbox[0] for s in order]
    result = []
    for label in labels:
        height = label.bbox[3] - label.bbox[1]
        reach = max(gap_pt, LABEL_REACH_EM * height)
        box = label.bbox
        first = bisect.bisect_left(starts, box[0] - reach - GLYPH_EM * height)
        for shape in order[first : bisect.bisect_right(starts, box[2] + reach)]:
            size = max(shape.bbox[2] - shape.bbox[0], shape.bbox[3] - shape.bbox[1])
            if size <= GLYPH_EM * height and _touch(label.bbox, shape.bbox, reach):
                box = _union([box, shape.bbox])
        result.append(dataclasses.replace(label, bbox=box, reach=max(gap_pt, LABEL_REACH_EM * (box[3] - box[1]))))
    return result


def _clusters(nodes: list, gap_pt: float) -> list[list]:
    """Nodes (shapes and labels) whose boxes touch, transitively; two labels never join each other.

    Nodes touch within the grouping gap, and a label within its own, larger, reach.
    """
    reach = [getattr(n, "reach", 0.0) or gap_pt for n in nodes]
    widest = max(reach, default=gap_pt)
    # Sweep by left edge: a node can only touch nodes that start before its right edge plus the widest reach.
    order = sorted(range(len(nodes)), key=lambda i: nodes[i].bbox[0])
    parent = list(range(len(nodes)))

    def find(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for a, i in enumerate(order):
        for j in order[a + 1 :]:
            if nodes[j].bbox[0] > nodes[i].bbox[2] + widest:
                break
            if isinstance(nodes[i], Label) and isinstance(nodes[j], Label):
                continue
            if _touch(nodes[i].bbox, nodes[j].bbox, max(reach[i], reach[j])):
                parent[find(j)] = find(i)
    members: dict[int, list[int]] = {}
    for i in range(len(nodes)):
        members.setdefault(find(i), []).append(i)
    return [[nodes[i] for i in sorted(indexes)] for indexes in members.values()]


def _area(r: Rect) -> float:
    return max(r[2] - r[0], 0.0) * max(r[3] - r[1], 0.0)


def _touch(a: Rect, b: Rect, gap: float) -> bool:
    return a[0] <= b[2] + gap and b[0] <= a[2] + gap and a[1] <= b[3] + gap and b[1] <= a[3] + gap


def _union(rects: list[Rect]) -> Rect:
    return (min(r[0] for r in rects), min(r[1] for r in rects), max(r[2] for r in rects), max(r[3] for r in rects))


def _points(name: str, values: list[float]) -> list[tuple[float, float]]:
    count = _POINT_COUNT.get(name, 0)
    return [(values[2 * i], values[2 * i + 1]) for i in range(count) if 2 * i + 1 < len(values)]
