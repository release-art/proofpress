# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `--exempt GLOB=reason`: declare files out of scope. They are listed with the reason, counted separately and never fail
  (`FileReport.exempt`, `core.verify_files(..., exemptions=...)`).
- `--k-only-text` (opt-in): text painted with any ink but K is an error.
- A hint on veraPDF's "device colour used without an output intent" failures (clause 6.2.4.3).

### Fixed

- A veraPDF failure on a file it cannot open now reads as one sentence ("... doesn't appear to be a valid PDF.") instead
  of arriving buried under the JVM's reflection warnings and veraPDF's log header.

### Changed

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
