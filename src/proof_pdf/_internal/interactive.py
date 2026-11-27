"""What a PDF holds beyond its pages: form fields, links, other annotations, outline, page labels, layers, attachments.

All of it says something about how the document was meant to work, which a template rebuild needs: a fillable
certificate is a form, a datasheet's contents list is an outline with page numbers, and a page numbered ``iv`` is
front matter. The Rust core reads all of it (``extract/interactive.rs``); a malformed entry is skipped, not a reason to
lose the rest.
"""

import pathlib
import re
import typing

_SLUG = re.compile(r"[^A-Za-z0-9._-]+")


def from_core(
    data: dict[str, typing.Any], blobs: list[bytes], out_dir: pathlib.Path, failures: list[str]
) -> dict[str, typing.Any]:
    """The core's reading (``_core.extract``'s ``interactive``), with each attachment written under *out_dir*."""
    attachments = []
    for attachment in data["attachments"]:
        blob = attachment["data"]
        if "error" in blob:
            failures.append(blob["error"])
            continue
        name = attachment["name"]
        safe = _SLUG.sub("_", pathlib.PurePath(name).name).strip("._") or "attachment"
        target = out_dir / "attachments" / safe
        target.parent.mkdir(exist_ok=True)
        target.write_bytes(blobs[blob["data"]])
        attachments.append(
            {
                "name": name,
                "file": target.relative_to(out_dir).as_posix(),
                "bytes": target.stat().st_size,
                "description": attachment["description"],
            }
        )
    return {**data, "attachments": attachments}
