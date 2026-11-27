"""The structure tree of a tagged PDF: what the producer said each piece of content *is*.

A tagged PDF (Word, InDesign and Acrobat export them) carries a tree of elements, ``H2``, ``P``, ``Table``, ``Figure``
and so on, whose leaves point at marked content on the pages by id (MCID). Where it exists it is better evidence than
anything inferred from geometry: it says which text is a level-2 heading, where a paragraph ends, what a figure is
(its alt text, which is the nearest thing a PDF has to a name for an asset), and carries the producer's own layout
attributes (alignment, indents, spacing, row and column spans).

Producers use their own tag names (``Heading 2``, ``Body Text``) and say what they mean in the ``/RoleMap``; every
element here has both its tag as written and its standard role. The Rust core reads the tree (``extract/tagged.rs``).
"""

import dataclasses
import typing

# Tags whose text is a unit of reading (a paragraph, a heading, a cell...): lines under one of them belong together.
LEAF_BLOCKS = frozenset(
    ("P", "H", "H1", "H2", "H3", "H4", "H5", "H6", "Title", "LBody", "Lbl", "TD", "TH", "Caption", "Code", "Formula")
    + ("Note", "BibEntry", "Reference", "Figure", "Quote", "BlockQuote")
)


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


def from_core(data: dict[str, typing.Any] | None) -> Tagged | None:
    """The structure tree as the Rust core read it (``_core.extract``'s ``structure``), or None when there is none."""
    if data is None:
        return None
    elements = [
        Element(
            e["id"],
            e["parent"],
            e["tag"],
            e["role"],
            e["block"],
            e["page"],
            tuple((page, mcid) for page, mcid in e["marks"]),
            e["alt"],
            e["actual_text"],
            e["title"],
            e["lang"],
            e["attrs"],
        )
        for e in data["elements"]
    ]
    by_mark = {mark: e.id for e in elements for mark in e.marks}
    return Tagged(data["marked"], data["language"], data["role_map"], elements, by_mark)
