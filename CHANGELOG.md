# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `proof-pdf extract` and `proof_pdf.extract.extract_pdf()`: recover assets from any PDF, for rebuilding a template from an
  archived job. Writes the images (JPEG and JPEG 2000 as stored, soft masks as `.mask.png`), the embedded font programs, ICC
  profiles, and vector artwork as one cropped PDF per piece (identical pieces deduplicated), plus `document.json`: page boxes,
  metadata and XMP, output intents, every asset with its placements (page, x, y, width, height in points), every text run
  with its font, size, colour and position, the same runs joined into `lines`, the plain rectangles and rules of the layout
  (touching collinear rules merged, a line's end points kept), and the `inspect` inventory. Vector pieces are named by page and
  order (`vector/p003-v02.pdf`).
- `proof-pdf crop` and `proof_pdf.crop.crop_page()` (public `proof_pdf.crop`): cut a rectangle of a page out as a
  one-page PDF of its own, for an illustration the extraction missed or an image model asks for. It is the page's own
  content seen through a smaller window (text stays text, vectors stay vectors, images keep their pixels), in the same
  point coordinates `document.json` gives every placement, so an asset's place can be cut out as it stands. The review
  page shows the coordinates under the mouse.
- `extract --previews` (`extract_pdf(previews=True)`): `previews/index.html`, a review page with each page rendered by
  Ghostscript and every recovered image, vector piece, text line and rule boxed on it (toggleable layers, links to the
  asset), then thumbnails of the images and vector pieces and the fonts, profiles and notes.
- `document.json` carries `notes` (what could not be done, e.g. text in fonts without `/ToUnicode`) and the font codes of
  such runs (`text[].codes`).
- `proof_pdf.layout` and `document.json["layout"]`: page layout read back from where the text sits, for rebuilding a
  template as a document class. Type styles with roles (body, heading levels, small print) and line pitch; text blocks
  (paragraphs) with alignment, first-line indent, paragraph spacing and column; the text columns, margins and content box
  per page; running headers, footers and folios; and a summary of the numbers a class takes (page size, margins, columns,
  gutter, measure, body font, size and leading, alignment, indent, paragraph skip). Inferred from geometry, so heuristic.
  Text in undecodable fonts takes part by its geometry (`lines[].text` is then `null`).
- `document.json["palette"]`: the colours in use, with the panel area and text characters each colours; boxes carry
  `fill_colour` and `stroke_colour` in the inventory's colour notation.
- XMP metadata is saved as `xmp.xml` and decoded into `metadata.xmp` (prefixed keys, lists, language alternatives and
  structures), which carries InDesign's font, plate and swatch lists and the edit history where the producer wrote them.
- The review page shows the layout summary and the text blocks (a toggleable layer).
- Text over vector artwork is drawn into the piece it overlaps, with its original font, and takes part in the grouping
  (a label touching two parts joins them; two labels never join each other), so a dimensioned drawing or a block diagram
  with its captions is one standalone PDF. The text is still recovered as text and also listed on the asset
  (`details.text`); layout marks such text as a figure label (`Block.figure`) and leaves it out of the body statistics.
  `--no-vector-text` turns it off. Short text within about a line and a half of a drawing is its label even when it does
  not touch it (pin names, package titles, dimensions), a compound path too big to be told from a background (all of a
  drawing's hairlines or arrowheads drawn as one path) is adopted by the drawing it lies in instead of being dropped, a label's reach follows its font size, and small shapes standing
  next to a label (a diameter sign drawn as a path) extend it; text in running paragraphs (a long line, or three or more
  left-aligned lines) never joins. The size cap for backgrounds applies to a shape, not to a whole drawing.
- Vector grouping: short rules and box outlines now bridge the pieces of a diagram when most of the cluster is real
  artwork (`ARTWORK_SHARE`), a cluster of mostly rules (a table) still groups only its non-plain shapes, and a group
  lying mostly inside a bigger one is folded into it. Plain lines drawn with the same pen (stroke width and colour) as
  an artwork shape in their cluster count as artwork, so a schematic that is mostly wires is one piece.
- Text in Adobe GB1, CNS1 and Korea1 CID fonts without `/ToUnicode` is read through the collection's Latin CIDs (1-95 are
  ASCII), which recovered all of one catalogue's text; other characters in such fonts stay `U+FFFD`.
- `document.json["interactive"]`: form fields (type, value, options, font and size, rect; a digital signature is
  summarised, never copied), links with their targets resolved to pages, other annotations, the outline, page labels,
  layers and attachments (saved under `attachments/`).
- Text runs carry rotation, horizontal scale, letter spacing and an invisible flag (an OCR layer); image placements
  carry their matrix.
- `document.json["spot_colours"]`: every Separation and DeviceN ink with its alternate colour at full tint, evaluated from
  the tint transform (sampled, exponential, stitching and PostScript-calculator functions), and the pages that use it.
  Spot fills are named in the palette and in text/box colours (`spot PANTONE 165 C 100%`); the palette now also counts
  the area of vector artwork painted in each colour (`artwork_area_pt2`).
- Tables (`layout.pages[].tables`): ruled grids (rules both ways, with row and column spans where a rule is missing) and
  row-ruled tables (rules between rows, columns from where the text starts), each with its edges and cells and their
  text. A candidate must hold text in enough cells, so gridlines and frames are not tables. Cell text is kept out of
  the column and body statistics (`Block.table`), and the review page has a tables layer. Tables are read from the
  rules the `vector` kind collects.
- Tagged PDFs: `document.json["structure"]` is the structure tree as data, each element with its tag as written and its
  standard role (through the `/RoleMap`), page, alt text, actual text, title, language, the producer's own layout
  attributes (text alignment, indents, spacing, line height, row and column spans), and, from the content it points at,
  its text and box. Runs carry `tag` (the paragraph-like element's role), `struct` and `artifact` (Header, Footer...
  page furniture the PDF marked as such); lines under different structure elements are not merged into one paragraph;
  a style whose lines are mostly tagged H1-H6, Title or Caption takes its role from the tag; artifact-marked headers and
  footers are running text; images and vector pieces take their figure's alt text (`details.alt`), which is the nearest
  thing a PDF has to a name for them.
- Fonts carry their descriptor facts: family, weight, flags, italic angle, bold and italic (from flags, weight, slant or
  the name), and metrics (`details.descriptor`).
- Text recovery in the paint walk (`Paint.text`, read through `/ToUnicode`; word gaps from TJ kerns are a heuristic).

## [0.3.4] - 2026-10-21

### Added

- `latexmk` in the TeX Live set of `releaseart/lualatex`, `releaseart/lualatex-python` and `releaseart/full`.

## [0.3.3] - 2026-10-21

v0.3.1 and v0.3.2 were tagged without changelog sections.

### Added

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

[Unreleased]: https://github.com/release-art/proofpress/compare/v0.3.4...HEAD
[0.3.4]: https://github.com/release-art/proofpress/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/release-art/proofpress/compare/v0.3.2...v0.3.3
[0.3.0]: https://github.com/release-art/proofpress/compare/v0.1.0...v0.3.0
[0.1.0]: https://github.com/release-art/proofpress/releases/tag/v0.1.0
