"""Verify PDFs from Python: a thin layer over the Rust core's ``proof-pdf verify``.

:func:`verify_file` runs the same command the CLI does, on one file, with the :class:`Profile` as its options and no
config file, and returns its findings. The PDF/X level, the print checks, and veraPDF (``Profile.pdfa``) and
Ghostscript (``Profile.tac``) when asked all run there. A check that cannot run, because its tool is missing or fails
or the file could not be read, is a failure, not a silent pass.
"""

import asyncio
import collections.abc
import json
import os
import pathlib
import shutil

from ._internal import _core
from .models import FileReport, Finding, Profile, Severity

# Each file may fan out to veraPDF (a JVM) and Ghostscript, so a handful in flight is plenty.
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

    The core runs in a worker thread with the GIL released, so the event loop stays free throughout. A profile the
    command refuses raises :class:`ValueError`.
    """
    argv = ["proof-pdf", "--no-config", "verify", str(path), "--json", "--verapdf", verapdf_bin, "--gs", gs_bin]
    argv += arguments(profile)
    if timeout is not None:
        argv += ["--timeout", repr(float(timeout))]
    code, out, err = await asyncio.to_thread(_core.run, argv)
    if code == 64:
        raise ValueError(err.strip())
    record = json.loads(out)[0]
    findings = [
        Finding(f["check"], Severity.ERROR if f["severity"] == "error" else Severity.WARN, f["message"], f["page"])
        for f in record.get("findings", [])
    ]
    findings += [
        Finding(c["id"], Severity.ERROR, f"could not be checked: {c['reason']}")
        for c in record.get("checks", [])
        if c["status"] == "not_evaluated"
    ]
    if record["unreadable"]:
        findings.append(Finding("structure", Severity.ERROR, record["unreadable"]))
    return FileReport(path=path, findings=findings)


def arguments(profile: Profile) -> list[str]:
    """*profile* as ``proof-pdf verify`` options."""
    number = _number
    args = ["--flavour", profile.flavour, "--colour-mode", str(profile.colour_mode)]
    flags = (
        ("--pdfa", profile.pdfa),
        ("--no-pdfx", not profile.pdfx),
        ("--tac", profile.tac),
        ("--no-colour", not profile.colour),
        ("--strict-cmyk", profile.strict_cmyk),
        ("--allow-k-only", profile.allow_k_only),
        ("--k-only-text", profile.k_only_text),
        ("--no-missing-glyphs", not profile.missing_glyphs),
        ("--no-fonts", not profile.fonts),
        ("--require-bleed", profile.require_bleed),
        ("--no-marks", profile.no_marks),
        ("--uniform-pages", profile.uniform_pages),
    )
    args += [flag for flag, on in flags if on]
    args += number("--max-tac", profile.max_tac) + number("--warn-tac", profile.warn_tac)
    args += number("--small-text-pt", profile.small_text_pt) + number("--min-dpi", profile.min_dpi)
    args += number("--warn-dpi", profile.warn_dpi) + number("--max-dpi", profile.max_dpi)
    args += number("--max-stretch-pct", profile.max_stretch_pct) + number("--min-text-pt", profile.min_text_pt)
    args += number("--min-stroke-pt", profile.min_stroke_pt) + number("--safe-mm", profile.safe_mm)
    args += number("--min-delta-l", profile.min_delta_l)
    if profile.proof_profile is not None:
        args += ["--proof-profile", str(profile.proof_profile)]
    if profile.allow_spots is not None:
        # An empty allowlist still turns the check on: no spot ink is allowed.
        args += [a for spot in (profile.allow_spots or ("",)) for a in ("--allow-spot", spot)]
    if profile.output_condition is not None:
        args += ["--output-condition", profile.output_condition]
    if profile.pages_multiple_of:
        args += ["--pages-multiple-of", str(profile.pages_multiple_of)]
    if (g := profile.geometry) is not None:
        args += ["--trim-mm", f"{g.trim_w_mm!r}x{g.trim_h_mm!r}", "--binding", g.binding]
        args += number("--bleed-mm", g.bleed_mm) + number("--tolerance-mm", g.tolerance_mm)
        args += number("--binding-bleed-mm", g.binding_bleed_mm)
    return args


def _number(flag: str, value: float | None) -> list[str]:
    """A number option, or nothing when *value* is not set."""
    return [flag, repr(float(value))] if value is not None else []


def missing_tools(profile: Profile, *, verapdf_bin: str = "verapdf", gs_bin: str = "gs") -> list[str]:
    """The external tools *profile* needs that cannot be found, by the executable names given (empty if all are there).

    veraPDF is needed for PDF/A (``pdfa``) and Ghostscript for peak ink (``tac``). :func:`verify_file` reports a
    missing tool as a failing finding; this lets a caller stop before a long run with one message of its own.
    """
    needed = ((verapdf_bin, profile.pdfa), (gs_bin, profile.tac))
    return [binary for binary, wanted in needed if wanted and shutil.which(binary) is None]


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
