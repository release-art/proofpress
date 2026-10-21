# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.4] - 2026-10-21

v0.3.1 and v0.3.2 were tagged without changelog sections.

## [0.3.3] - 2026-10-21

v0.3.1 and v0.3.2 were tagged without changelog sections.

### Added

- `latexmk` in the TeX Live set of `releaseart/lualatex`, `releaseart/lualatex-python` and `releaseart/full`.
- `proof_pdf.load_profile()` (public `proof_pdf.config`): the CLI's config-file reading for library callers, returning the `Profile`,
  `exclude` globs, `exempt` map and the tool settings (`LoadedConfig`); errors are `proof_pdf.ConfigError`.
- `core.missing_tools(profile)`: which of veraPDF and Ghostscript the profile needs and cannot find.

### Fixed

- `releaseart/lualatex` and `releaseart/lualatex-python` (and so `full`) now include `curl` and `xz-utils`. Without them
  `tlmgr install` failed every download yet exited 0, so a derived image missed its packages until typeset time.
- The image build retries the TeX Live and veraPDF downloads, which fail now and then from CI runners.

## [0.3.0] - 2026-10-21

Includes everything since 0.1.0; v0.2.0 was tagged without a changelog section.

### Added

- A calibration run over the Altona Test Suite 2.0 (`bin/suites/fetch.sh`, `tests/fixtures/suites/`): the files cannot be
  redistributed, so they are fetched and checked against `expected.toml`, skipping when absent. The Ghent suite is not covered yet.
- `releaseart/lualatex-python`: Python, LuaLaTeX, Ghostscript and veraPDF without `proof-pdf`, a base for images that `pip install`
  it. `releaseart/full` is now built on it. Published with the other images.
- `--require-bleed` (opt-in, `artwork-bleed`): artwork that reaches the trim edge must run on to the bleed edge. The shared
  walk now tracks clipping (`W`/`W*`, scoped by `q`/`Q` and forms) and each `Paint` has a `visible` extent beside `bbox`.
- `--exempt GLOB=reason`: declare files out of scope. They are listed with the reason, counted separately and never fail
  (`FileReport.exempt`, `core.verify_files(..., exemptions=...)`).
- `--k-only-text` (opt-in): text painted with any ink but K is an error.
- A hint on veraPDF's "device colour used without an output intent" failures (clause 6.2.4.3).

- Proofing checks on `verify`: missing glyphs (`.notdef`, on by default), `--min-text-pt`, `--min-stroke-pt` (hairlines),
  `--safe-mm` and `--min-delta-l` (L* contrast through the output intent or `--proof-profile`, with CIEDE2000 reported
  beside it). Every threshold is opt-in with no default.
- `proof-pdf inspect`: what is in a PDF, as data (fonts and whether they are embedded, text colours and sizes, spot inks,
  images with their effective dpi, the thinnest line, contrast pairs), and `proof_pdf.inventory.inspect_pdf`.
- `proof-pdf texlog`: lint a LaTeX log for dropped characters, TeX errors, undefined references, font substitution,
  package warnings and overfull boxes.

- More print-readiness checks: white overprint (error), unembedded and Type 3 fonts (independent of PDF/A), the output
  intent's printing condition (`--output-condition`), a spot-ink allowlist (`--allow-spot`), the transparency blend space
  against the output intent, and page-set rules (`--uniform-pages`, `--pages-multiple-of`).
- A config file (`proof-pdf.toml` or `[tool.proof-pdf]`) with named profiles (`--profile`) for a printer's numbers, found from
  the working directory or named with `--config`. Command-line flags win; unknown keys are errors.

### Fixed

- An image placed at exactly the minimum resolution no longer fails on float rounding ("300 dpi, below the 300 dpi minimum").
- An ICC profile is classified by its own colour space, not just its component count: a Lab profile (three components) is no
  longer read as RGB, which gave `inspect` and the contrast check nonsense colours.
- A veraPDF failure on a file it cannot open now reads as one sentence ("... doesn't appear to be a valid PDF.") instead
  of arriving buried under the JVM's reflection warnings and veraPDF's log header.

### Changed

- The public API is now the top-level modules (`core`, `models`, `inventory`, `preview`, `texlog`, `cli`). The check modules,
  the content and paint walks, the tool wrappers and the config reader moved to `proof_pdf._internal` and are private:
  `proof_pdf.colour`, `.verapdf`, `.structure` and the rest no longer import from their old paths.
- `preview --safe` no longer defaults to 3 mm; a safe-area guide is drawn only when an inset is given.
- Overprint is judged by what is painted under it: overprinting 100% K text passes, and overprint that paints nothing is
  silent. Overprint on any other content is still flagged.
- The overprint, rich-black and K-only checks share one graphics-state walk that honours `q`/`Q` and Form XObject scope, so a
  fill set inside `q ... Q` no longer leaks onto later text.
- The library API is async: `verify_file`, `verify_files`, `verapdf.run`, `colour.check_tac` and `preview.preview_file` are
  coroutines. veraPDF and Ghostscript run as asyncio subprocesses, killed on timeout or cancellation, and the PDF checks run in
  worker threads.
- `proof-pdf verify` checks several files at once; `--jobs/-j` sets how many.

## [0.1.0] - 2026-10-21

First release.

### Added

- `proof-pdf verify` and `proof-pdf preview`: PDF/A (veraPDF), PDF/X-4 structure, page geometry,
  total ink, RGB, image resolution and rich-black checks.
- `releaseart/lualatex` and `releaseart/full` Docker images.
- `--exclude` globs for directory arguments, and `--trim-mm` to switch the geometry check on. There is no
  built-in page size.
- Weekly Docker image builds, with `:latest`, `:tl2025` and a `:<version>` tag per release. Each image records its
  commit, and `proof-pdf --version` shows it.

[Unreleased]: https://github.com/release-art/proofpress/compare/v0.3.3...HEAD
[0.3.3]: https://github.com/release-art/proofpress/compare/v0.3.2...v0.3.3
[0.3.0]: https://github.com/release-art/proofpress/compare/v0.1.0...v0.3.0
[0.1.0]: https://github.com/release-art/proofpress/releases/tag/v0.1.0
