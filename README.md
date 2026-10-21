# proofpress

[![CI](https://github.com/release-art/proofpress/actions/workflows/ci.yml/badge.svg)](https://github.com/release-art/proofpress/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/proof-pdf?logo=python&color=41bb13)](https://pypi.org/project/proof-pdf)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Open-source pieces of a print-on-demand pipeline, extracted from production use.

| Piece | What it is |
|---|---|
| [`proof-pdf`](#proof-pdf) | CLI and library that hold a print PDF to PDF/X-4 and PDF/A. |
| [`releaseart/lualatex`](#docker-images) | TeX Live 2025 with LuaLaTeX. |
| [`releaseart/full`](#docker-images) | `lualatex` plus Ghostscript, veraPDF and `proof-pdf`. |

## proof-pdf

`verify` runs four layers, each of which can be switched off:

- **PDF/A** with [veraPDF](https://verapdf.org) (`--flavour 2b` by default; `--no-pdfa` to skip).
- **PDF/X-4 structure**: page boxes, `/Trapped`, the `GTS_PDFX` output intent and the XMP version key (`--no-pdfx`).
- **Geometry**: trim and bleed against the finished size you expect. Runs only when `--trim-mm` is given (`--bleed-mm`, `--tolerance-mm`).
- **Colour**: total ink coverage (`--max-tac`, 300% default), stray RGB (`--strict-cmyk`), effective image DPI (`--min-dpi`, 300 default),
  overprint and rich-black small text (`--no-colour`). Overprinting 100% K text is fine; overprint on anything else is flagged.
- **K-only text** (opt-in, `--k-only-text`): text painted with any ink but K (a four-colour build, RGB, a spot or a pattern) is an error.

`preview` rasterises each page and draws the trim, bleed and safe guides from the PDF's own boxes, so a geometry finding can be seen.

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

## Docker images

```zsh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/full verify job/ --trim-mm 105x148
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/lualatex lualatex -interaction=nonstopmode book.tex
```

`releaseart/lualatex` is TeX Live 2025 (the `collection-latex`, `luatex`, `latexrecommended` and `fontsrecommended` sets plus a few common
packages) on Debian, with no Java or Python. Add your own packages in a derived image with `tlmgr install`.
`releaseart/full` adds Ghostscript, veraPDF (with a JRE) and `proof-pdf`.

Both are published for `linux/amd64` and `linux/arm64`. `:latest` is the most recent build: the weekly one of `main`
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
