# proofpress

[![CI](https://github.com/release-art/proofpress/actions/workflows/ci.yml/badge.svg)](https://github.com/release-art/proofpress/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/proof-pdf?logo=python&color=41bb13)](https://pypi.org/project/proof-pdf)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Open-source pieces of a print-on-demand pipeline, extracted from production use.

| Piece | What it is |
|---|---|
| [`proof-pdf`](#proof-pdf) | CLI and library that hold a print PDF to PDF/X, print checks and, when asked, PDF/A. |
| [`releaseart/lualatex`](#docker-images) | TeX Live 2025 with LuaLaTeX. |
| [`releaseart/lualatex-python`](#docker-images) | Python, `lualatex`, Ghostscript and veraPDF: everything `proof-pdf` needs, without it. A base to `pip install` it on. |
| [`releaseart/full`](#docker-images) | `lualatex` plus Ghostscript, veraPDF and `proof-pdf`. |

## proof-pdf

`verify` runs four layers, each of which can be switched off:

- **PDF/A** with [veraPDF](https://verapdf.org), when asked (`--pdfa`; `--flavour 2b` by default, and it needs `--pdfa`).
- **PDF/X**: every requirement of ISO 15930-7 (PDF/X-4, X-4p) and ISO 15930-8 (PDF/X-5g, X-5pg, X-5n) that a file can be
  judged on, from the output intent and metadata to fonts, colour, transparency and referenced files (`--pdfx x4` by
  default, another level with `--pdfx x4p|x5g|x5pg|x5n`, `--no-pdfx` to skip). The counts are under [Checks](#checks).
  PDF/X-1a:2001, X-3:2002, X-1a:2003 and X-3:2003 (ISO 15930-1, -3, -4, -6) are checked too (`--pdfx
  x1a-2001|x3-2002|x1a-2003|x3-2003`); without `--pdfx`, a file whose Info claims one of them is verified at it.
- **Geometry**: trim and bleed against the finished size you expect. Runs only when `--trim-mm` is given (`--bleed-mm`, `--tolerance-mm`).
  For a book interior, `--binding-bleed-mm 0` (with `--binding left|right`, the bound edge of page 1; it alternates page by page) expects no bleed on the binding edge,
  so the sheet is wider by the outer bleed only; the centring check follows the asymmetry.
- **Colour**: total ink coverage, when asked (`--tac`; `--max-tac`, 300% default, and opt-in `--warn-tac N` both need `--tac`, or `tac = true` in a config file; `--warn-tac`, below `--max-tac`, warns when the peak is between N and the limit; the peak is measured per pixel on Ghostscript's separations, one plate per ink, so each spot colour counts once at its tint), stray RGB (`--strict-cmyk`), effective image DPI (`--min-dpi`, 300 default; opt-in `--warn-dpi N` warns below N, `--max-dpi N` warns above N since it only grows the file, `--max-stretch-pct P` warns when an image's two axes differ by more than P percent, larger over smaller less one; none has a default, and `--warn-dpi` must exceed `--min-dpi`; each message gives the worst placement in mm from the page's bottom-left),
  overprint and rich-black small text (`--no-colour`). Overprinting 100% K text is fine; overprint on anything else is flagged,
  and **white that overprints is an error** (it adds no ink, so it vanishes on press). A page that uses transparency must blend
  in the output intent's colour space (a mismatch is an error, an undeclared space a warning).
- **Spot inks and ICC profiles**: one ink defined two ways in the same space is an error (`spot-definition`); an invalid ICC profile, or a
  profile that disagrees with its colour space, is an error, and so is a mix of RGB profiles in an RGB job (`icc-profile`); in a CMYK
  job a CMYK `ICCBased` space using the output intent's own profile is an error, as PDF/X-4 forbids it (ISO 15930-7, 6.4.3.3).
- **RGB proofing** (opt-in, `--colour-mode rgb`): for a screen job, CMYK images, fills, strokes, shadings and patterns and a CMYK output intent are errors, spot colours a warning;
  total ink, overprint and rich-black checks are skipped. Save it as a profile: `[profiles.web.verify]` with `colour-mode = "rgb"` and `pdfx = false`, chosen with `proof-pdf --profile web verify`.
- **Marks and hidden content** (`--no-marks`, opt-in): anything painted beyond the bleed region is an error (`outside-bleed`): crop and registration marks, slug text.
  The region is the BleedBox, else the TrimBox grown by `--bleed-mm` when `--trim-mm` is given, else the TrimBox (a note says which). Judged on the visible extent after
  clipping, so artwork in the bleed zone is correct; pages without a TrimBox are skipped and rotated pages reported as not checked. A text run's extent is its font box.
  Always on, regardless of the flag: an object painted entirely beyond the MediaBox is a warning (`outside-media`, hidden content that ships in the file),
  and a `Do` naming an image or form that is not there is an error (`xobject-missing`): nothing prints where it was placed.
- **Gray proofing** (opt-in, `--colour-mode gray`): for a black-and-white interior, anything painted outside the gray family (DeviceGray, CalGray, ICC gray) is an
  error (`gray-only`): RGB, CMYK and Lab, by fill, stroke, image, inline image, stencil mask (judged by its fill), shading, pattern and form content; spot colours a warning;
  an RGB or CMYK output intent is an error; more than one gray ICC profile is an error (`icc-profile`). The ink checks are skipped, so total ink is not measured (K alone cannot exceed 100%).
  Strict by default. `--allow-k-only` also accepts DeviceCMYK that uses the K plate alone (C, M and Y all zero) and a CMYK output intent: CMYK images are then decoded to
  check their pixels, once per image, which costs time on large images. A DeviceCMYK image that cannot be decoded, inline CMYK images, CMYK shadings and patterns, and
  ICC-based or indexed CMYK are never accepted as K-only. `--allow-k-only` does nothing in the other modes.
- **K-only text** (opt-in, `--k-only-text`): text painted with any ink but K (a four-colour build, RGB, a spot or a pattern) is an error.
- **Proofing** (the text and lines a page paints, measured in page points; see below).

`inspect` reports what is in a PDF without judging it, and `texlog` lints a LaTeX build log. `preview` rasterises each page and draws the trim, bleed and safe guides from the PDF's own boxes, so a geometry finding can be seen.

```zsh
pip install proof-pdf          # Python 3.11+; verapdf (--pdfa) and gs (--tac) on PATH when asked for. Wheels for macOS arm64, Linux
                               # x86-64 and arm64, Windows x64; elsewhere pip builds the Rust core, which needs rustup

proof-pdf verify job/ --trim-mm 105x148 --bleed-mm 3 --exclude '*_preview.pdf'
proof-pdf verify job/ --exempt 'manual.pdf=internal docs, never printed'   # listed with the reason, never fails
proof-pdf verify job/ --k-only-text       # house rule: text is K-only
proof-pdf verify card.pdf --json          # machine-readable findings, for CI
proof-pdf preview card.pdf --dpi 150
proof-pdf --version                       # version, plus the commit when built from an image
```

Without Python, the Rust `proof-pdf` runs `verify` on its own (`inspect`, `preview`, `extract`, `crop`, `layout` and
`texlog` need the Python package):

```zsh
brew install release-art/tap/proof-pdf     # macOS, Linux
```

```powershell
scoop bucket add release-art https://github.com/release-art/scoop-bucket
scoop install release-art/proof-pdf        # Windows
```

or download `proof-pdf-<version>-<target>` from a [GitHub release](https://github.com/release-art/proofpress/releases)
(macOS arm64, Linux x86-64 and arm64, Windows x64), each with its Sigstore bundle.

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
tools the profile needs (veraPDF for PDF/A, Ghostscript for total ink) that are not on `PATH`, so a caller can fail early with one message.

### Checks

<!-- checks -->
Generated from `rust/checks.toml`; every check, with its clauses, is in [rust/CHECKS.md](rust/CHECKS.md).

| Level | Implemented | Not yet | Not checkable (skip) |
|---|---|---|---|
| PDF/X-4 | 146 | 0 | 5 |
| PDF/X-4p | 156 | 0 | 5 |
| PDF/X-5g | 160 | 0 | 25 |
| PDF/X-5pg | 170 | 0 | 25 |
| PDF/X-5n | 160 | 0 | 24 |
| PDF/X-1a:2001 | 82 | 0 | 3 |
| PDF/X-3:2002 | 84 | 0 | 3 |
| PDF/X-1a:2003 | 92 | 0 | 3 |
| PDF/X-3:2003 | 93 | 0 | 3 |
| Print checks | 55 | 0 | 4 |

The print checks:

| Id | What it checks | Severity |
|---|---|---|
| `tac` | Peak total ink stays under the limit (by Ghostscript) | error |
| `overprint` | Overprint is used on purpose | warning |
| `white-overprint` | White is never set to overprint | error |
| `colour-space` | Colour spaces suit the job's colour mode | error |
| `rgb-only` | An RGB job paints only RGB | error |
| `gray-only` | A gray job paints only gray | error |
| `k-only-text` | Text is 100% black only | error |
| `rich-black` | Small text is not in rich black | warning |
| `spot-ink` | Spot inks are on the allowed list | error |
| `spot-definition` | A spot ink has one definition | error |
| `icc-profile` | Embedded ICC profiles are sound and suit the job | error |
| `blend-space` | Transparency declares its blend space | error |
| `image-dpi` | Images meet the resolution floor | error |
| `image-dpi-max` | Images do not exceed the resolution ceiling | warning |
| `image-stretch` | Images are not stretched unevenly | warning |
| `text-size` | Text meets the size floor | error |
| `hairline` | Strokes meet the width floor | error |
| `contrast` | Text contrasts with its background | error |
| `safe-area` | Text stays inside the safe area | error |
| `missing-glyph` | Text shows no missing glyphs | error |
| `font-embedded` | Fonts are embedded | error |
| `font-type3` | Type 3 fonts are reported | warning |
| `geometry` | Pages are the expected trim size | error |
| `bleed` | Pages carry the expected bleed | error |
| `centring` | The trim is centred in the bleed | error |
| `artwork-bleed` | Artwork reaches into the bleed | error |
| `outside-bleed` | Nothing is painted beyond the bleed | error |
| `outside-media` | Nothing is painted wholly off the page | warning |
| `xobject-missing` | Painted XObjects exist | error |
| `bleedbox` | Pages with a trim have a BleedBox (not checked: Replaced by the x4.boxes rows (a BleedBox inside the MediaBox) and the geometry checks' bleed) | error |
| `boxes` | Each page has a MediaBox (not checked: Replaced by page.media-box and the x4.boxes rows) | error |
| `page-size` | Pages share one size | error |
| `page-count` | The page count fits the rule | error |
| `page-rotate` | Pages are not rotated | warning |
| `output-condition` | The output intent names the expected condition | error |
| `pdfa` | veraPDF passes the PDF/A flavour (by veraPDF) | error |
| `structure` | The file can be read (not checked: Not a check: whether the file could be read is the report's own structure and verdict) | error |
| `internal` | The engine itself failed on this file (not checked: Raised when a check panics, which is always a bug in proof-pdf: no file is meant to trigger it, so it has no fixtures; unit tests cover it) | error |
| `file.header` | The file starts with a %PDF- header in its first 1024 bytes | error |
| `file.header-offset` | The header is at the very start of the file | warning |
| `file.startxref` | The file ends with startxref and the offset of its cross-reference | error |
| `file.xref` | Every cross-reference section can be read | error |
| `file.trailer` | The trailer names the document catalog | error |
| `file.encrypted` | The file is not encrypted | error |
| `file.eof` | The file ends with %%EOF and nothing after it | warning |
| `file.limit` | The file stays within proof-pdf's safety limits | error |
| `syntax.hex-digits` | Hex strings have an even number of digits | warning |
| `syntax.octal-escape` | Octal escapes in strings stay within one byte | warning |
| `syntax.name-escape` | Escapes in names are two hex digits, never 00 | error |
| `syntax.duplicate-key` | No dictionary gives a key twice | warning |
| `stream.eol` | Stream data is followed by an end of line before endstream | warning |
| `stream.filter` | Every stream names filters ISO 32000 defines | error |
| `content.damaged` | Every content stream can be read to its end | error |
| `content.limit` | Every content stream stays within proof-pdf's safety limits | error |
| `content.operator` | Undefined operators appear only inside BX and EX | error |
| `content.compatibility` | BX and EX pair up | error |
| `page.media-box` | Every page has a MediaBox, its own or inherited | error |
| `object.damaged` | Every object reachable from the trailer can be read | error |
| `object.dangling-reference` | References point at objects that exist | warning |
<!-- /checks -->

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
proof-pdf extract old/brochure.pdf       # recover images, fonts, ICC profiles, vector artwork and text into old/brochure/
proof-pdf extract old/brochure.pdf --previews   # ...plus old/brochure/previews/index.html to review it
proof-pdf texlog build/card.log           # dropped characters, TeX errors, undefined references, substituted fonts, overfull boxes
proof-pdf verify card.pdf --safe-mm 3 --min-text-pt 6 --min-delta-l 30 --proof-profile FOGRA51.icc
```

`texlog` reads the log because the PDF can't tell the whole story: pdfTeX drops a character the font lacks (LuaTeX typesets
`.notdef`, which `verify` finds), and an undefined reference only prints as `??`. Missing characters, TeX errors and
undefined references are errors; font substitution, package warnings, a pending rerun and overfull boxes are warnings
(`--max-overfull-pt N` promotes the worst overfull boxes).

`extract` is for the archive: a customer's old PDF is all that is left of a job, and you want its logo, pictograms and
photos back. It writes `images/`, `fonts/`, `icc/` and `vector/` (one cropped PDF per piece of artwork, ready for
`\includegraphics`) and a `document.json` with the page boxes, every asset's placements in points from the page's
bottom-left, every text run with font, size, colour and position (and the runs joined into lines), and the plain rectangles and rules of
the layout.
Vector artwork has no names in a PDF, so pieces are found by grouping shapes that touch (`--gap-pt`) and ignoring
anything covering more than `--max-fraction` of the page; clips are not applied and text is not part of a piece.
`document.json` also holds what a template rebuild needs: `metadata.xmp` (the XMP packet decoded, also saved as `xmp.xml`;
InDesign writes its fonts, plates and swatches there), `palette`, and `layout`. The layout reads the page back from where
the text sits: the type styles in use and what each is for (body, heading levels, small print), the paragraphs with their
alignment, indent and spacing, the text columns, margins and content box of every page, the running headers, footers and
folios, and a `summary` of the numbers a document class takes (page size, margins, columns, gutter, measure, body font,
size and leading). It is inferred from geometry, so treat it as evidence to check against the page, not as fact; datasheets
full of tables read poorly, flowing text well.

`interactive` lists the form fields (with type, value, font and size, so a fillable certificate comes back as a form), the
links and where they go, the outline, page labels, layers and attachments. A digital signature is described by who, when
and how it was made, never copied. Text lying on a piece of artwork (dimensions on a drawing, labels on a diagram) is
drawn into that piece's PDF and listed in its `details.text`, as well as recovered as text; `--no-vector-text` leaves the
pieces as the drawn shapes alone.

`spot_colours` names every ink the file paints with and what it stands in for on screen (the tint transform evaluated at
full tint: a PANTONE usually comes back as Lab, a die-line as a CMYK tint); the printer's ink book, not this, says what the
ink really is. `layout.pages[].tables` are tables read from their rules, with cells, spans and text, so a results table or
a product list comes back as rows and columns rather than as scattered text; they need rules (a table made of whitespace
alone is not found).

For a tagged PDF (Word, InDesign and Acrobat export them), `structure` is what the producer said each piece is: which
text is a level-2 heading, where a paragraph or table cell begins and ends, a figure's alt text (the nearest thing a PDF has
to a name for an image or drawing, copied to the asset as `details.alt`), and its own alignment, indent and spacing
numbers. Where the file is tagged it is better evidence than the geometry, and the layout uses it for heading roles and
paragraph boundaries; an untagged file (`structure.tagged` is false) is read from geometry alone. Fonts carry their
descriptor facts (bold, italic, weight, flags, metrics).

`crop` is for what the extraction missed: it cuts a rectangle out of one page as a one-page PDF, exactly as the page
shows it (text stays text, vectors stay vectors, images keep their pixels).

```zsh
proof-pdf crop old/brochure.pdf --page 14 --box 64,300,556,607 --pad-pt 2 --out figure-14.pdf
```

The box is `X0,Y0,X1,Y1` in points from the page's bottom-left, the numbers `document.json` gives every placement (`x`, `y`,
`x + width`, `y + height`) and the review page shows under the mouse; from a preview PNG at a given dpi, a pixel is
`x = media_x0 + px / dpi * 72`, `y = media_y1 - py / dpi * 72`. In code, `proof_pdf.crop.crop_page(path, page, box, out)`.
The piece also holds the rest of the page's content outside the window, which a viewer does not show; rasterise it when
that matters.

`--previews` renders `previews/index.html`: each page with every recovered asset boxed on it and
linked to its thumbnail below, so a whole archive job can be reviewed by eye. Text in fonts without a `/ToUnicode` map
cannot be read from the PDF; `document.json` says so in `notes` and keeps the font codes. Mind who owns the artwork you
recover.

The library offers the same as data: `proof_pdf.inventory.inspect_pdf(path)` returns plain dataclasses, `proof_pdf.extract.extract_pdf(path, out_dir)` writes the assets and returns what it found, and
`await proof_pdf.core.verify_file(path, profile)` runs the checks and returns a report.

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

Python 3.15 (the package supports 3.11 and later) and [PDM](https://pdm-project.org).

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
Release with the standalone downloads, points the Homebrew tap and the Scoop bucket at it (stable releases only) and
pushes the images.

## Licence

Apache-2.0. Third-party terms, including Ghostscript (AGPL-3.0) and veraPDF, are in [NOTICE](NOTICE).
