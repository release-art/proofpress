# proofpress

[![CI](https://github.com/release-art/proofpress/actions/workflows/ci.yml/badge.svg)](https://github.com/release-art/proofpress/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/proof-pdf?logo=python&color=41bb13)](https://pypi.org/project/proof-pdf)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Open-source pieces of a print-on-demand pipeline, extracted from production use.

| Piece | What it is |
|---|---|
| [`proof-pdf`](#proof-pdf) | CLI and library that hold a print PDF to PDF/X-4 and PDF/A. |
| [`releaseart/lualatex`](#docker-images) | TeX Live 2025 with LuaLaTeX. |
| [`releaseart/lualatex-python`](#docker-images) | Python, `lualatex`, Ghostscript and veraPDF: everything `proof-pdf` needs, without it. A base to `pip install` it on. |
| [`releaseart/full`](#docker-images) | `lualatex` plus Ghostscript, veraPDF and `proof-pdf`. |

## proof-pdf

`verify` runs four layers, each of which can be switched off:

- **PDF/A** with [veraPDF](https://verapdf.org) (`--flavour 2b` by default; `--no-pdfa` to skip).
- **PDF/X-4 structure**: page boxes, `/Trapped`, the `GTS_PDFX` output intent and the XMP version key (`--no-pdfx`).
- **Geometry**: trim and bleed against the finished size you expect. Runs only when `--trim-mm` is given (`--bleed-mm`, `--tolerance-mm`).
- **Colour**: total ink coverage (`--max-tac`, 300% default), stray RGB (`--strict-cmyk`), effective image DPI (`--min-dpi`, 300 default),
  overprint and rich-black small text (`--no-colour`). Overprinting 100% K text is fine; overprint on anything else is flagged,
  and **white that overprints is an error** (it adds no ink, so it vanishes on press). A page that uses transparency must blend
  in the output intent's colour space (a mismatch is an error, an undeclared space a warning).
- **K-only text** (opt-in, `--k-only-text`): text painted with any ink but K (a four-colour build, RGB, a spot or a pattern) is an error.
- **Proofing** (the text and lines a page paints, measured in page points; see below).

`inspect` reports what is in a PDF without judging it, and `texlog` lints a LaTeX build log. `preview` rasterises each page and draws the trim, bleed and safe guides from the PDF's own boxes, so a geometry finding can be seen.

```zsh
pip install proof-pdf          # Python 3.15; needs verapdf and gs (Ghostscript) on PATH

proof-pdf verify job/ --trim-mm 105x148 --bleed-mm 3 --exclude '*_preview.pdf'
proof-pdf verify job/ --exempt 'manual.pdf=internal docs, never printed'   # listed with the reason, never fails
proof-pdf verify job/ --k-only-text       # house rule: text is K-only
proof-pdf verify card.pdf --json          # machine-readable findings, for CI
proof-pdf preview card.pdf --dpi 150
proof-pdf --version                       # version, plus the commit when built from an image
```

- Arguments are files or directories; a directory expands to every PDF beneath it, minus `--exclude` globs (relative to that directory).
- `--exempt GLOB=reason` declares a file out of scope. It is still listed, with the reason, and never fails; the glob matches at any
  directory level (`res/**`, `*_preview.pdf`). Unlike `--exclude`, which drops files silently, an exemption stays visible in CI output.
- **A check whose tool is missing fails, it is never skipped**: a file that can't be confirmed must not read as OK.
- A veraPDF "device colour without an output intent" failure carries a hint saying what to add.
- Errors fail the file and exit 1. Warnings are advisory. `--quiet` prints nothing, for exit-code-only use.

`verify -j N` checks N files at once (default: up to 4). veraPDF and Ghostscript run as subprocesses and the PDF checks in worker threads,
so nothing blocks the event loop.

As a library the API is async: `await proof_pdf.core.verify_file(path, profile)` returns a `FileReport` of `Finding`s, and
`await proof_pdf.core.verify_files(paths, profile, jobs=4)` returns one report per path in order. See `proof_pdf.models.Profile`.

A job's config file works from Python too. `proof_pdf.load_profile(path=None, *, profile=None)` reads it like the CLI does (the nearest
`proof-pdf.toml`, `PROOF_PDF_CONFIG`, `PROOF_PDF_PROFILE`) and returns a `LoadedConfig`: the `Profile`, the `exclude` globs, the `exempt`
map, and `jobs`, `timeout` and the tool names. A file the CLI would reject raises `proof_pdf.ConfigError`. Pass those to
`core.expand_targets` and `core.verify_files`, so the CLI and your code share one definition. `core.missing_tools(profile)` names the
tools the profile needs (veraPDF for PDF/A, Ghostscript for colour) that are not on `PATH`, so a caller can fail early with one message.

### Proofing

Beyond the standards, `verify` can hold a PDF to the rules a print job usually has. Missing glyphs and fonts are checked by
default (they are exact); every rule that needs a number or a name takes it from you and has **no default**, because a size
floor, a safe margin or a printing condition is a house rule, not something to infer.

| Flag | Fails when |
|---|---|
| *(on by default)* `--no-missing-glyphs` | a character is shown with the `.notdef` glyph (prints as a box or nothing) |
| *(on by default)* `--no-fonts` | text is set in a font that is not embedded (error), or in a Type 3 font (warning); independent of PDF/A |
| `--min-text-pt N` | text is smaller than N pt (effective size, after every scale) |
| `--min-stroke-pt N` | a stroked line is thinner than N pt; a zero-width line is the thinnest the device can draw |
| `--safe-mm N` | text reaches past the trim edge inset by N mm (a box from the font's declared widths; a font without widths gives an estimated warning) |
| `--min-delta-l N` | text and its background differ by less than N in L* |
| `--allow-spot NAME` (repeatable) | a spot ink not on the list is used; names match exactly, and `All` (the registration colour) must be named |
| `--output-condition TEXT` | no output intent names this printing condition (e.g. `FOGRA51`; matched against its identifier and its ICC profile's description) |
| `--require-bleed` | artwork that reaches the trim edge stops short of the bleed edge (see below) |
| `--uniform-pages` | a page's finished size differs from the first page's (a rotated page is a warning) |
| `--pages-multiple-of N` | the page count is not a multiple of N (booklets) |

Contrast is deliberately **not WCAG**, which is a screen model. Colours are converted through an ICC profile (the PDF's own
output intent, or `--proof-profile`) to L*a*b*, the paper is the page's white, and the L* difference is reported with
CIEDE2000 beside it. The background of a text run is the last filled shape painted under it, else the paper; text over an
image or shading is reported as *not measured*, never as a pass. Clipping and transparency are not modelled, so treat it as
the design's intent. CMYK ink needs a CMYK profile to be measured through; without one the check says so and fails.

`--require-bleed` catches the white sliver after cutting: for each side of a page that has a bleed zone (BleedBox, else MediaBox, larger
than the TrimBox), a filled path, image or shading that reaches the trim edge must run to the bleed edge, within half a point. White fills count;
text and strokes do not; objects inset from the trim are ignored. Clips (`W`/`W*`) are followed, so art clipped back to the trim fails and art
clipped to the bleed box passes. It judges geometry, not the printed result, and its limits are: transparency and soft masks are taken as opaque;
a non-rectangular clip counts as its bounding extent; a Form XObject's own `/BBox` is not applied. An object that stops short is excused wherever other artwork covers the bleed strip
beside it, whatever the paint order, so a panel at the trim over a full-bleed background passes. Pages without a TrimBox or bleed zone are skipped (the page-box checks report them) and a
rotated page gets a warning that it was not checked.

### Printer profiles

A job's numbers come from its printer's spec, so keep them in a file committed with the job instead of repeating flags in CI.
`proof-pdf.toml` (or a `[tool.proof-pdf]` table in `pyproject.toml`) is found by walking up from the working directory, or
named with `--config` / `PROOF_PDF_CONFIG`. Each table is a command and each key one of its options, written with dashes or
underscores (or as the flag is spelled):

```toml
[verify]
trim-mm = "105x148"
bleed-mm = 3
min-text-pt = 6
min-stroke-pt = 0.25
safe-mm = 3
output-condition = "FOGRA51"
allow-spot = ["Dieline"]
proof-profile = "profiles/PSOcoated_v3.icc"     # relative to this file

[verify.exempt]
"manual.pdf" = "internal documentation, never printed"

[profiles.deluxe.verify]                         # chosen with --profile deluxe; overrides [verify]
bleed-mm = 5
```

A flag on the command line beats the file, and the file beats the built-in default. Unknown tables and keys are errors, so a
typo cannot silently switch a check off.

```zsh
proof-pdf inspect card.pdf                # fonts (embedded?, .notdef), text colours and sizes, spot inks, images, contrast
proof-pdf inspect card.pdf --json         # the same as data, for other tools
proof-pdf texlog build/card.log           # dropped characters, TeX errors, undefined references, substituted fonts, overfull boxes
proof-pdf verify card.pdf --safe-mm 3 --min-text-pt 6 --min-delta-l 30 --proof-profile FOGRA51.icc
```

`texlog` reads the log because the PDF can't tell the whole story: pdfTeX drops a character the font lacks (LuaTeX typesets
`.notdef`, which `verify` finds), and an undefined reference only prints as `??`. Missing characters, TeX errors and
undefined references are errors; font substitution, package warnings, a pending rerun and overfull boxes are warnings
(`--max-overfull-pt N` promotes the worst overfull boxes).

The library offers the same as data: `proof_pdf.inventory.inspect_pdf(path)` returns plain dataclasses, and the checks are
functions over an opened `pikepdf.Pdf`.

## Docker images

```zsh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/full verify job/ --trim-mm 105x148
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/lualatex lualatex -interaction=nonstopmode book.tex
```

`releaseart/lualatex` is TeX Live 2025 (the `collection-latex`, `luatex`, `latexrecommended` and `fontsrecommended` sets plus a few common
packages) on Debian, with no Java or Python. Add your own packages in a derived image with `tlmgr install`.
`releaseart/lualatex-python` adds Python (the official `python` image), Ghostscript and veraPDF (with a JRE): everything `proof-pdf` needs
except `proof-pdf`, for use as a `FROM` base. `releaseart/full` is that plus the CLI.

All three are published for `linux/amd64` and `linux/arm64`. `:latest` is the most recent build: the weekly one of `main`
(Mondays, which also picks up base-image updates) or a tagged release, whichever is newer. `:1.2.3` comes from a git tag
and `:tl2025` tracks the TeX Live year. Each image reports its commit in `PROOFPRESS_REVISION` and the OCI `revision` label.

## Development

Python 3.15 and [PDM](https://pdm-project.org).

```zsh
git config core.hooksPath .githooks   # ruff + pyright before each commit
pdm install
bin/autoformat.sh                     # ruff format + fix, pyright
bin/test.sh                           # pytest (integration tests skip without veraPDF and gs)
bin/docker/build.sh && bin/docker/smoke.sh
bin/docker/test.sh                    # the whole suite inside the full image, integration tests included
```

See [CONTRIBUTING.md](CONTRIBUTING.md). To release, add the version to [CHANGELOG.md](CHANGELOG.md) and push a tag:
`git tag v1.2.3 && git push origin v1.2.3`. The tag is the version; it publishes to PyPI (TestPyPI first), creates a GitHub
Release and pushes both images.

## Licence

Apache-2.0. Third-party terms, including Ghostscript (AGPL-3.0) and veraPDF, are in [NOTICE](NOTICE).
