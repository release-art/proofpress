# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

[Unreleased]: https://github.com/release-art/proofpress/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/release-art/proofpress/releases/tag/v0.1.0
