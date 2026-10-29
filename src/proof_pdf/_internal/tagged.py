"""The structure tree of a tagged PDF: what the producer said each piece of content *is*.

A tagged PDF (Word, InDesign and Acrobat export them) carries a tree of elements, ``H2``, ``P``, ``Table``, ``Figure``
and so on, whose leaves point at marked content on the pages by id (MCID). Where it exists it is better evidence than
anything inferred from geometry: it says which text is a level-2 heading, where a paragraph ends, what a figure is
(its alt text, which is the nearest thing a PDF has to a name for an asset), and carries the producer's own layout
attributes (alignment, indents, spacing, row and column spans).

Producers use their own tag names (``Heading 2``, ``Body Text``) and say what they mean in the ``/RoleMap``; every
element here has both its tag as written and its standard role.
"""

import dataclasses
import typing

import pikepdf

# Tags whose text is a unit of reading (a paragraph, a heading, a cell...): lines under one of them belong together.
LEAF_BLOCKS = frozenset(
    ("P", "H", "H1", "H2", "H3", "H4", "H5", "H6", "Title", "LBody", "Lbl", "TD", "TH", "Caption", "Code", "Formula")
    + ("Note", "BibEntry", "Reference", "Figure", "Quote", "BlockQuote")
)
_ATTRIBUTES = frozenset(
    ("TextAlign", "BlockAlign", "InlineAlign", "SpaceBefore", "SpaceAfter", "StartIndent", "EndIndent", "TextIndent")
    + ("LineHeight", "Placement", "BBox", "Width", "Height", "RowSpan", "ColSpan", "Scope", "ListNumbering")
    + ("WritingMode", "BaselineShift", "TextDecorationType")
)
_MAX_ELEMENTS = 200_000
_MAX_DEPTH = 300
_MAX_ROLE_HOPS = 6


@dataclasses.dataclass(frozen=True)
class Element:
    id: int
    parent: int | None
    tag: str  # as the producer wrote it
    role: str  # the standard tag it stands for, through the RoleMap
    block: int  # the id of the paragraph-like element this one belongs to (itself, for one)
    page: int | None
    marks: tuple[tuple[int, int], ...]  # (page, MCID) of the content it holds directly
    alt: str | None
    actual_text: str | None
    title: str | None
    lang: str | None
    attrs: dict[str, typing.Any]


@dataclasses.dataclass
class Tagged:
    marked: bool  # the catalog says the file is tagged
    language: str | None
    role_map: dict[str, str]
    elements: list[Element]
    by_mark: dict[tuple[int, int], int]  # (page, MCID) -> id of the innermost element holding it

    def at(self, page: int, mcid: int | None) -> Element | None:
        index = self.by_mark.get((page, mcid)) if mcid is not None else None
        return None if index is None else self.elements[index]

    def with_alt(self, page: int, mcids: typing.Iterable[int | None]) -> Element | None:
        """The nearest element, from the content's own up through its parents, that carries alt text."""
        for mcid in mcids:
            element = self.at(page, mcid)
            while element is not None:
                if element.alt:
                    return element
                element = self.elements[element.parent] if element.parent is not None else None
        return None


def read(pdf: pikepdf.Pdf) -> Tagged | None:
    """The structure tree, or None when the file has none (or it cannot be read)."""
    root = pdf.Root.get("/StructTreeRoot")
    if root is None:
        return None
    pages = {page.obj.objgen: n for n, page in enumerate(pdf.pages, start=1)}
    role_map = {str(k).lstrip("/"): str(v).lstrip("/") for k, v in root.get("/RoleMap", {}).items()}
    reader = _Reader(pages, role_map)
    try:
        reader.node(root.get("/K"), None, None, 0)
    except pikepdf.PdfError, KeyError, TypeError, ValueError, AttributeError, IndexError:
        pass  # keep what was read before the damage
    mark_info = pdf.Root.get("/MarkInfo")
    return Tagged(
        marked=bool(mark_info is not None and mark_info.get("/Marked", False)),
        language=str(pdf.Root["/Lang"]) if "/Lang" in pdf.Root else None,
        role_map=role_map,
        elements=reader.elements,
        by_mark=reader.by_mark,
    )


class _Reader:
    def __init__(self, pages: dict[tuple[int, int], int], role_map: dict[str, str]) -> None:
        self.pages, self.role_map = pages, role_map
        self.elements: list[Element] = []
        self.by_mark: dict[tuple[int, int], int] = {}
        self.seen: set[tuple[int, int]] = set()

    def node(self, kids: typing.Any, parent: int | None, page: int | None, depth: int) -> None:
        if kids is None or depth > _MAX_DEPTH or len(self.elements) > _MAX_ELEMENTS:
            return
        if isinstance(kids, pikepdf.Array):
            for kid in kids:
                self.node(kid, parent, page, depth)
        elif isinstance(kids, int) and parent is not None and page is not None:
            self.mark(parent, page, kids)
        elif isinstance(kids, pikepdf.Dictionary):
            self.dictionary(kids, parent, page, depth)

    def dictionary(self, kid: pikepdf.Dictionary, parent: int | None, page: int | None, depth: int) -> None:
        if kid.objgen != (0, 0):
            if kid.objgen in self.seen:
                return
            self.seen.add(kid.objgen)
        own = self.pages.get(kid["/Pg"].objgen, page) if "/Pg" in kid else page
        kind = str(kid.get("/Type", ""))
        if kind == "/MCR" and parent is not None and "/MCID" in kid and "/Stm" not in kid:
            if own is not None:
                self.mark(parent, own, int(kid["/MCID"]))
        elif "/S" in kid:
            self.element(kid, parent, own, depth)

    def mark(self, element: int, page: int, mcid: int) -> None:
        current = self.elements[element]
        self.elements[element] = dataclasses.replace(current, marks=(*current.marks, (page, mcid)))
        self.by_mark[(page, mcid)] = element

    def element(self, node: pikepdf.Dictionary, parent: int | None, page: int | None, depth: int) -> None:
        tag = str(node["/S"]).lstrip("/")
        role = self._role(tag)
        index = len(self.elements)
        above = self.elements[parent] if parent is not None else None
        in_leaf = above is not None and self.elements[above.block].role in LEAF_BLOCKS
        block = index if role in LEAF_BLOCKS or above is None or not in_leaf else above.block
        self.elements.append(
            Element(
                index,
                parent,
                tag,
                role,
                block,
                page,
                (),
                _text(node.get("/Alt")),
                _text(node.get("/ActualText")),
                _text(node.get("/T")),
                _text(node.get("/Lang")),
                _attributes(node.get("/A")),
            )
        )
        self.node(node.get("/K"), index, page, depth + 1)

    def _role(self, tag: str) -> str:
        for _ in range(_MAX_ROLE_HOPS):
            nxt = self.role_map.get(tag)
            if nxt is None or nxt == tag:
                break
            tag = nxt
        return tag


def _text(value: typing.Any) -> str | None:
    if value is None:
        return None
    return str(value).replace("\x00", "").strip() or None


def _attributes(value: typing.Any) -> dict[str, typing.Any]:
    found: dict[str, typing.Any] = {}
    groups = list(value) if isinstance(value, pikepdf.Array) else [value]
    for group in groups:
        if isinstance(group, pikepdf.Dictionary):
            for key, item in group.items():
                name = str(key).lstrip("/")
                if name in _ATTRIBUTES and (plain := _plain(item)) is not None:
                    found[name] = plain
    return found


def _plain(value: typing.Any) -> typing.Any:
    if isinstance(value, bool | int | float):
        return value
    if isinstance(value, pikepdf.Name):
        return str(value).lstrip("/")
    if isinstance(value, pikepdf.Array):
        items = [_plain(v) for v in value]
        return items if all(i is not None for i in items) else None
    try:
        return float(value)
    except TypeError, ValueError:
        return None
