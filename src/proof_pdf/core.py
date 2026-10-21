"""Orchestrate a full verification of one PDF.

Runs the veraPDF PDF/A layer, then -- when the profile asks for them -- the
PDF/X-4 structural checks, the finished-card geometry, and the colour-sanity
checks, collecting everything into one :class:`FileReport`.

A check that cannot run -- its external tool is missing or errors -- is a
failure, not a silent pass: the point of the tool is to *confirm* conformance, so
an unconfirmable file must not read as OK.
"""

import asyncio
import collections.abc
import logging
import os
import pathlib

import pikepdf

from . import colour, contrast, resolution, structure, text, verapdf
from .models import FileReport, Finding, Profile, Severity

logger = logging.getLogger(__name__)

# Each file already fans out to veraPDF (a JVM) and Ghostscript, so a handful in flight is plenty.
DEFAULT_JOBS = min(4, os.cpu_count() or 1)


async def verify_file(
    path: pathlib.Path,
    profile: Profile,
    *,
    verapdf_bin: str = "verapdf",
    gs_bin: str = "gs",
    timeout: float | None = None,
) -> FileReport:
    """Verify *path* against *profile* and return the collected findings.

    veraPDF, the Ghostscript ink scan and the in-process PDF checks run concurrently; the external tools run as
    subprocesses, and the pikepdf checks in a worker thread, so the event loop stays free throughout.
    """
    report = FileReport(path=path)
    vera = asyncio.create_task(_run_verapdf(path, profile, verapdf_bin, timeout)) if profile.pdfa else None
    tac = asyncio.create_task(_run_tac(path, profile, gs_bin, timeout)) if profile.colour else None
    try:
        structural, opened = await asyncio.to_thread(_check_pdf, path, profile)
        if vera:
            report.findings += await vera
        report.findings += structural
        if opened and tac:
            report.findings += await tac
    finally:
        for task in (vera, tac):
            if task and not task.done():
                task.cancel()
    return report


def exemption(path: pathlib.PurePath, exemptions: collections.abc.Mapping[str, str]) -> str | None:
    """The reason *path* was declared exempt, or ``None``.

    Each key is a glob matched against the path from any directory level, so
    ``*_preview.pdf`` and ``res/**`` work without spelling out the whole path
    (:meth:`pathlib.PurePath.full_match`). The first matching glob wins.
    """
    for glob, reason in exemptions.items():
        if path.full_match(glob) or path.full_match(f"**/{glob}"):
            return reason
    return None


async def verify_files(
    paths: collections.abc.Iterable[pathlib.Path],
    profile: Profile,
    *,
    exemptions: collections.abc.Mapping[str, str] | None = None,
    jobs: int = DEFAULT_JOBS,
    verapdf_bin: str = "verapdf",
    gs_bin: str = "gs",
    timeout: float | None = None,
) -> list[FileReport]:
    """Verify *paths*, at most *jobs* files at a time, and return the reports in input order.

    A path matching an *exemptions* glob is not checked: its report carries the
    reason and counts as passing. That is a declaration, never inferred from what
    the file contains, so a file that merely forgot its metadata still fails.
    """
    gate = asyncio.Semaphore(jobs)

    async def one(path: pathlib.Path) -> FileReport:
        if (reason := exemption(path, exemptions or {})) is not None:
            return FileReport(path=path, exempt=reason)
        async with gate:
            return await verify_file(path, profile, verapdf_bin=verapdf_bin, gs_bin=gs_bin, timeout=timeout)

    return await asyncio.gather(*(one(p) for p in paths))


def _check_pdf(path: pathlib.Path, profile: Profile) -> tuple[list[Finding], bool]:
    """The checks that read the PDF's object graph (CPU-bound, so run in a thread); False if it would not open."""
    findings: list[Finding] = []
    try:
        with pikepdf.open(path) as pdf:
            if profile.pdfx:
                findings += structure.check_page_boxes(pdf)
                findings += structure.check_trapped(pdf)
                findings += structure.check_pdfx_identity(pdf)
            if profile.geometry is not None:
                findings += structure.check_geometry(pdf, profile.geometry)
            if profile.k_only_text:
                findings += colour.check_k_only_text(pdf)
            findings += _text_checks(pdf, profile)
            if profile.colour:
                findings += colour.check_overprint(pdf)
                findings += colour.check_rich_black(pdf, profile.small_text_pt)
                findings += colour.check_colour_spaces(pdf, strict=profile.strict_cmyk)
                findings += resolution.check_image_dpi(pdf, profile.min_dpi)
    except pikepdf.PdfError as exc:  # corrupt, truncated or encrypted -- a FAIL, not a crash
        return [Finding("structure", Severity.ERROR, f"cannot open the PDF (corrupt or encrypted): {exc}")], False
    return findings, True


def _text_checks(pdf: pikepdf.Pdf, profile: Profile) -> list[Finding]:
    findings: list[Finding] = []
    if profile.missing_glyphs:
        findings += text.check_missing_glyphs(pdf)
    if profile.min_text_pt is not None:
        findings += text.check_min_text_size(pdf, profile.min_text_pt)
    if profile.min_stroke_pt is not None:
        findings += text.check_min_stroke(pdf, profile.min_stroke_pt)
    if profile.safe_mm is not None:
        findings += text.check_safe_area(pdf, profile.safe_mm)
    if profile.min_delta_l is not None:
        findings += contrast.check_contrast(pdf, profile.min_delta_l, profile.proof_profile)
    return findings


async def _run_verapdf(path: pathlib.Path, profile: Profile, binary: str, timeout: float | None) -> list[Finding]:
    if not verapdf.available(binary):
        return [
            Finding("pdfa", Severity.ERROR, f"veraPDF not found ({binary}) -- cannot confirm PDF/A-{profile.flavour}")
        ]
    try:
        stdout = await verapdf.run(binary, profile.flavour, str(path), timeout=timeout)
        return verapdf.parse(stdout, profile.flavour)
    except verapdf.VeraPdfError as exc:
        return [Finding("pdfa", Severity.ERROR, f"veraPDF could not validate the file: {exc}")]


async def _run_tac(path: pathlib.Path, profile: Profile, binary: str, timeout: float | None) -> list[Finding]:
    if not colour.gs_available(binary):
        return [Finding("tac", Severity.ERROR, f"Ghostscript not found ({binary}) -- cannot measure ink coverage")]
    try:
        return await colour.check_tac(binary, str(path), profile.max_tac, timeout=timeout)
    except colour.GhostscriptError as exc:
        return [Finding("tac", Severity.ERROR, f"Ghostscript could not rasterise the file: {exc}")]


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
