"""What a text run needs to know about its font: glyph widths, embedding, and which codes show ``.notdef``.

Read straight from the PDF font dictionaries, no font program is parsed. Widths come from ``/Widths`` (simple fonts) or
``/W`` and ``/DW`` (composite fonts); a font that declares neither falls back to a plausible default, so a text box
for an unembedded standard font is approximate. ``.notdef`` is exact for the two cases that matter: a composite font
using an ``Identity`` encoding (what LuaLaTeX writes), where code 0 is CID 0, and a simple font whose ``/Differences``
maps a code to ``.notdef``.
"""

import dataclasses
import re

import pikepdf

_SUBSET_PREFIX = re.compile(r"^[A-Z]{6}\+")
_IDENTITY = ("/Identity-H", "/Identity-V")
_ADOBE_COLLECTIONS = ("GB1", "CNS1", "Korea1")  # Japan1 differs from ASCII at a few Latin CIDs, so it is left out
_ADOBE_LATIN_CIDS, _ADOBE_LATIN_OFFSET = 95, 31  # CID 1 is the space, U+0020
_FALLBACK_WIDTH = 500.0  # per 1000 em; typical of a text face
_DEFAULT_ASCENT, _DEFAULT_DESCENT = 800.0, -200.0


@dataclasses.dataclass(frozen=True)
class Font:
    name: str
    subtype: str
    embedded: bool
    has_to_unicode: bool
    code_bytes: int
    default_width: float
    widths: dict[int, float]
    notdef_codes: frozenset[int]
    ascent: float = _DEFAULT_ASCENT
    descent: float = _DEFAULT_DESCENT
    composite: bool = False
    decodable: bool = True  # False for a composite font whose encoding is not Identity: codes can't be split reliably
    to_unicode: dict[int, str] = dataclasses.field(default_factory=dict)
    adobe_latin: bool = False  # an Adobe GB1/CNS1/Korea1 CID font: CIDs 1-95 are ASCII 0x20-0x7E by definition

    def text(self, codes: list[int]) -> str | None:
        """The characters *codes* show: from ``/ToUnicode``, else (for an Adobe CJK collection) the Latin CIDs, else
        Latin-1 for a simple font, else ``None`` (unknown)."""
        if self.to_unicode or self.adobe_latin:
            shown = [self.to_unicode.get(code) or self._latin(code) or "\N{REPLACEMENT CHARACTER}" for code in codes]
            return "".join(shown) if self.to_unicode or any(c != "\N{REPLACEMENT CHARACTER}" for c in shown) else None
        return None if self.composite else bytes(code & 0xFF for code in codes).decode("latin-1")

    def _latin(self, code: int) -> str | None:
        return chr(code + _ADOBE_LATIN_OFFSET) if self.adobe_latin and 1 <= code <= _ADOBE_LATIN_CIDS else None

    def decode(self, data: bytes) -> list[int]:
        """The character codes in a shown string."""
        if self.code_bytes == 1:
            return list(data)
        padded = data if len(data) % 2 == 0 else data + b"\x00"
        return [int.from_bytes(padded[i : i + 2]) for i in range(0, len(padded), 2)]

    def width(self, code: int) -> float:
        """Advance of *code*, in thousandths of the font size."""
        return self.widths.get(code, self.default_width)

    def is_notdef(self, code: int) -> bool:
        if self.composite:
            return self.decodable and code == 0
        return code in self.notdef_codes


UNKNOWN = Font("?", "unknown", False, False, 1, _FALLBACK_WIDTH, {}, frozenset())


def load(font: pikepdf.Object | None) -> Font:
    """Read a font dictionary; an absent or malformed one gives :data:`UNKNOWN`."""
    if font is None:
        return UNKNOWN
    try:
        return _load(font)
    except KeyError, TypeError, ValueError, IndexError, AttributeError, pikepdf.PdfError:
        return UNKNOWN


def _load(font: pikepdf.Object) -> Font:
    subtype = str(font.get("/Subtype", "")).lstrip("/")
    name = _SUBSET_PREFIX.sub("", str(font.get("/BaseFont", "?")).lstrip("/"))
    if subtype == "Type0":
        return _load_composite(font, name)
    descriptor = font.get("/FontDescriptor")
    scale = float(font["/FontMatrix"][0]) * 1000.0 if "/FontMatrix" in font else 1.0  # Type3
    first = int(font.get("/FirstChar", 0))
    widths = {first + i: float(w) * scale for i, w in enumerate(font.get("/Widths", []))}
    missing = float(descriptor.get("/MissingWidth", 0)) if descriptor is not None else 0.0
    return Font(
        name=name,
        subtype=subtype,
        embedded=subtype == "Type3" or _embedded(descriptor),
        has_to_unicode="/ToUnicode" in font,
        code_bytes=1,
        default_width=missing or (_FALLBACK_WIDTH if not widths else 0.0),
        widths=widths,
        notdef_codes=_notdef_codes(font.get("/Encoding")),
        to_unicode=_to_unicode(font.get("/ToUnicode")),
        ascent=float(descriptor.get("/Ascent", _DEFAULT_ASCENT)) if descriptor is not None else _DEFAULT_ASCENT,
        descent=float(descriptor.get("/Descent", _DEFAULT_DESCENT)) if descriptor is not None else _DEFAULT_DESCENT,
    )


def _load_composite(font: pikepdf.Object, name: str) -> Font:
    descendant = font["/DescendantFonts"][0]
    descriptor = descendant.get("/FontDescriptor")
    encoding = font.get("/Encoding")
    identity = encoding is not None and str(encoding) in _IDENTITY
    return Font(
        name=name,
        subtype="Type0",
        embedded=_embedded(descriptor),
        has_to_unicode="/ToUnicode" in font,
        code_bytes=2,
        default_width=float(descendant.get("/DW", 1000)),
        widths=_cid_widths(descendant.get("/W")),
        notdef_codes=frozenset(),
        to_unicode=_to_unicode(font.get("/ToUnicode")),
        ascent=float(descriptor.get("/Ascent", _DEFAULT_ASCENT)) if descriptor is not None else _DEFAULT_ASCENT,
        descent=float(descriptor.get("/Descent", _DEFAULT_DESCENT)) if descriptor is not None else _DEFAULT_DESCENT,
        composite=True,
        decodable=identity,
        adobe_latin=identity and _adobe_collection(descendant),
    )


def _adobe_collection(descendant: pikepdf.Object) -> bool:
    info = descendant.get("/CIDSystemInfo")
    return (
        info is not None and str(info.get("/Registry")) == "Adobe" and str(info.get("/Ordering")) in _ADOBE_COLLECTIONS
    )


_BF_CHAR = re.compile(rb"beginbfchar(.*?)endbfchar", re.DOTALL)
_BF_RANGE = re.compile(rb"beginbfrange(.*?)endbfrange", re.DOTALL)
_BF_TOKEN = re.compile(rb"<[^>]*>|\[|\]")


def _hex(token: bytes) -> bytes:
    return bytes.fromhex(re.sub(rb"[<>\s]", b"", token).decode())


def _to_unicode(stream: pikepdf.Object | None) -> dict[int, str]:
    """A ``/ToUnicode`` CMap's ``bfchar`` and ``bfrange`` entries as code -> text; empty if absent or unreadable."""
    if stream is None:
        return {}
    try:
        data = bytes(stream.read_bytes())
        mapping: dict[int, str] = {}
        for block in _BF_CHAR.findall(data):
            tokens = _BF_TOKEN.findall(block)
            for src, dst in zip(tokens[::2], tokens[1::2], strict=False):
                mapping[int.from_bytes(_hex(src))] = _hex(dst).decode("utf-16-be", "replace")
        for block in _BF_RANGE.findall(data):
            tokens, i = _BF_TOKEN.findall(block), 0
            while i + 2 < len(tokens):
                low, high = int.from_bytes(_hex(tokens[i])), int.from_bytes(_hex(tokens[i + 1]))
                if tokens[i + 2] == b"[":
                    end = tokens.index(b"]", i + 2)
                    for code, dst in zip(range(low, high + 1), tokens[i + 3 : end], strict=False):
                        mapping[code] = _hex(dst).decode("utf-16-be", "replace")
                    i = end + 1
                else:
                    base = _hex(tokens[i + 2]).decode("utf-16-be", "replace")
                    for offset in range(min(high - low, 0xFFFF) + 1):
                        mapping[low + offset] = base[:-1] + chr(ord(base[-1]) + offset) if base else ""
                    i += 3
        return mapping
    except pikepdf.PdfError, ValueError, IndexError, TypeError, OverflowError:
        return {}


def _embedded(descriptor: pikepdf.Object | None) -> bool:
    return descriptor is not None and any(k in descriptor for k in ("/FontFile", "/FontFile2", "/FontFile3"))


def _cid_widths(w: pikepdf.Object | None) -> dict[int, float]:
    """A CID font's ``/W`` array: ``c [w1 w2 ...]`` and ``c_first c_last w`` entries."""
    widths: dict[int, float] = {}
    if w is None:
        return widths
    items = list(w)
    i = 0
    while i < len(items):
        first = int(items[i])
        nxt = items[i + 1] if i + 1 < len(items) else None
        if isinstance(nxt, pikepdf.Array):
            for offset, width in enumerate(nxt):
                widths[first + offset] = float(width)
            i += 2
        else:
            last, width = int(nxt), float(items[i + 2])  # type: ignore[arg-type]
            for cid in range(first, last + 1):
                widths[cid] = width
            i += 3
    return widths


def _notdef_codes(encoding: pikepdf.Object | None) -> frozenset[int]:
    """Codes a simple font's ``/Differences`` maps to ``.notdef``."""
    if encoding is None or not isinstance(encoding, pikepdf.Dictionary):
        return frozenset()
    codes: set[int] = set()
    code = 0
    for item in encoding.get("/Differences", []):
        if isinstance(item, pikepdf.Name):
            if str(item) == "/.notdef":
                codes.add(code)
            code += 1
        else:
            code = int(item)
    return frozenset(codes)
