"""The print checks already ported to the Rust core, run there (docs/briefs/rust-port.md, stage 3).

Each was proved against the Python it replaced, finding for finding, on the corpus before that Python was deleted
(``bin/parity/print.py``). A check the core could not judge, because it refused the file or read it with gaps, fails
here rather than passing silently: the point of the tool is to confirm a file, so an unread one must not read as OK.
"""

import json
import pathlib

from ..models import Finding, Severity
from . import _core

# The ported checks that always run; the rest of the print profile still runs in Python.
PORTED = frozenset({"outside-media", "xobject-missing"})

_SEVERITY = {"error": Severity.ERROR, "warning": Severity.WARN}


def check(path: pathlib.Path) -> list[Finding]:
    """The ported checks' findings on the PDF at *path*."""
    report = json.loads(_core.verify(str(path), path.read_bytes(), "print"))
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
