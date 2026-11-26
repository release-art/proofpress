"""The print checks already ported to the Rust core, run there (docs/briefs/rust-port.md, stage 3).

Each was proved against the Python it replaced, finding for finding, on the corpus before that Python was deleted
(``bin/parity/print.py``). A check the core could not judge, because it refused the file or read it with gaps, fails
here rather than passing silently: the point of the tool is to confirm a file, so an unread one must not read as OK.
"""

import dataclasses
import json
import pathlib

from ..models import ColourMode, Finding, Profile, Severity
from . import _core

# The checks that run in the Rust core.
PORTED = frozenset(
    {
        "outside-media",
        "xobject-missing",
        "overprint",
        "white-overprint",
        "rich-black",
        "k-only-text",
        "geometry",
        "bleed",
        "centring",
        "page-count",
        "page-size",
        "page-rotate",
        "output-condition",
        "artwork-bleed",
        "outside-bleed",
    }
)
# Those the CMYK ink rules run.
_INK = frozenset({"overprint", "white-overprint", "rich-black"})

_SEVERITY = {"error": Severity.ERROR, "warning": Severity.WARN}


def run(path: pathlib.Path, params: dict[str, object] | None = None) -> list[Finding]:
    """Every ported check's findings on the PDF at *path*, with the print checks' *params* (the core's defaults for
    any left out)."""
    report = json.loads(_core.verify(str(path), path.read_bytes(), "print", json.dumps(params or {})))
    findings = [
        Finding(f["check"], _SEVERITY[f["severity"]], f["message"], f["page"])
        for f in report["findings"]
        if f["check"] in PORTED
    ]
    findings += [
        Finding(c["id"], Severity.ERROR, f"could not be checked: {c['reason']}")
        for c in report["checks"]
        if c["id"] in PORTED and c["status"] == "not_evaluated"
    ]
    return findings


def check(path: pathlib.Path, profile: Profile) -> list[Finding]:
    """The findings of the ported checks *profile* turns on."""
    wanted = {"outside-media", "xobject-missing", "page-count"}
    if profile.colour and profile.colour_mode is ColourMode.CMYK:
        wanted |= _INK
    if profile.k_only_text:
        wanted.add("k-only-text")
    if profile.geometry is not None:
        wanted |= {"geometry", "bleed", "centring"}
    if profile.uniform_pages:
        wanted |= {"page-size", "page-rotate"}
    if profile.output_condition is not None:
        wanted.add("output-condition")
    if profile.require_bleed:
        wanted.add("artwork-bleed")
    if profile.no_marks:
        wanted.add("outside-bleed")
    return [f for f in run(path, params(profile)) if f.check in wanted]


def params(profile: Profile) -> dict[str, object]:
    """The print checks' parameters *profile* sets, as the core takes them."""
    return {
        "small_text_pt": profile.small_text_pt,
        "geometry": dataclasses.asdict(profile.geometry) if profile.geometry is not None else None,
        "uniform_pages": profile.uniform_pages,
        "pages_multiple_of": profile.pages_multiple_of or None,
        "output_condition": profile.output_condition,
    }
