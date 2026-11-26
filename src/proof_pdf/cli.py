"""The ``proof-pdf`` subcommands -- hold a print PDF to PDF/X-4 + PDF/A.

``verify`` wraps veraPDF and adds the PDF/X-4, geometry and colour checks
veraPDF cannot make. ``preview`` rasterises pages with the page-box guides drawn
on, so a geometry finding can be seen rather than only read.

Both take files or directories; a directory expands to every PDF beneath it,
minus anything matching ``--exclude`` (``core.expand_targets``).
The commands take paths only, so a file a printer sends back can be checked
the same way a freshly built one is.
"""

import asyncio
import collections
import dataclasses
import importlib.metadata
import json
import logging
import os
import pathlib
import sys
import typing

import pikepdf
import typer

from . import core, crop, extract, inventory, preview, texlog
from ._internal import (
    _core,
    config as config_file,
)
from .models import (
    FileReport,
    Finding,
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
    ctx: typer.Context,
    version: typing.Annotated[
        bool, typer.Option("--version", help="Show the version and exit.", callback=_show_version, is_eager=True)
    ] = False,
    config: typing.Annotated[
        pathlib.Path | None,
        typer.Option(
            "--config",
            envvar="PROOF_PDF_CONFIG",
            help="Defaults file (default: the nearest proof-pdf.toml, or [tool.proof-pdf] in pyproject.toml).",
        ),
    ] = None,
    profile: typing.Annotated[
        str | None,
        typer.Option("--profile", envvar="PROOF_PDF_PROFILE", help="Named profile in the config file, e.g. a printer."),
    ] = None,
) -> None:
    """Read the config file, if any, into the defaults of the command about to run."""
    path = config or config_file.find_config(pathlib.Path.cwd())
    if path is None:
        if profile:
            raise typer.BadParameter("--profile needs a config file, and none was found", param_hint="--profile")
        return
    commands = getattr(ctx.command, "commands", {})
    try:
        ctx.default_map = config_file.default_map(path, profile, commands)
    except config_file.ConfigError as exc:
        raise typer.BadParameter(str(exc), param_hint="--config") from exc


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


@pdf_app.command(
    "verify",
    context_settings={"allow_extra_args": True, "ignore_unknown_options": True, "help_option_names": []},
    add_help_option=False,
)
def verify_cmd(ctx: typer.Context) -> None:
    """Verify each PDF: its PDF/X level, the print checks and, when asked, PDF/A and peak ink (see verify --help).

    The command is the Rust core's: its options, config file and reports are documented by ``proof-pdf verify --help``.
    """
    params = ctx.parent.params if ctx.parent else {}
    argv = ["proof-pdf"]
    if params.get("config"):
        argv += ["--config", str(params["config"])]
    if params.get("profile"):
        argv += ["--profile", params["profile"]]
    code, out, err = _core.run([*argv, "verify", *ctx.args], sys.stdout.isatty())
    typer.echo(out, nl=False)
    typer.echo(err, nl=False, err=True)
    raise typer.Exit(code)


@pdf_app.command("inspect")
def inspect_cmd(
    files: PathsArg = None,
    exclude: ExcludeOpt = None,
    proof_profile: typing.Annotated[
        pathlib.Path | None,
        typer.Option(
            "--proof-profile", help="ICC profile to measure contrast through (default: the PDF's output intent)."
        ),
    ] = None,
    contrast: typing.Annotated[
        bool, typer.Option("--contrast/--no-contrast", help="Measure text/background lightness differences.")
    ] = True,
    as_json: typing.Annotated[bool, typer.Option("--json", help="Emit the inventory as JSON.")] = False,
) -> None:
    """Report what is in each PDF (fonts, text colours and sizes, inks, images, contrast) without judging it."""
    targets = core.expand_targets(files or [], exclude or [])
    if not targets:
        typer.echo("no PDFs to inspect", err=True)
        raise typer.Exit(1)
    failed = False
    records: list[dict] = []
    for target in targets:
        try:
            facts = asyncio.run(inventory.inspect_file(target, proof_profile=proof_profile, with_contrast=contrast))
        except (pikepdf.PdfError, pikepdf.PasswordError) as exc:
            typer.secho(f"FAIL -- cannot open {_rel(target)}: {exc}", fg=typer.colors.RED, err=True)
            failed = True
            continue
        records.append(facts.to_dict())
        if not as_json:
            _print_inventory(facts)
    if as_json:
        typer.echo(json.dumps(records, indent=2))
    raise typer.Exit(1 if failed else 0)


@pdf_app.command("extract")
def extract_cmd(
    files: PathsArg = None,
    exclude: ExcludeOpt = None,
    out: typing.Annotated[
        pathlib.Path | None,
        typer.Option("--out", help="Output directory; each PDF gets a folder of its name inside (default: beside it)."),
    ] = None,
    kind: typing.Annotated[
        list[str] | None,
        typer.Option(
            "--kind",
            help=f"What to recover: {', '.join(extract.KINDS)}. Repeatable (default: all). Tables need vector.",
        ),
    ] = None,
    gap_pt: typing.Annotated[
        float, typer.Option("--gap-pt", help="Vector shapes closer than this (pt) are one piece of artwork.")
    ] = extract.DEFAULT_GAP_PT,
    max_fraction: typing.Annotated[
        float,
        typer.Option(
            "--max-fraction", help="Vector shapes covering more of the page than this are taken for backgrounds."
        ),
    ] = extract.DEFAULT_MAX_FRACTION,
    vector_text: typing.Annotated[
        bool,
        typer.Option("--vector-text/--no-vector-text", help="Draw text lying on a piece of artwork into the piece."),
    ] = True,
    previews: typing.Annotated[
        bool,
        typer.Option(
            "--previews", help="Also write previews/index.html: the pages with every asset boxed (Ghostscript)."
        ),
    ] = False,
    preview_dpi: typing.Annotated[
        int, typer.Option("--preview-dpi", help="Resolution of the page renders in the review page.")
    ] = extract.DEFAULT_PREVIEW_DPI,
    gs_bin: GsOpt = "gs",
    quiet: QuietOpt = False,
) -> None:
    """Recover images, fonts, ICC profiles and vector artwork from each PDF, with a document.json saying where."""
    unknown = sorted(set(kind or []) - set(extract.KINDS))
    if unknown:
        raise typer.BadParameter(f"unknown kind {', '.join(unknown)}; choose from {', '.join(extract.KINDS)}")
    targets = core.expand_targets(files or [], exclude or [])
    if not targets:
        typer.echo("no PDFs to extract from", err=True)
        raise typer.Exit(1)
    if previews and not preview.gs_available(gs_bin):
        typer.echo(f"Ghostscript not found ({gs_bin}) -- cannot render previews", err=True)
        raise typer.Exit(1)
    failed = False
    for target in targets:
        dst = (out if out is not None else target.parent) / target.stem
        try:
            result = extract.extract_pdf(
                target,
                dst,
                kinds=kind or extract.KINDS,
                gap_pt=gap_pt,
                max_fraction=max_fraction,
                vector_text=vector_text,
                previews=previews,
                preview_dpi=preview_dpi,
                gs_bin=gs_bin,
            )
        except preview.GhostscriptError as exc:
            typer.secho(f"FAIL -- Ghostscript could not render {_rel(target)}: {exc}", fg=typer.colors.RED, err=True)
            failed = True
            continue
        except (pikepdf.PdfError, pikepdf.PasswordError) as exc:
            typer.secho(f"FAIL -- cannot open {_rel(target)}: {exc}", fg=typer.colors.RED, err=True)
            failed = True
            continue
        if not quiet:
            _print_extraction(target, dst, result)
    raise typer.Exit(1 if failed else 0)


@pdf_app.command("crop")
def crop_cmd(
    file: typing.Annotated[pathlib.Path, typer.Argument(help="The PDF to cut from.")],
    page: typing.Annotated[int, typer.Option("--page", min=1, help="Page number, from 1.")],
    box: typing.Annotated[
        str,
        typer.Option(
            "--box", help="X0,Y0,X1,Y1 in points from the page's bottom-left, as document.json gives placements."
        ),
    ],
    pad_pt: typing.Annotated[
        float, typer.Option("--pad-pt", min=0, help="Widen the box by this much on every side.")
    ] = 0.0,
    out: typing.Annotated[
        pathlib.Path | None, typer.Option("--out", help="Output PDF (default: beside the file).")
    ] = None,
) -> None:
    """Cut a rectangle of one page out as a one-page PDF: an illustration the extraction missed."""
    try:
        x0, y0, x1, y1 = (float(v) for v in box.split(","))
    except ValueError as exc:
        raise typer.BadParameter(f"--box must be four numbers, X0,Y0,X1,Y1; got {box!r}", param_hint="--box") from exc
    target = out or file.with_name(f"{file.stem}-p{page}-{round(min(x0, x1))}-{round(min(y0, y1))}.pdf")
    try:
        crop.crop_page(file, page, (x0, y0, x1, y1), target, pad_pt=pad_pt)
    except ValueError as exc:
        typer.secho(f"FAIL -- {exc}", fg=typer.colors.RED, err=True)
        raise typer.Exit(1) from exc
    except crop.UnreadablePdfError as exc:
        typer.secho(f"FAIL -- cannot open {_rel(file)}: {exc}", fg=typer.colors.RED, err=True)
        raise typer.Exit(1) from exc
    typer.echo(f"wrote {_rel(target)}")


def _print_extraction(target: pathlib.Path, dst: pathlib.Path, result: extract.Extraction) -> None:
    counts = collections.Counter(a.kind for a in result.assets)
    found = ", ".join(f"{n} {k}" for k, n in sorted(counts.items())) or "no assets"
    typer.echo(f"{_rel(target)} -> {_rel(dst)}/  ({found}, {len(result.lines)} text line(s))")
    for message in (*result.notes, *result.failures):
        typer.secho(f"  ! {message}", fg=typer.colors.YELLOW)
    if result.previews:
        typer.echo(f"  review: {_rel(dst / result.previews)}")


@pdf_app.command("texlog")
def texlog_cmd(
    logs: typing.Annotated[list[pathlib.Path] | None, typer.Argument(help="LaTeX .log files to lint.")] = None,
    max_overfull_pt: typing.Annotated[
        float | None,
        typer.Option(
            "--max-overfull-pt", help="Fail an overfull \\hbox wider than this (pt). Otherwise only a warning."
        ),
    ] = None,
    as_json: typing.Annotated[bool, typer.Option("--json", help="Emit machine-readable findings.")] = False,
    quiet: QuietOpt = False,
) -> None:
    """Lint a LaTeX log: dropped characters, TeX errors, undefined references, substituted fonts, overfull boxes."""
    if not logs:
        typer.echo("no logs to lint", err=True)
        raise typer.Exit(1)
    reports = [FileReport(path=log, findings=texlog.check_log(log, max_overfull_pt=max_overfull_pt)) for log in logs]
    if as_json:
        typer.echo(json.dumps([_as_record(r) for r in reports], indent=2))
    elif not quiet:
        for report in reports:
            _print_report(report)
        failed = sum(not r.ok for r in reports)
        typer.echo("")
        if failed:
            typer.secho(f"FAIL -- {failed} of {len(reports)} log(s) have errors", fg=typer.colors.RED)
        else:
            typer.secho(f"OK -- {len(reports)} log(s) clean of errors", fg=typer.colors.GREEN)
    raise typer.Exit(1 if any(not r.ok for r in reports) else 0)


@pdf_app.command("preview")
def preview_cmd(
    files: PathsArg = None,
    exclude: ExcludeOpt = None,
    dpi: typing.Annotated[int, typer.Option("--dpi", help="Raster resolution.")] = 300,
    guides: typing.Annotated[
        bool, typer.Option("--guides/--no-guides", help="Draw trim/bleed/safe guides from the page boxes.")
    ] = True,
    safe_mm: typing.Annotated[
        float | None,
        typer.Option("--safe", help="Also draw a safe-area guide this far inside the trim edge, in mm. No default."),
    ] = None,
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
            pngs = asyncio.run(
                preview.preview_file(
                    target, dst, dpi=dpi, guides=guides, safe_mm=safe_mm, gs_bin=gs_bin, timeout=timeout
                )
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


def _print_inventory(facts: inventory.Inventory) -> None:
    pages = len(facts.pages)
    typer.echo(f"{_rel(pathlib.Path(facts.path))}  ({pages} page{'s' if pages != 1 else ''})")
    for page in facts.pages:
        size = f"trim {page.trim_mm[0]:g}x{page.trim_mm[1]:g}mm" if page.trim_mm else "no TrimBox"
        text = f", {page.text_runs} text run(s), smallest {page.smallest_text_pt:.1f}pt" if page.text_runs else ""
        line = f", thinnest line {page.thinnest_stroke_pt:g}pt" if page.thinnest_stroke_pt is not None else ""
        typer.echo(f"  page {page.number}: {size}{text}{line}")
    for font in facts.fonts:
        flags = ("embedded" if font.embedded else "NOT embedded") + ("" if font.has_to_unicode else ", no ToUnicode")
        notdef = f", {font.notdef} .notdef" if font.notdef else ""
        typer.echo(f"  font {font.name} ({font.subtype}, {flags}): {font.glyphs} glyph(s){notdef}")
    for colour in facts.text_colours:
        typer.echo(f"  text colour {colour.colour}: {colour.runs} run(s), smallest {colour.smallest_pt:.1f}pt")
    if facts.spot_inks:
        typer.echo(f"  spot inks: {', '.join(facts.spot_inks)}")
    for image in facts.images:
        dpi = f"{image.dpi:g} dpi" if image.dpi is not None else "unknown dpi"
        typer.echo(f"  image {image.name} {image.colour_space} {image.pixels[0]}x{image.pixels[1]}px, {dpi}")
    for pair in facts.contrast:
        measure = f"L* diff {pair.delta_l:g}, dE2000 {pair.delta_e:g}" if pair.delta_l is not None else "not measured"
        typer.echo(f"  contrast {pair.text} on {pair.background}: {measure} ({pair.runs} run(s))")
    if facts.contrast_error:
        typer.echo(f"  contrast: not measured ({facts.contrast_error})")


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
        "exempt": report.exempt,
        "findings": [dataclasses.asdict(f) for f in report.findings],
    }


def _print_report(report: FileReport) -> None:
    if report.exempt is not None:
        typer.echo(f"  {_rel(report.path):<52} {typer.style('EXEMPT', fg=typer.colors.YELLOW)}")
        typer.echo(f"      - {report.exempt}")
        return
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
