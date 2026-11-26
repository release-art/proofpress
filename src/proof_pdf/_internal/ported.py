"""The print checks already ported to the Rust core, run there (docs/briefs/rust-port.md, stage 3).

Each was proved against the Python it replaced, finding for finding, on the corpus before that Python was deleted
(``bin/parity/print.py``). A check the core could not judge, because it refused the file or read it with gaps, fails
here rather than passing silently: the point of the tool is to confirm a file, so an unread one must not read as OK.
"""

import json
import pathlib

from ..models import ColourMode, Finding, Profile, Severity
from . import _core

# The checks that run in the Rust core.
PORTED = frozenset({"outside-media", "xobject-missing", "overprint", "white-overprint", "rich-black", "k-only-text"})
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
    wanted = {"outside-media", "xobject-missing"}
    if profile.colour and profile.colour_mode is ColourMode.CMYK:
        wanted |= _INK
    if profile.k_only_text:
        wanted.add("k-only-text")
    return [f for f in run(path, {"small_text_pt": profile.small_text_pt}) if f.check in wanted]
