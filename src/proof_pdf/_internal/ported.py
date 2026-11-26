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
        "blend-space",
        "spot-ink",
        "colour-space",
        "rgb-only",
        "icc-profile",
    }
)
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
    cmyk, rgb = profile.colour_mode is ColourMode.CMYK, profile.colour_mode is ColourMode.RGB
    switches = (
        (True, ("outside-media", "xobject-missing", "page-count")),
        (profile.colour and cmyk, ("overprint", "white-overprint", "rich-black", "colour-space")),
        (profile.colour and rgb, ("rgb-only",)),
        (profile.colour, ("blend-space", "icc-profile")),
        (profile.k_only_text, ("k-only-text",)),
        (profile.geometry is not None, ("geometry", "bleed", "centring")),
        (profile.uniform_pages, ("page-size", "page-rotate")),
        (profile.output_condition is not None, ("output-condition",)),
        (profile.require_bleed, ("artwork-bleed",)),
        (profile.no_marks, ("outside-bleed",)),
        (profile.allow_spots is not None, ("spot-ink",)),
    )
    wanted = {check for on, checks in switches if on for check in checks}
    return [f for f in run(path, params(profile)) if f.check in wanted]


def params(profile: Profile) -> dict[str, object]:
    """The print checks' parameters *profile* sets, as the core takes them."""
    return {
        "small_text_pt": profile.small_text_pt,
        "geometry": dataclasses.asdict(profile.geometry) if profile.geometry is not None else None,
        "uniform_pages": profile.uniform_pages,
        "pages_multiple_of": profile.pages_multiple_of or None,
        "output_condition": profile.output_condition,
        "allow_spots": list(profile.allow_spots) if profile.allow_spots is not None else None,
        "strict_cmyk": profile.strict_cmyk,
        "colour_mode": str(profile.colour_mode),
    }
