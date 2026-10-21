"""The ``proof-pdf`` subcommands -- hold a print PDF to PDF/X-4 + PDF/A.

``verify`` wraps veraPDF and adds the PDF/X-4, geometry and colour checks
veraPDF cannot make. ``preview`` rasterises pages with the page-box guides drawn
on, so a geometry finding can be seen rather than only read.

Both take files or directories; a directory expands to every PDF beneath it,
minus anything matching ``--exclude`` (``core.expand_targets``).
The commands take paths only, so a file a printer sends back can be checked
the same way a freshly built one is.
"""

import dataclasses
import importlib.metadata
import json
import logging
import os
import pathlib
import typing

import typer

from . import core, preview, verapdf
from .models import (
    DEFAULT_MAX_TAC,
    DEFAULT_MIN_DPI,
    DEFAULT_SMALL_TEXT_PT,
    FileReport,
    Finding,
    PageGeometry,
    Profile,
    Severity,
)

logger = logging.getLogger(__name__)

pdf_app = typer.Typer(
    pretty_exceptions_enable=False,
    help="Verify and proof print PDFs (PDF/X-4 + PDF/A).",
    no_args_is_help=True,
)


def version_string() -> str:
    """``1.2.3``, plus ``(commit abc123)`` when the image or build recorded one in PROOFPRESS_REVISION."""
    version = importlib.metadata.version("proof-pdf")
    revision = os.environ.get("PROOFPRESS_REVISION")
    return f"{version} (commit {revision[:12]})" if revision and revision != "unknown" else version


def _show_version(value: bool) -> None:
    # An eager option callback: click would otherwise reject a bare ``--version`` as a missing command.
    if value:
        typer.echo(f"proof-pdf {version_string()}")
        raise typer.Exit()


@pdf_app.callback()
def _main(
    version: typing.Annotated[
        bool, typer.Option("--version", help="Show the version and exit.", callback=_show_version, is_eager=True)
    ] = False,
) -> None:
    pass


# A hung Ghostscript on a 400 MB file would otherwise block a CI run for ever.
DEFAULT_TIMEOUT = 900.0

_MARK = {
    Severity.ERROR: typer.style("x", fg=typer.colors.RED),
    Severity.WARN: typer.style("!", fg=typer.colors.YELLOW),
}

PathsArg = typing.Annotated[
    list[pathlib.Path] | None,
    typer.Argument(help="PDFs to process. A directory contributes every PDF beneath it."),
]
GsOpt = typing.Annotated[str, typer.Option("--gs", help="Ghostscript executable.")]
TimeoutOpt = typing.Annotated[float, typer.Option("--timeout", help="Per-tool timeout, in seconds.")]
ExcludeOpt = typing.Annotated[
    list[str] | None,
    typer.Option("--exclude", help="Glob, relative to a directory argument, of PDFs to skip. Repeatable."),
]
QuietOpt = typing.Annotated[bool, typer.Option("--quiet", help="Exit code only, for CI.")]


@pdf_app.command("verify")
def verify_cmd(
    files: PathsArg = None,
    exclude: ExcludeOpt = None,
    flavour: typing.Annotated[
        str, typer.Option("--flavour", help="veraPDF PDF/A flavour: 2b, 2a, 2u, 1b, 3b, 4...")
    ] = "2b",
    pdfa: typing.Annotated[bool, typer.Option("--pdfa/--no-pdfa", help="Run the veraPDF PDF/A layer.")] = True,
    pdfx: typing.Annotated[bool, typer.Option("--pdfx/--no-pdfx", help="Check the PDF/X-4 identity.")] = True,
    colour: typing.Annotated[
        bool, typer.Option("--colour/--no-colour", help="Check ink, overprint, RGB, rich black and image DPI.")
    ] = True,
    trim_mm: typing.Annotated[
        str | None,
        typer.Option(
            "--trim-mm", help="Expected finished size as WxH in mm, e.g. 105x148. Enables the geometry layer."
        ),
    ] = None,
    bleed_mm: typing.Annotated[float, typer.Option("--bleed-mm", help="Expected bleed per side, in mm.")] = 3.0,
    tolerance_mm: typing.Annotated[
        float, typer.Option("--tolerance-mm", help="Allowed geometry deviation, in mm.")
    ] = 0.25,
    max_tac: typing.Annotated[
        float, typer.Option("--max-tac", help="Total-ink-coverage ceiling, in percent.")
    ] = DEFAULT_MAX_TAC,
    small_text_pt: typing.Annotated[
        float, typer.Option("--small-text-pt", help="Rich-black cutoff (pt).")
    ] = DEFAULT_SMALL_TEXT_PT,
    min_dpi: typing.Annotated[
        float, typer.Option("--min-dpi", help="Minimum effective resolution for placed images.")
    ] = DEFAULT_MIN_DPI,
    strict_cmyk: typing.Annotated[
        bool, typer.Option("--strict-cmyk", help="Fail, rather than warn, on RGB vector fills.")
    ] = False,
    verapdf_bin: typing.Annotated[str, typer.Option("--verapdf", help="veraPDF executable.")] = "verapdf",
    gs_bin: GsOpt = "gs",
    timeout: TimeoutOpt = DEFAULT_TIMEOUT,
    as_json: typing.Annotated[
        bool, typer.Option("--json", help="Emit machine-readable findings instead of a report.")
    ] = False,
    quiet: QuietOpt = False,
) -> None:
    """Verify each PDF is a valid dual PDF/X-4 + PDF/A file."""
    targets = core.expand_targets(files or [], exclude or [])
    if not targets:
        typer.echo("no PDFs to verify", err=True)
        raise typer.Exit(1)

    profile = Profile(
        flavour=flavour,
        pdfa=pdfa,
        pdfx=pdfx,
        colour=colour,
        geometry=_page_geometry(trim_mm, bleed_mm, tolerance_mm) if trim_mm else None,
        max_tac=max_tac,
        small_text_pt=small_text_pt,
        min_dpi=min_dpi,
        strict_cmyk=strict_cmyk,
    )
    reports = [core.verify_file(t, profile, verapdf_bin=verapdf_bin, gs_bin=gs_bin, timeout=timeout) for t in targets]

    if as_json:
        typer.echo(json.dumps([_as_record(r) for r in reports], indent=2))
    elif not quiet:
        for report in reports:
            _print_report(report)
        _print_summary(reports, profile)
    raise typer.Exit(1 if any(not r.ok for r in reports) else 0)


@pdf_app.command("preview")
def preview_cmd(
    files: PathsArg = None,
    exclude: ExcludeOpt = None,
    dpi: typing.Annotated[int, typer.Option("--dpi", help="Raster resolution.")] = 300,
    guides: typing.Annotated[
        bool, typer.Option("--guides/--no-guides", help="Draw trim/bleed/safe guides from the page boxes.")
    ] = True,
    safe_mm: typing.Annotated[float, typer.Option("--safe", help="Safe-area inset from the trim edge, in mm.")] = 3.0,
    out: typing.Annotated[
        pathlib.Path | None,
        typer.Option("--out", help="Output directory (default: a preview/ folder beside each PDF)."),
    ] = None,
    gs_bin: GsOpt = "gs",
    timeout: TimeoutOpt = DEFAULT_TIMEOUT,
    quiet: QuietOpt = False,
) -> None:
    """Rasterise each PDF to one PNG per page, overlaying the page-box guides."""
    targets = core.expand_targets(files or [], exclude or [])
    if not targets:
        typer.echo("no PDFs to preview", err=True)
        raise typer.Exit(1)
    if not preview.gs_available(gs_bin):
        typer.echo(f"Ghostscript not found ({gs_bin}) -- cannot render previews", err=True)
        raise typer.Exit(1)

    written: list[pathlib.Path] = []
    for target in targets:
        dst = out if out is not None else target.parent / "preview"
        try:
            pngs = preview.preview_file(
                target, dst, dpi=dpi, guides=guides, safe_mm=safe_mm, gs_bin=gs_bin, timeout=timeout
            )
        except preview.GhostscriptError as exc:
            typer.secho(f"FAIL -- Ghostscript could not render {_rel(target)}: {exc}", fg=typer.colors.RED, err=True)
            raise typer.Exit(1) from exc
        written += pngs
        if not quiet:
            for png in pngs:
                typer.echo(f"wrote {_rel(png)}")
    if not quiet:
        note = " with trim/bleed/safe guides" if guides else ""
        typer.secho(f"\nOK -- {len(written)} page(s){note} from {len(targets)} file(s)", fg=typer.colors.GREEN)


def _page_geometry(trim_mm: str, bleed_mm: float, tolerance_mm: float) -> PageGeometry:
    """Parse ``--trim-mm WxH`` into a :class:`PageGeometry`."""
    parts = trim_mm.lower().replace("*", "x").split("x")
    if len(parts) != 2:
        raise typer.BadParameter(f"--trim-mm must look like 105x148, got {trim_mm!r}")
    try:
        width, height = (float(p) for p in parts)
    except ValueError as exc:
        raise typer.BadParameter(f"--trim-mm must be two numbers, got {trim_mm!r}") from exc
    if width <= 0 or height <= 0:
        raise typer.BadParameter(f"--trim-mm must be positive, got {trim_mm!r}")
    return PageGeometry(trim_w_mm=width, trim_h_mm=height, bleed_mm=bleed_mm, tolerance_mm=tolerance_mm)


def _rel(path: pathlib.Path) -> str:
    """*path* relative to the working directory when that is shorter to read."""
    try:
        return str(path.resolve().relative_to(pathlib.Path.cwd()))
    except ValueError:  # outside the working tree -- an absolute path is the clearest thing to show
        return str(path)


def _as_record(report: FileReport) -> dict:
    return {
        "path": str(report.path),
        "ok": report.ok,
        "findings": [dataclasses.asdict(f) for f in report.findings],
    }


def _print_report(report: FileReport) -> None:
    status = typer.style("OK", fg=typer.colors.GREEN) if report.ok else typer.style("FAIL", fg=typer.colors.RED)
    typer.echo(f"  {_rel(report.path):<52} {status}")
    for finding, pages in _collapse(report.findings):
        typer.echo(f"      {_MARK[finding.severity]} {_labelled(finding, pages)}")


def _collapse(findings: list[Finding]) -> list[tuple[Finding, list[int]]]:
    """Group findings that differ only by page number, preserving first-seen order.

    A job says the same thing about all fifty of its cards. Printed one line per
    page that is a hundred lines of noise burying whatever else was found, so the
    identical ones collapse into a single line naming the pages.
    """
    grouped: dict[tuple[str, Severity, str], list[int]] = {}
    first: dict[tuple[str, Severity, str], Finding] = {}
    for finding in findings:
        key = (finding.check, finding.severity, finding.message)
        first.setdefault(key, finding)
        if finding.page is not None:
            grouped.setdefault(key, []).append(finding.page)
        else:
            grouped.setdefault(key, [])
    return [(first[key], pages) for key, pages in grouped.items()]


def _labelled(finding: Finding, pages: list[int]) -> str:
    """One finding's line, with its page numbers folded into ranges."""
    if not pages:
        return finding.label()
    where = "page" if len(pages) == 1 else "pages"
    return f"[{finding.check}] ({where} {_ranges(pages)}) {finding.message}"


def _ranges(pages: list[int]) -> str:
    """``[1, 2, 3, 7]`` as ``"1-3, 7"``."""
    spans: list[tuple[int, int]] = []
    for page in sorted(set(pages)):
        if spans and page == spans[-1][1] + 1:
            spans[-1] = (spans[-1][0], page)
        else:
            spans.append((page, page))
    return ", ".join(str(lo) if lo == hi else f"{lo}-{hi}" for lo, hi in spans)


def _print_summary(reports: list[FileReport], profile: Profile) -> None:
    layers = ", ".join(
        name
        for name, on in (
            (f"PDF/A-{profile.flavour}", profile.pdfa),
            ("PDF/X-4", profile.pdfx),
            ("geometry", profile.geometry is not None),
            ("colour", profile.colour),
        )
        if on
    )
    failed = [r for r in reports if not r.ok]
    typer.echo("")
    if failed:
        typer.secho(f"FAIL -- {len(failed)} of {len(reports)} file(s) failed: {layers}", fg=typer.colors.RED)
        return
    warnings = sum(len(r.warnings) for r in reports)
    note = f" ({warnings} advisory warning(s))" if warnings else ""
    typer.secho(f"OK -- all {len(reports)} file(s) passed: {layers}{note}", fg=typer.colors.GREEN)


# Re-exported so callers can special-case a genuine tool failure if they wish.
VeraPdfError = verapdf.VeraPdfError
