"""An XMP packet as plain data, keyed by the prefixes the packet itself declares (``dc:title``, ``xmpTPg:Fonts``...).

Layout programs write more than a title into XMP: InDesign lists the fonts, plate names and swatches (with their CMYK
values) a job used, and the edit history records which application made each save. Those are exactly the facts a
template rebuild wants, so nothing is dropped: properties become strings, ``rdf:Seq``/``Bag`` become lists, a language
``rdf:Alt`` becomes ``{lang: text}`` (or the text, when it is only ``x-default``), and structures become dicts.

The standard library parser is enough because expat refuses entity-expansion bombs, and a packet that is not XML at
all raises :class:`ValueError`, so the caller can keep the raw text and say so.
"""

import io
import typing
import xml.etree.ElementTree as ET  # noqa: S405 -- see the module docstring

_RDF = "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
_XML = "http://www.w3.org/XML/1998/namespace"
_CONTAINERS = {f"{{{_RDF}}}{name}" for name in ("Seq", "Bag", "Alt")}
_SKIP_ATTRIBUTES = {f"{{{_RDF}}}about", f"{{{_RDF}}}parseType", f"{{{_RDF}}}resource", f"{{{_XML}}}lang"}


def parse(packet: bytes) -> dict[str, typing.Any]:
    """The properties of every ``rdf:Description`` in *packet*, merged. Raises ValueError for unreadable XML."""
    prefixes: dict[str, str] = {}
    root = None
    try:
        for event, item in ET.iterparse(io.BytesIO(packet), events=("start-ns", "end")):
            if event == "start-ns":
                prefixes.setdefault(item[1], item[0])
            else:
                root = item
    except ET.ParseError as exc:
        raise ValueError(f"not XML: {exc}") from exc
    if root is None:
        return {}
    reader = _Reader(prefixes)
    merged: dict[str, typing.Any] = {}
    for description in root.iter(f"{{{_RDF}}}Description"):
        merged.update(reader.properties(description))
    return merged


class _Reader:
    def __init__(self, prefixes: dict[str, str]) -> None:
        self.prefixes = prefixes

    def name(self, tag: str) -> str:
        uri, _, local = tag[1:].partition("}") if tag.startswith("{") else ("", "", tag)
        return f"{self.prefixes[uri]}:{local}" if self.prefixes.get(uri) else local

    def attributes(self, element: ET.Element) -> dict[str, str]:
        return {self.name(a): v for a, v in element.attrib.items() if a not in _SKIP_ATTRIBUTES and "xmlns" not in a}

    def value(self, element: ET.Element) -> typing.Any:
        children = list(element)
        if children and children[0].tag in _CONTAINERS:
            return self.container(children[0])
        fields = self.attributes(element)
        if children and children[0].tag == f"{{{_RDF}}}Description":
            return {**fields, **self.properties(children[0])}
        if children:
            return {**fields, **{self.name(c.tag): self.value(c) for c in children}}
        text = (element.text or "").strip()
        return {**fields, "value": text} if fields and text else (fields or text)

    def container(self, element: ET.Element) -> typing.Any:
        items = [(item.get(f"{{{_XML}}}lang"), self.value(item)) for item in element]
        if element.tag.endswith("}Alt") and all(lang for lang, _ in items):
            langs = dict(items)
            return langs["x-default"] if list(langs) == ["x-default"] else langs
        return [item for _, item in items]

    def properties(self, description: ET.Element) -> dict[str, typing.Any]:
        found: dict[str, typing.Any] = dict(self.attributes(description))
        found.update({self.name(child.tag): self.value(child) for child in description})
        return found
