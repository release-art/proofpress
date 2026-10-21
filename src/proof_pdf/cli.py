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
import dataclasses
import importlib.metadata
import json
import logging
import os
import pathlib
import typing

import pikepdf
import typer

from . import (
    config as config_file,
    core,
    inventory,
    preview,
    texlog,
    verapdf,
)
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
ExemptOpt = typing.Annotated[
    list[str] | None,
    typer.Option(
        "--exempt",
        help="GLOB=reason: skip matching PDFs, list them with the reason. Matches at any directory level.",
    ),
]
ExcludeOpt = typing.Annotated[
    list[str] | None,
    typer.Option("--exclude", help="Glob, relative to a directory argument, of PDFs to skip. Repeatable."),
]
QuietOpt = typing.Annotated[bool, typer.Option("--quiet", help="Exit code only, for CI.")]


@pdf_app.command("verify")
def verify_cmd(
    files: PathsArg = None,
    exclude: ExcludeOpt = None,
    exempt: ExemptOpt = None,
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
    k_only_text: typing.Annotated[
        bool, typer.Option("--k-only-text", help="Fail text painted with any ink but K (house rule; off by default).")
    ] = False,
    missing_glyphs: typing.Annotated[
        bool, typer.Option("--missing-glyphs/--no-missing-glyphs", help="Flag characters shown with the .notdef glyph.")
    ] = True,
    min_text_pt: typing.Annotated[
        float | None, typer.Option("--min-text-pt", help="Fail text set smaller than this (pt). No default.")
    ] = None,
    min_stroke_pt: typing.Annotated[
        float | None, typer.Option("--min-stroke-pt", help="Fail lines thinner than this (pt). No default.")
    ] = None,
    safe_mm: typing.Annotated[
        float | None,
        typer.Option("--safe-mm", help="Fail text outside this inset from the trim edge, in mm. No default."),
    ] = None,
    fonts: typing.Annotated[
        bool, typer.Option("--fonts/--no-fonts", help="Flag fonts that are not embedded, and Type 3 fonts.")
    ] = True,
    allow_spot: typing.Annotated[
        list[str] | None,
        typer.Option(
            "--allow-spot", help="Fail any spot ink not named here (exact match; repeatable; 'All' needs naming)."
        ),
    ] = None,
    output_condition: typing.Annotated[
        str | None,
        typer.Option(
            "--output-condition", help="Fail unless an output intent names this printing condition, e.g. FOGRA51."
        ),
    ] = None,
    require_bleed: typing.Annotated[
        bool,
        typer.Option(
            "--require-bleed", help="Fail artwork that reaches the trim edge but stops short of the bleed edge."
        ),
    ] = False,
    uniform_pages: typing.Annotated[
        bool, typer.Option("--uniform-pages", help="Fail pages whose finished size differs from the first page's.")
    ] = False,
    pages_multiple_of: typing.Annotated[
        int | None, typer.Option("--pages-multiple-of", min=1, help="Fail a page count that is not a multiple of N.")
    ] = None,
    min_delta_l: typing.Annotated[
        float | None,
        typer.Option(
            "--min-delta-l", help="Fail text whose L* difference from its background is below this. No default."
        ),
    ] = None,
    proof_profile: typing.Annotated[
        pathlib.Path | None,
        typer.Option(
            "--proof-profile", help="ICC profile to measure contrast through (default: the PDF's output intent)."
        ),
    ] = None,
    jobs: typing.Annotated[
        int, typer.Option("--jobs", "-j", min=1, help="Files to verify at once.")
    ] = core.DEFAULT_JOBS,
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
        k_only_text=k_only_text,
        missing_glyphs=missing_glyphs,
        min_text_pt=min_text_pt,
        min_stroke_pt=min_stroke_pt,
        safe_mm=safe_mm,
        min_delta_l=min_delta_l,
        proof_profile=proof_profile,
        fonts=fonts,
        allow_spots=tuple(allow_spot) if allow_spot else None,
        output_condition=output_condition,
        require_bleed=require_bleed,
        uniform_pages=uniform_pages,
        pages_multiple_of=pages_multiple_of,
    )
    reports = asyncio.run(
        core.verify_files(
            targets,
            profile,
            exemptions=_parse_exemptions(exempt or []),
            jobs=jobs,
            verapdf_bin=verapdf_bin,
            gs_bin=gs_bin,
            timeout=timeout,
        )
    )

    if as_json:
        typer.echo(json.dumps([_as_record(r) for r in reports], indent=2))
    elif not quiet:
        for report in reports:
            _print_report(report)
        _print_summary(reports, profile)
    raise typer.Exit(1 if any(not r.ok for r in reports) else 0)


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
        except pikepdf.PdfError as exc:
            typer.secho(f"FAIL -- cannot open {_rel(target)}: {exc}", fg=typer.colors.RED, err=True)
            failed = True
            continue
        records.append(facts.to_dict())
        if not as_json:
            _print_inventory(facts)
    if as_json:
        typer.echo(json.dumps(records, indent=2))
    raise typer.Exit(1 if failed else 0)


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


def _parse_exemptions(specs: list[str]) -> dict[str, str]:
    """``GLOB=reason`` options as a mapping; a missing reason is rejected, since the reason is the point."""
    exemptions: dict[str, str] = {}
    for spec in specs:
        glob, _, reason = spec.partition("=")
        if not glob.strip() or not reason.strip():
            raise typer.BadParameter(f"--exempt must look like GLOB=reason, got {spec!r}")
        exemptions[glob.strip()] = reason.strip()
    return exemptions


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


def _print_summary(reports: list[FileReport], profile: Profile) -> None:
    layers = ", ".join(
        name
        for name, on in (
            (f"PDF/A-{profile.flavour}", profile.pdfa),
            ("PDF/X-4", profile.pdfx),
            ("geometry", profile.geometry is not None),
            ("colour", profile.colour),
            ("K-only text", profile.k_only_text),
            ("missing glyphs", profile.missing_glyphs),
            ("min text size", profile.min_text_pt is not None),
            ("hairlines", profile.min_stroke_pt is not None),
            ("safe area", profile.safe_mm is not None),
            ("contrast", profile.min_delta_l is not None),
            ("fonts", profile.fonts),
            ("spot inks", profile.allow_spots is not None),
            ("output condition", profile.output_condition is not None),
            ("artwork bleed", profile.require_bleed),
            ("page set", profile.uniform_pages or bool(profile.pages_multiple_of)),
        )
        if on
    )
    failed = [r for r in reports if not r.ok]
    exempt = [r for r in reports if r.exempt is not None]
    checked = len(reports) - len(exempt)
    typer.echo("")
    if failed:
        typer.secho(f"FAIL -- {len(failed)} of {checked} file(s) failed: {layers}", fg=typer.colors.RED)
        return
    warnings = sum(len(r.warnings) for r in reports)
    notes = ([f"{warnings} advisory warning(s)"] if warnings else []) + ([f"{len(exempt)} exempt"] if exempt else [])
    note = f" ({', '.join(notes)})" if notes else ""
    typer.secho(f"OK -- all {checked} file(s) passed: {layers}{note}", fg=typer.colors.GREEN)


# Re-exported so callers can special-case a genuine tool failure if they wish.
VeraPdfError = verapdf.VeraPdfError
