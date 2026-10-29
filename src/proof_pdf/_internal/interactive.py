"""What a PDF holds beyond its pages: form fields, links, other annotations, outline, page labels, layers, attachments.

All of it says something about how the document was meant to work, which a template rebuild needs: a fillable
certificate is a form, a datasheet's contents list is an outline with page numbers, and a page numbered ``iv`` is
front matter. Everything is read defensively: a malformed entry is skipped, not a reason to lose the rest.
"""

import pathlib
import re
import typing

import pikepdf

_ERRORS = (pikepdf.PdfError, KeyError, IndexError, TypeError, ValueError, AttributeError, OSError)

_BUTTON_RADIO, _BUTTON_PUSH = 1 << 15, 1 << 16
_CHOICE_COMBO = 1 << 17
_DEFAULT_APPEARANCE = re.compile(r"/(\S+)\s+([\d.]+)\s+Tf")
_SLUG = re.compile(r"[^A-Za-z0-9._-]+")
_MAX_OUTLINE = 5000  # entries; a hostile or broken outline can loop or be enormous


def collect(pdf: pikepdf.Pdf, out_dir: pathlib.Path, failures: list[str]) -> dict[str, typing.Any]:
    pages = {page.obj.objgen: number for number, page in enumerate(pdf.pages, start=1)}
    reader = _Reader(pdf, pages)
    steps = {
        "fields": reader.fields,
        "links": reader.links,
        "annotations": reader.annotations,
        "outline": reader.outline,
        "page_labels": reader.page_labels,
        "layers": reader.layers,
    }
    result: dict[str, typing.Any] = {}
    for name, step in steps.items():
        try:
            result[name] = step()
        except _ERRORS as exc:
            failures.append(f"{name}: {exc}")
            result[name] = []
    result["attachments"] = _attachments(pdf, out_dir, failures)
    return result


class _Reader:
    def __init__(self, pdf: pikepdf.Pdf, pages: dict[tuple[int, int], int]) -> None:
        self.pdf, self.pages = pdf, pages
        self.names = self._named_destinations()

    # ------------------------------------------------------------------------------------------ destinations
    def _named_destinations(self) -> dict[str, typing.Any]:
        found: dict[str, typing.Any] = {}
        root = self.pdf.Root
        try:
            if "/Dests" in root:
                found.update({str(k).lstrip("/"): v for k, v in root.Dests.items()})
            tree = root.get("/Names", {}).get("/Dests")
            if tree is not None:
                found.update({str(k): v for k, v in pikepdf.NameTree(tree).items()})
        except _ERRORS:
            pass
        return found

    def target(self, dest: typing.Any) -> dict[str, typing.Any] | None:
        """Where a destination (explicit, named, or a dictionary holding ``/D``) points: a page and a position."""
        if isinstance(dest, pikepdf.Name | pikepdf.String):
            dest = self.names.get(str(dest).lstrip("/"))
        if isinstance(dest, pikepdf.Dictionary):
            dest = dest.get("/D")
        if not isinstance(dest, pikepdf.Array) or not len(dest):
            return None
        page = self.pages.get(dest[0].objgen) if isinstance(dest[0], pikepdf.Object) else None
        spot = [float(v) for v in list(dest)[2:] if isinstance(v, int | float | pikepdf.Object) and _number(v)]
        return {"page": page, "view": str(dest[1]).lstrip("/") if len(dest) > 1 else None, "at": spot or None}

    def action(self, annotation: pikepdf.Object) -> dict[str, typing.Any]:
        act = annotation.get("/A")
        if act is None:
            return {"to": self.target(annotation.get("/Dest"))} if "/Dest" in annotation else {}
        kind = str(act.get("/S", "")).lstrip("/")
        out: dict[str, typing.Any] = {"action": kind}
        if kind == "URI":
            out["uri"] = str(act.get("/URI", ""))
        elif kind == "GoTo":
            out["to"] = self.target(act.get("/D"))
        elif kind in ("GoToR", "Launch"):
            out["file"] = str(act.get("/F", ""))
        return out

    # ------------------------------------------------------------------------------------------ annotations
    def _annotations(self) -> typing.Iterator[tuple[int, pikepdf.Object]]:
        for number, page in enumerate(self.pdf.pages, start=1):
            for annotation in page.obj.get("/Annots", []):
                yield number, annotation

    def links(self) -> list[dict[str, typing.Any]]:
        return [
            {"page": n, "rect": _rect(a), **self.action(a)}
            for n, a in self._annotations()
            if str(a.get("/Subtype")) == "/Link"
        ]

    def annotations(self) -> list[dict[str, typing.Any]]:
        """Notes, highlights, stamps and the like (links and form widgets have their own lists)."""
        skip = ("/Link", "/Widget", "/Popup")
        return [
            {
                "page": n,
                "type": str(a.get("/Subtype", "")).lstrip("/"),
                "rect": _rect(a),
                "contents": str(a.get("/Contents", "")) or None,
                "author": str(a.get("/T", "")) or None,
            }
            for n, a in self._annotations()
            if str(a.get("/Subtype")) not in skip
        ]

    # --------------------------------------------------------------------------------------------- forms
    def fields(self) -> list[dict[str, typing.Any]]:
        form = self.pdf.Root.get("/AcroForm")
        if form is None:
            return []
        widget_pages = {a.objgen: n for n, a in self._annotations() if str(a.get("/Subtype")) == "/Widget"}
        found: list[dict[str, typing.Any]] = []
        for top in form.get("/Fields", []):
            self._field(top, "", {}, widget_pages, found, form.get("/DA"))
        return found

    def _field(self, node, prefix: str, inherited: dict, widget_pages: dict, found: list, default_da) -> None:
        inherited = {
            **inherited,
            **{k: node[k] for k in ("/FT", "/V", "/DV", "/Ff", "/DA", "/Opt", "/MaxLen", "/Q") if k in node},
        }
        name = f"{prefix}.{node['/T']}".lstrip(".") if "/T" in node else prefix
        kids = list(node.get("/Kids", []))
        widgets = [k for k in kids if "/T" not in k and str(k.get("/Subtype")) == "/Widget"]
        if kids and not widgets:
            for kid in kids:
                self._field(kid, name, inherited, widget_pages, found, default_da)
            return
        for widget in widgets or [node]:
            found.append(_describe_field(name, inherited, widget, widget_pages.get(widget.objgen), node, default_da))

    # --------------------------------------------------------------------------------------- navigation
    def outline(self) -> list[dict[str, typing.Any]]:
        root = self.pdf.Root.get("/Outlines")
        entries: list[dict[str, typing.Any]] = []
        if root is None:
            return entries
        budget = [_MAX_OUTLINE]

        def walk(item, level: int) -> None:
            while item is not None and budget[0] > 0:
                budget[0] -= 1
                where = self.action(item) if "/A" in item else {"to": self.target(item.get("/Dest"))}
                entries.append({"level": level, "title": str(item.get("/Title", "")), **where})
                if item.get("/First") is not None:
                    walk(item["/First"], level + 1)
                item = item.get("/Next")

        walk(root.get("/First"), 1)
        return entries

    def page_labels(self) -> list[dict[str, typing.Any]]:
        labels = self.pdf.Root.get("/PageLabels")
        if labels is None:
            return []
        return [
            {
                "from_page": int(index) + 1,
                "style": str(label.get("/S", "")).lstrip("/") or None,  # D decimal, r/R roman, a/A letters
                "prefix": str(label.get("/P", "")),
                "start": int(label.get("/St", 1)),
            }
            for index, label in pikepdf.NumberTree(labels).items()
        ]

    def layers(self) -> list[dict[str, typing.Any]]:
        properties = self.pdf.Root.get("/OCProperties")
        if properties is None:
            return []
        hidden = {g.objgen for g in properties.get("/D", {}).get("/OFF", [])}
        return [
            {"name": str(group.get("/Name", "")), "visible": group.objgen not in hidden}
            for group in properties.get("/OCGs", [])
        ]


def _describe_field(name: str, inherited: dict, widget: pikepdf.Object, page: int | None, node, default_da) -> dict:
    kind = str(inherited.get("/FT", "")).lstrip("/")
    flags = int(inherited.get("/Ff", 0))
    shown = {
        "Tx": "text",
        "Sig": "signature",
        "Ch": "dropdown" if flags & _CHOICE_COMBO else "list",
        "Btn": "radio" if flags & _BUTTON_RADIO else "button" if flags & _BUTTON_PUSH else "checkbox",
    }.get(kind, kind or "unknown")
    appearance = _DEFAULT_APPEARANCE.search(str(inherited.get("/DA", default_da or "")))
    return {
        "name": name,
        "type": shown,
        "page": page,
        "rect": _rect(widget),
        "value": _signature(inherited.get("/V")) if kind == "Sig" else _value(inherited.get("/V")),
        "default": _value(inherited.get("/DV")),
        "options": [_value(o if not isinstance(o, pikepdf.Array) else o[1]) for o in inherited.get("/Opt", [])] or None,
        "max_length": int(inherited["/MaxLen"]) if "/MaxLen" in inherited else None,
        "alignment": {0: "left", 1: "center", 2: "right"}.get(int(inherited.get("/Q", 0))),
        "font": appearance.group(1) if appearance else None,
        "font_size_pt": float(appearance.group(2)) if appearance else None,
        "tooltip": str(node.get("/TU", "")) or None,
        "required": bool(flags & 2),
        "read_only": bool(flags & 1),
    }


def _attachments(pdf: pikepdf.Pdf, out_dir: pathlib.Path, failures: list[str]) -> list[dict[str, typing.Any]]:
    tree = pdf.Root.get("/Names", {}).get("/EmbeddedFiles")
    found: list[dict[str, typing.Any]] = []
    if tree is None:
        return found
    try:
        items = list(pikepdf.NameTree(tree).items())
    except _ERRORS as exc:
        failures.append(f"attachments: {exc}")
        return found
    for name, spec in items:
        try:
            filename = str(spec.get("/UF") or spec.get("/F") or name)
            stream = spec["/EF"].get("/F") or spec["/EF"].get("/UF")
            if stream is None:
                continue
            safe = _SLUG.sub("_", pathlib.PurePath(filename).name).strip("._") or "attachment"
            target = out_dir / "attachments" / safe
            target.parent.mkdir(exist_ok=True)
            target.write_bytes(bytes(stream.read_bytes()))
            found.append(
                {
                    "name": filename,
                    "file": target.relative_to(out_dir).as_posix(),
                    "bytes": target.stat().st_size,
                    "description": str(spec.get("/Desc", "")) or None,
                }
            )
        except _ERRORS as exc:
            failures.append(f"attachment {name}: {exc}")
    return found


def _rect(annotation: pikepdf.Object) -> list[float] | None:
    try:
        x0, y0, x1, y1 = (float(v) for v in annotation["/Rect"])
        return [min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1)]
    except _ERRORS:
        return None


def _number(value: typing.Any) -> bool:
    try:
        float(value)
    except TypeError, ValueError:
        return False
    return True


def _value(value: typing.Any) -> str | None:
    """A field's value as text; a dictionary or stream (not a text value) is left out rather than dumped."""
    if value is None or isinstance(value, pikepdf.Dictionary | pikepdf.Stream | pikepdf.Array):
        return None
    return str(value).lstrip("/") if isinstance(value, pikepdf.Name) else str(value)


def _signature(value: typing.Any) -> dict[str, typing.Any] | None:
    """The facts of a digital signature (who, when, how); its binary contents are not copied."""
    if not isinstance(value, pikepdf.Dictionary):
        return None
    ranges = value.get("/ByteRange")
    return {
        "signer": _value(value.get("/Name")),
        "date": _value(value.get("/M")),
        "reason": _value(value.get("/Reason")),
        "location": _value(value.get("/Location")),
        "filter": _value(value.get("/Filter")),
        "sub_filter": _value(value.get("/SubFilter")),
        "byte_range": [int(v) for v in ranges] if ranges is not None else None,
    }
