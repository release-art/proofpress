"""Page layout read back from where the text sits: type styles, paragraphs, columns, margins, running heads.

A PDF holds glyphs at positions, not paragraphs or columns, so this works backwards from text lines (as ``extract``
recovers them) and answers the questions a template needs: which type styles are used and for what (body, heading
levels, small print), where the text frames are, how many columns there are and how wide, what the margins are, what
repeats from page to page (headers, footers, folios), and the numbers a document class takes: body size and leading,
paragraph indent and spacing, alignment.

Everything is inferred from geometry, so it is evidence, not fact. The thresholds below are named, heuristic and
documented; a font's line pitch is measured, never assumed. Coordinates are PDF points, origin bottom-left. Text in
fonts the PDF cannot decode (``text`` is ``None``) still takes part: its geometry is as good as any other.
"""

import collections
import dataclasses
import re
import statistics
import typing

from .models import pt_to_mm

# Two lines of one style are one paragraph when the next is within LEADING_SLACK of the style's usual line pitch and
# overlaps it by OVERLAP of the narrower line's width; pitches beyond MAX_PITCH_EM are never line spacing.
LEADING_SLACK = 1.2
OVERLAP = 0.3
MIN_PITCH_EM, MAX_PITCH_EM = 0.8, 2.2
# A style is a heading when its size beats the body's by HEADING_RATIO, small print when it is under SMALL_RATIO.
HEADING_RATIO, SMALL_RATIO = 1.15, 0.9
LEVEL_TOLERANCE = 0.04  # heading sizes this close (relative) are one level
# Edges within EDGE_EM of the font size are aligned; a first line this far in is an indent.
EDGE_EM = 0.1
INDENT_EM = 0.3
# A style whose lines are tagged mostly (TAG_SHARE) with one structure tag takes its role from that tag.
TAG_SHARE = 0.6
RUNNING_ARTIFACTS = frozenset(("Header", "Footer", "Pagination"))
# A figure covering more of the page than this is a background, not content.
BACKGROUND_FRACTION = 0.5


@dataclasses.dataclass(frozen=True)
class Line:
    """Text runs of one font, size and colour on one baseline, joined: what a text frame would be filled with."""

    page: int
    text: str | None  # None when the font gives no way to read it
    x: float
    y: float
    width: float
    height: float
    font: str
    size_pt: float
    colour: str
    rotation_deg: float = 0.0  # text direction, counter-clockwise from the page's x axis
    h_scale: float = 1.0  # Tz as a factor
    letter_spacing_em: float = 0.0  # Tc in ems
    tag: str | None = None  # the standard structure tag of a tagged PDF (H2, P, TD...)
    struct: int | None = None  # the paragraph-like structure element it belongs to
    artifact: str | None = None  # page furniture kind when the PDF marks it (Header, Footer...)


@dataclasses.dataclass(frozen=True)
class Figure:
    """A placed image or piece of vector artwork, for working out where the content is."""

    page: int
    kind: str
    file: str
    x: float
    y: float
    width: float
    height: float
    has_text: bool = False  # text was drawn into the piece, so text over it is its labels, not body copy


@dataclasses.dataclass(frozen=True)
class Cell:
    row: int  # from the top, 0-based
    col: int  # from the left, 0-based
    rowspan: int
    colspan: int
    x: float
    y: float
    width: float
    height: float
    text: str | None  # None when the cell's text cannot be read; "" when it is empty


@dataclasses.dataclass(frozen=True)
class Table:
    """A table read from its rules (``grid``: ruled both ways; ``ruled``: ruled between rows, columns by alignment)."""

    id: str
    page: int
    mode: str
    x: float
    y: float
    width: float
    height: float
    rows: int
    cols: int
    row_edges: list[float]  # y of each row boundary, top first
    col_edges: list[float]  # x of each column boundary, left first
    cells: list[Cell]


@dataclasses.dataclass(frozen=True)
class Style:
    id: str
    font: str
    size_pt: float
    colour: str
    role: str  # "body", "heading-1" (largest) onwards, "small" or "other" (same size as body, another font or colour)
    chars: int
    lines: int
    pages: list[int]
    leading_pt: float | None  # the usual distance between baselines of this style
    sample: str | None
    tags: dict[str, int] = dataclasses.field(default_factory=dict)  # structure tags its lines carry, with counts


@dataclasses.dataclass(frozen=True)
class Block:
    """One paragraph or label: consecutive lines of one style. Several blocks make a text frame."""

    id: str
    page: int
    style: str
    role: str
    text: str | None  # the lines joined by spaces, not de-hyphenated
    x: float
    y: float
    width: float
    height: float
    lines: int
    align: str | None  # "left", "justify", "center", "right"; None for a single line
    indent_pt: float  # first line relative to the rest; negative is a hanging indent
    space_before_pt: float | None  # beyond normal line spacing, from the block above in its column
    column: int | None  # index into the page's columns; None when it spans more than one
    running: bool  # repeats on other pages at the same place: a header, footer or folio
    figure: str | None = None  # the vector piece this text is a label of, when it was drawn into one
    table: str | None = None  # the table this text sits in
    tag: str | None = None  # the structure tag most of its lines carry (tagged PDFs)


@dataclasses.dataclass(frozen=True)
class PageLayout:
    page: int
    width_pt: float
    height_pt: float
    body_box: list[float] | None  # x0, y0, x1, y1 of the content, leaving out running text and backgrounds
    margins_pt: dict[str, float] | None
    columns: list[list[float]]  # [x0, x1] of each text column
    blocks: list[Block]
    figures: list[Figure]
    tables: list[Table]


@dataclasses.dataclass(frozen=True)
class Running:
    """Text that repeats from page to page at one place (digits are folded, so folios match)."""

    text: str
    position: str  # "header" or "footer"
    x: float
    y: float
    font: str
    size_pt: float
    pages: list[int]
    folio: bool  # the text differs between pages only in its digits


@dataclasses.dataclass(frozen=True)
class Summary:
    """The numbers a document class takes, each the typical value over the pages that have one."""

    page_size_mm: list[float] | None
    margins_mm: dict[str, float] | None
    columns: int | None
    gutter_pt: float | None
    measure_pt: float | None  # width of a text column
    body_font: str | None
    body_size_pt: float | None
    leading_pt: float | None
    alignment: str | None
    paragraph_indent_pt: float | None
    paragraph_skip_pt: float | None


@dataclasses.dataclass
class Layout:
    summary: Summary
    styles: list[Style]
    running: list[Running]
    pages: list[PageLayout]

    def to_dict(self) -> dict:
        return dataclasses.asdict(self)


def analyse(
    lines: typing.Sequence[Line],
    figures: typing.Sequence[Figure],
    pages: typing.Sequence[dict],
    tables: typing.Sequence[Table] = (),
) -> Layout:
    """The layout of a document from its text *lines*, placed *figures* and *pages* (``number``, ``media_pt``)."""
    styles = _styles(lines)
    style_of = {(s.font, s.size_pt, s.colour): s for s in styles}
    running = _running(lines, pages)
    result = [
        _page(
            page,
            [line for line in lines if line.page == page["number"]],
            [f for f in figures if f.page == page["number"]],
            [t for t in tables if t.page == page["number"]],
            styles,
            style_of,
            running,
        )
        for page in pages
    ]
    return Layout(_summary(result, styles), styles, running, result)


def _page(
    page: dict,
    lines: list[Line],
    figures: list[Figure],
    tables: list[Table],
    styles: list[Style],
    style_of: dict,
    running: list[Running],
) -> PageLayout:
    media = page.get("media_pt") or [0.0, 0.0, page.get("width_pt") or 0.0, page.get("height_pt") or 0.0]
    placed = [f for f in figures if not _background(f, media)]
    blocks = _mark_tables(
        _mark_figures(_mark_running(_blocks(lines, style_of, page["number"]), running), placed), tables
    )
    text = [b for b in blocks if not b.running and not b.figure and not b.table]
    body = [b for b in text if b.role == "body"] or text
    columns = _columns([b for b in body if b.lines > 1] or body)
    blocks = _spacing([dataclasses.replace(b, column=_column_of(b, columns)) for b in blocks], styles)
    # The content box is what sits inside the page: artwork running off an edge is bleed, not content.
    rects = [(b.x, b.y, b.x + b.width, b.y + b.height) for b in blocks if not b.running]
    rects += [(f.x, f.y, f.x + f.width, f.y + f.height) for f in placed]
    box = _union([r for r in rects if _inside(r, media)])
    margins = (
        {"left": box[0] - media[0], "right": media[2] - box[2], "top": media[3] - box[3], "bottom": box[1] - media[1]}
        if box
        else None
    )
    return PageLayout(
        page["number"],
        media[2] - media[0],
        media[3] - media[1],
        list(box) if box else None,
        margins,
        columns,
        blocks,
        placed,
        tables,
    )


def _mark_figures(blocks: list[Block], figures: list[Figure]) -> list[Block]:
    """Flag the blocks whose centre lies on a figure that had text drawn into it: they label it, they are not body."""
    labelled = [f for f in figures if f.has_text]

    def host(block: Block) -> str | None:
        cx, cy = block.x + block.width / 2, block.y + block.height / 2
        return next((f.file for f in labelled if f.x <= cx <= f.x + f.width and f.y <= cy <= f.y + f.height), None)

    return [dataclasses.replace(b, figure=host(b)) for b in blocks]


def _mark_tables(blocks: list[Block], tables: list[Table]) -> list[Block]:
    """Flag the blocks whose centre lies in a table: cell text is not flowing text, so it stays out of the stats."""

    def host(block: Block) -> str | None:
        cx, cy = block.x + block.width / 2, block.y + block.height / 2
        return next((t.id for t in tables if t.x <= cx <= t.x + t.width and t.y <= cy <= t.y + t.height), None)

    return [dataclasses.replace(b, table=host(b)) for b in blocks]


def _inside(rect: tuple[float, float, float, float], media: list[float]) -> bool:
    return rect[0] >= media[0] and rect[1] >= media[1] and rect[2] <= media[2] and rect[3] <= media[3]


# --------------------------------------------------------------------------------------------------------- styles


def _styles(lines: typing.Sequence[Line]) -> list[Style]:
    groups: dict[tuple, list[Line]] = collections.defaultdict(list)
    for line in lines:
        groups[(line.font, round(line.size_pt, 1), line.colour)].append(line)
    if not groups:
        return []
    chars = {key: sum(_chars(line) for line in group) for key, group in groups.items()}
    body_key = max(chars, key=lambda key: chars[key])
    body_size = body_key[1]
    heading_sizes = _levels(sorted({key[1] for key in groups if key[1] > body_size * HEADING_RATIO}, reverse=True))
    styles = []
    for index, (key, group) in enumerate(sorted(groups.items(), key=lambda kv: (-kv[0][1], kv[0][0], kv[0][2])), 1):
        font, size, colour = key
        if key == body_key:
            role = "body"
        elif size in heading_sizes:
            role = f"heading-{heading_sizes[size]}"
        elif size < body_size * SMALL_RATIO:
            role = "small"
        else:
            role = "other"
        tags = collections.Counter(line.tag for line in group if line.tag)
        role = _tag_role(tags, role)
        sample = next((line.text for line in group if line.text), None)
        styles.append(
            Style(
                f"S{index}",
                font,
                size,
                colour,
                role,
                chars[key],
                len(group),
                sorted({line.page for line in group}),
                _leading(group),
                sample[:60] if sample else None,
                dict(tags),
            )
        )
    return styles


def _tag_role(tags: collections.Counter, geometric: str) -> str:
    """The role a style's structure tags give it, when they agree; otherwise the one its geometry gave."""
    if not tags:
        return geometric
    tag, count = tags.most_common(1)[0]
    if count < TAG_SHARE * sum(tags.values()):  # of the lines that are tagged; untagged ones say nothing
        return geometric
    if re.fullmatch(r"H[1-6]", tag):
        return f"heading-{tag[1]}"
    if tag in ("H", "Title", "Caption"):
        return tag.lower()
    return geometric  # P, Span, TD...: a tag that says only 'text' leaves the size and look to decide


def _levels(sizes: list[float]) -> dict[float, int]:
    """Heading level of each size, largest first; sizes within LEVEL_TOLERANCE of a level's first size share it."""
    levels: dict[float, int] = {}
    level, anchor = 0, 0.0
    for size in sizes:
        if not anchor or size < anchor * (1 - LEVEL_TOLERANCE):
            level, anchor = level + 1, size
        levels[size] = level
    return levels


def _chars(line: Line) -> int:
    return len(line.text) if line.text else round(line.width / max(line.size_pt * 0.5, 1.0))


def _leading(group: list[Line]) -> float | None:
    """The usual baseline-to-baseline distance of a style: the most common pitch between vertically adjacent lines."""
    pitches = [round(pitch, 1) for _, _, pitch in _pairs(group)]
    return statistics.mode(pitches) if pitches else None


def _pairs(group: list[Line]) -> typing.Iterator[tuple[Line, Line, float]]:
    """``(above, below, pitch)`` for each line and the nearest line under it that overlaps it horizontally."""
    for page_lines in _by_page(group).values():
        ordered = sorted((line for line in page_lines if abs(line.rotation_deg) < 1), key=lambda line: -line.y)
        for i, above in enumerate(ordered):
            for below in ordered[i + 1 :]:
                pitch = above.y - below.y
                if pitch > MAX_PITCH_EM * above.size_pt:
                    break
                if (
                    pitch >= MIN_PITCH_EM * above.size_pt
                    and _overlap(above, below) >= OVERLAP
                    and (above.struct is None or below.struct is None or above.struct == below.struct)
                ):
                    yield above, below, pitch
                    break


def _by_page(lines: typing.Iterable[Line]) -> dict[int, list[Line]]:
    pages: dict[int, list[Line]] = collections.defaultdict(list)
    for line in lines:
        pages[line.page].append(line)
    return pages


def _overlap(a: Line, b: Line) -> float:
    shared = min(a.x + a.width, b.x + b.width) - max(a.x, b.x)
    return shared / max(min(a.width, b.width), 0.001)


# -------------------------------------------------------------------------------------------------------- blocks


def _blocks(lines: list[Line], style_of: dict, page: int) -> list[Block]:
    parent = list(range(len(lines)))

    def find(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    index = {id(line): i for i, line in enumerate(lines)}
    by_style: dict[tuple, list[Line]] = collections.defaultdict(list)
    for line in lines:
        by_style[(line.font, round(line.size_pt, 1), line.colour)].append(line)
    for key, group in by_style.items():
        usual = style_of[key].leading_pt
        for above, below, pitch in _pairs(group):
            if usual is not None and pitch <= usual * LEADING_SLACK:
                parent[find(index[id(below)])] = find(index[id(above)])
    groups: dict[int, list[Line]] = collections.defaultdict(list)
    for i, line in enumerate(lines):
        groups[find(i)].append(line)
    blocks = []
    for members in groups.values():
        members.sort(key=lambda line: -line.y)
        first = members[0]
        style = style_of[(first.font, round(first.size_pt, 1), first.colour)]
        x0, y0, x1, y1 = _union([(m.x, m.y, m.x + m.width, m.y + m.height) for m in members]) or (0, 0, 0, 0)
        readable = [m.text for m in members if m.text]
        blocks.append(
            Block(
                "",
                page,
                style.id,
                style.role,
                " ".join(readable) if readable else None,
                x0,
                y0,
                x1 - x0,
                y1 - y0,
                len(members),
                _align(members),
                _indent(members),
                None,
                None,
                all(m.artifact in RUNNING_ARTIFACTS for m in members),
                tag=_dominant_tag(members),
            )
        )
    blocks.sort(key=lambda b: (-(b.y + b.height), b.x))
    return [dataclasses.replace(b, id=f"p{page}-b{i}") for i, b in enumerate(blocks, 1)]


def _dominant_tag(lines: list[Line]) -> str | None:
    tags = collections.Counter(line.tag for line in lines if line.tag)
    return tags.most_common(1)[0][0] if tags else None


def _align(lines: list[Line]) -> str | None:
    if len(lines) < 2:
        return None
    tol = EDGE_EM * lines[0].size_pt
    lefts, rights = [m.x for m in lines], [m.x + m.width for m in lines]
    centres = [m.x + m.width / 2 for m in lines]
    left_flush = max(lefts[1:]) - min(lefts[1:]) <= tol and abs(lefts[0] - lefts[1]) <= INDENT_EM * 8 * lines[0].size_pt
    right_flush = max(rights[:-1]) - min(rights[:-1]) <= tol if len(lines) > 2 else False
    if left_flush and right_flush:
        return "justify"
    if max(centres) - min(centres) <= 2 * tol:
        return "center"
    if left_flush:
        return "left"
    if max(rights) - min(rights) <= tol:
        return "right"
    return None


def _indent(lines: list[Line]) -> float:
    if len(lines) < 2:
        return 0.0
    shift = lines[0].x - lines[1].x
    return round(shift, 2) if abs(shift) > INDENT_EM * lines[0].size_pt else 0.0


def _spacing(blocks: list[Block], styles: list[Style]) -> list[Block]:
    """Fill ``space_before_pt``: how far the block's first line sits below the block above it in its column, beyond
    the style's usual line pitch."""
    leading = {s.id: s.leading_pt for s in styles}
    result = []
    for block in blocks:
        first_bottom = block.y + block.height - _line_height(block)
        pitches = [
            other.y - first_bottom
            for other in blocks
            if other is not block
            and other.column == block.column
            and other.y >= first_bottom
            and min(other.x + other.width, block.x + block.width) - max(other.x, block.x) > 0
        ]
        usual = leading.get(block.style)
        gap = round(min(pitches) - usual, 1) if pitches and usual else None
        result.append(dataclasses.replace(block, space_before_pt=gap if gap is not None and gap > 0.5 else None))
    return result


def _line_height(block: Block) -> float:
    return block.height / max(block.lines, 1)


# ----------------------------------------------------------------------------------------- columns, running text


def _columns(blocks: list[Block]) -> list[list[float]]:
    spans = sorted([b.x, b.x + b.width] for b in blocks if b.width > 0)
    merged: list[list[float]] = []
    for lo, hi in spans:
        if merged and lo < merged[-1][1] - 0.5:
            merged[-1][1] = max(merged[-1][1], hi)
        else:
            merged.append([lo, hi])
    return merged


def _column_of(block: Block, columns: list[list[float]]) -> int | None:
    hits = [i for i, (lo, hi) in enumerate(columns) if min(hi, block.x + block.width) - max(lo, block.x) > 0.5]
    return hits[0] if len(hits) == 1 else None


def _fold(text: str | None) -> str:
    return re.sub(r"\d+", "#", text or "")


def _running(lines: typing.Sequence[Line], pages: typing.Sequence[dict]) -> list[Running]:
    if len(pages) < 2:
        return []
    seen: dict[tuple, list[Line]] = collections.defaultdict(list)
    for line in lines:
        if line.text:
            seen[(line.font, round(line.size_pt, 1), round(line.y), round(line.x), _fold(line.text))].append(line)
    found = []
    for (font, size, y, x, folded), group in seen.items():
        distinct = {line.page for line in group}
        if len(distinct) >= max(2, len(pages) // 2):
            page = next(p for p in pages if p["number"] == group[0].page)
            middle = (page.get("height_pt") or 0) / 2
            found.append(
                Running(
                    folded if len({line.text for line in group}) > 1 else group[0].text or "",
                    "header" if y > middle else "footer",
                    float(x),
                    float(y),
                    font,
                    size,
                    sorted(distinct),
                    len({line.text for line in group}) > 1,
                )
            )
    return sorted(found, key=lambda r: (r.position, -r.y, r.x))


def _mark_running(blocks: list[Block], running: list[Running]) -> list[Block]:
    keys = {(round(r.y), round(r.x), r.text) for r in running}

    def repeats(block: Block) -> bool:
        if block.lines != 1 or not block.text:
            return False
        at = (round(block.y), round(block.x))
        return (*at, block.text) in keys or (*at, _fold(block.text)) in keys

    return [dataclasses.replace(b, running=True) if repeats(b) else b for b in blocks]


# ------------------------------------------------------------------------------------------------ geometry, summary


def _union(rects: list[tuple[float, float, float, float]]) -> tuple[float, float, float, float] | None:
    if not rects:
        return None
    return (min(r[0] for r in rects), min(r[1] for r in rects), max(r[2] for r in rects), max(r[3] for r in rects))


def _background(figure: Figure, media: list[float]) -> bool:
    page_area = (media[2] - media[0]) * (media[3] - media[1])
    return page_area > 0 and figure.width * figure.height > BACKGROUND_FRACTION * page_area


def _summary(pages: list[PageLayout], styles: list[Style]) -> Summary:
    body = next((s for s in styles if s.role == "body"), None)
    body_blocks = [
        b for p in pages for b in p.blocks if b.role == "body" and not b.running and not b.figure and b.lines > 1
    ]
    gutters = [
        cols[i + 1][0] - cols[i][1]
        for p in pages
        for cols in [p.columns]
        for i in range(len(cols) - 1)
        if len(cols) > 1
    ]
    skips = [b.space_before_pt for b in body_blocks if b.space_before_pt]
    widths = [hi - lo for p in pages for lo, hi in p.columns]
    margins = [p.margins_pt for p in pages if p.margins_pt]
    return Summary(
        page_size_mm=[round(pt_to_mm(pages[0].width_pt), 1), round(pt_to_mm(pages[0].height_pt), 1)] if pages else None,
        margins_mm={k: round(pt_to_mm(_median([m[k] for m in margins])), 1) for k in ("top", "bottom", "left", "right")}
        if margins
        else None,
        columns=_mode([len(p.columns) for p in pages if p.columns]),
        gutter_pt=round(_median(gutters), 1) if gutters else None,
        measure_pt=round(_median(widths), 1) if widths else None,
        body_font=body.font if body else None,
        body_size_pt=body.size_pt if body else None,
        leading_pt=body.leading_pt if body else None,
        alignment=_mode([b.align for b in body_blocks if b.align]),
        paragraph_indent_pt=_mode([b.indent_pt for b in body_blocks if b.lines > 2 and b.indent_pt > 0]),
        paragraph_skip_pt=round(_median(skips), 1) if skips else None,
    )


def _median(values: list[float]) -> float:
    return statistics.median(values)


def _mode(values: list) -> typing.Any:
    return statistics.mode(values) if values else None
