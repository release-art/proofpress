"""Orchestrate a full verification of one PDF.

Runs the veraPDF PDF/A layer, then -- when the profile asks for them -- the
PDF/X-4 structural checks, the finished-card geometry, and the colour-sanity
checks, collecting everything into one :class:`FileReport`.

A check that cannot run -- its external tool is missing or errors -- is a
failure, not a silent pass: the point of the tool is to *confirm* conformance, so
an unconfirmable file must not read as OK.
"""

import collections.abc
import logging
import pathlib

import pikepdf

from . import colour, resolution, structure, verapdf
from .models import FileReport, Finding, Profile, Severity

logger = logging.getLogger(__name__)


def verify_file(
    path: pathlib.Path,
    profile: Profile,
    *,
    verapdf_bin: str = "verapdf",
    gs_bin: str = "gs",
    timeout: float | None = None,
) -> FileReport:
    """Verify *path* against *profile* and return the collected findings."""
    report = FileReport(path=path)
    if profile.pdfa:
        _run_verapdf(report, profile, verapdf_bin, timeout)
    try:
        with pikepdf.open(path) as pdf:
            if profile.pdfx:
                report.findings += structure.check_page_boxes(pdf)
                report.findings += structure.check_trapped(pdf)
                report.findings += structure.check_pdfx_identity(pdf)
            if profile.geometry is not None:
                report.findings += structure.check_geometry(pdf, profile.geometry)
            if profile.colour:
                report.findings += colour.check_overprint(pdf)
                report.findings += colour.check_rich_black(pdf, profile.small_text_pt)
                report.findings += colour.check_colour_spaces(pdf, strict=profile.strict_cmyk)
                report.findings += resolution.check_image_dpi(pdf, profile.min_dpi)
    except pikepdf.PdfError as exc:  # corrupt, truncated or encrypted -- a FAIL, not a crash
        report.findings.append(
            Finding("structure", Severity.ERROR, f"cannot open the PDF (corrupt or encrypted): {exc}")
        )
        return report
    if profile.colour:
        _run_tac(report, profile, gs_bin, path, timeout)
    return report


def _run_verapdf(report: FileReport, profile: Profile, binary: str, timeout: float | None) -> None:
    if not verapdf.available(binary):
        report.findings.append(
            Finding("pdfa", Severity.ERROR, f"veraPDF not found ({binary}) -- cannot confirm PDF/A-{profile.flavour}")
        )
        return
    try:
        stdout = verapdf.run(binary, profile.flavour, str(report.path), timeout=timeout)
        report.findings += verapdf.parse(stdout, profile.flavour)
    except verapdf.VeraPdfError as exc:
        report.findings.append(Finding("pdfa", Severity.ERROR, f"veraPDF could not validate the file: {exc}"))


def _run_tac(report: FileReport, profile: Profile, binary: str, path: pathlib.Path, timeout: float | None) -> None:
    if not colour.gs_available(binary):
        report.findings.append(
            Finding("tac", Severity.ERROR, f"Ghostscript not found ({binary}) -- cannot measure ink coverage")
        )
        return
    try:
        report.findings += colour.check_tac(binary, str(path), profile.max_tac, timeout=timeout)
    except colour.GhostscriptError as exc:
        report.findings.append(Finding("tac", Severity.ERROR, f"Ghostscript could not rasterise the file: {exc}"))


def expand_targets(
    paths: collections.abc.Iterable[pathlib.Path], exclude: collections.abc.Iterable[str] = ()
) -> list[pathlib.Path]:
    """Resolve the CLI's file-or-directory arguments to a sorted list of PDFs.

    A directory contributes every PDF under it, recursively, except those whose
    path relative to that directory matches an *exclude* glob (``**/res/**``,
    ``*_preview.pdf``; see :meth:`pathlib.PurePath.full_match`). A file named
    directly is taken as given, even if it is not named ``.pdf`` or matches an
    exclude.
    """
    patterns = tuple(exclude)
    targets: list[pathlib.Path] = []
    for path in paths:
        if path.is_dir():
            targets += sorted(
                p
                for p in path.glob("**/*.pdf")
                if p.is_file() and not any(p.relative_to(path).full_match(g) for g in patterns)
            )
        else:
            targets.append(path)
    # Preserve first-seen order while dropping the duplicates a caller creates by
    # naming both a directory and a file inside it.
    return list(dict.fromkeys(targets))
