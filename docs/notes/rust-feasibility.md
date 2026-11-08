# A Rust engine for proof-pdf: feasibility, findings and gotchas

Written 2026-11-08 to plan the work from. It collects what the spikes of 2026-11-06 and 2026-11-07 established. The measurements
themselves are in [`rust-port-spikes.md`](rust-port-spikes.md); the step-by-step plan, if the work goes ahead, is
[`../briefs/rust-port.md`](../briefs/rust-port.md). Each claim below is marked by how well it is supported:

- **Measured:** run against real files and compared.
- **Partial:** measured on a small or one-sided sample.
- **Judgement:** reasoned, not run.
- **Untested.**

## Bottom line

- **Feasible.** Every hard capability was tried: reading PDFs, the content walk, ICC colour, image decoding, writing, a Python
  wheel, a native binary. Each worked, or has a working stand-in.
- **The riskiest part was the parser, and it came out well.** A strict parser of about 800 lines matched pikepdf on 3.09M
  operators and refused only files qpdf also called damaged, or encrypted ones.
- **The largest part is the least proven.** `paint`, `colour` and `functions`, about 1,500 lines, were not ported in the spikes.
  The estimate rests on one port of 130 lines.
- **It will not make `verify` faster** in normal use: veraPDF's JVM start dominates. The gains are a single binary, an 8 ms start
  when veraPDF is off, no pikepdf or Pillow at runtime, and a parser that reports damage instead of repairing it.
- **About 10-14 weeks for everything, or 6-8 for `verify` and its CLI**, for one experienced engineer. Judgement.
- **Stage 4 is a viable place to stop.** With `verify` in Rust, the other commands could stay in Python on pikepdf.

## What the evidence supports

| Claim | Evidence | Strength |
|---|---|---|
| A strict parser of our own reads valid PDFs as pikepdf does | 114 clean fixtures, 992 of 1,044 pdf.js and Altona files, 544 of 644 `corpus/` files, all 84 real card decks (10 GB, 4,037 pages). Every refusal checked against qpdf's own damage warnings: none where qpdf saw nothing wrong, except our deliberate "`%PDF-` at byte 0" rule | Measured |
| It survives hostile input | No crash or hang on 1,074 pdf.js and Altona files or on 4,000 mutated fixtures, after one fix for self-referencing objects | Measured, but no long fuzzing |
| It reads InDesign and Illustrator output | 2 InDesign, 4 Illustrator and 27 QuarkXPress files, all accepted. Few, and the stillhq set they mostly come from is old | Partial: the main gap |
| The content walk matches Python exactly, CTM included | 3.09M operators; 5 page walks differ, on damaged files or the inline-image case | Measured |
| A ported check gives identical findings | `x4.py`: 1,194 of 1,196 files identical, 5,299 findings | Measured, one check |
| ICC colour matches Pillow | `lcms2`: 1296 of 1296 grid colours exact, on two real CMYK profiles | Measured |
| Image decoding matches Pillow | 217 identical, 137 within 3 codes, 11 CMYK JPEGs resolved by `/Decode`, 25 that Pillow could not decode. JBIG2 and CCITT checked for size only | Partial |
| Peak ink from a Ghostscript raster ports | The `tiff` crate matched on 6 of 6 rasters | Partial |
| Writing PDFs works | Crop through `lopdf` pixel-identical on 5 of 5. Our own writer not tried | Partial |
| Previews without Ghostscript | `hayro` rendered 116 of 116, median 0.13% of pixels noticeably off | Measured |
| A Python wheel on 3.15 | PyO3 0.30 built a `cp315` wheel on macOS arm64; findings matched | Partial: one platform |
| A single binary | Links only macOS system libraries, including static `lcms2` | Partial: macOS only |
| It is faster | Walk about 7x, X-4 about 3x, start 8 ms against 200 ms | Measured, but invisible behind veraPDF |
| Porting `paint`, `colour`, `functions`, `layout`, `extract` | — | Untested |

## Component map

Lines are today's Python. "Read" means the module only reads PDF objects, which the parser covers.

| Python | Lines | In Rust | Evidence | Risk |
|---|---|---|---|---|
| pikepdf itself (opening, objects, streams, filters) | — | Our strict parser | See above | Medium: real InDesign files |
| `content.py` | 178 | Walk with CTM, form recursion, inherited resources | Exact on 3.09M operators | Low |
| `paint.py` | 649 | Graphics state, clipping, text state, colour per painted object | Not spiked | **High**: largest module, everything depends on it |
| `colour.py`, `spaces.py`, `spots.py` | 903 | Checks over paint events. Total ink stays on Ghostscript, moving to `tiffsep` | Ink: 6 rasters | Medium |
| `functions.py` | 233 | PDF functions, including a Type 4 PostScript calculator | Not spiked; already our own code | Low-medium |
| `contrast.py`, `deltae.py` | 272 | `lcms2` to Lab, CIEDE2000 | `lcms2` exact | Low, but see the 8-bit gotcha |
| `fonts.py`, `text.py` | 377 | Font dictionaries and `ToUnicode` maps only; no font program is parsed | Not spiked | Low |
| `structure.py`, `pdfx/` | 599 | Page boxes and geometry | X-4 ported exactly | Low |
| `resolution.py` | 179 | Image placement maths | Not spiked | Low |
| `xmp.py` | 76 | An XML crate | Not spiked | Low |
| `interactive.py`, `tagged.py`, `inventory.py` | 694 | Read: name trees, number trees, structure tree | Not spiked | Low-medium |
| `layout.py`, `tables.py` | 855 | Pure geometry over text positions from `paint` | Not spiked | Low, once `paint` exists |
| `extract.py`, `vector.py` | 1,536 | Image and font extraction, vector art rewritten as one-page PDFs, via the writer | Not spiked | **Medium-high**: the writer and object copying are untested |
| `crop.py` | 56 | The writer | `lopdf` only | Medium |
| `preview.py`, `previews.py` | 442 | `hayro` render, guide labels via a font rasteriser, HTML | Render measured; labels not | Low-medium |
| `texlog.py` | 141 | Text only | Not spiked | Low |
| `verapdf.py`, `proc.py` | 159 | Subprocess and JSON | Not spiked | Low |
| `core.py`, `models.py`, `config.py`, `_internal/config.py`, `cli.py` | 1,368 | clap, TOML, threads instead of asyncio, the same flags and output | Not spiked | Medium: exact flag and output parity is long and fiddly |

## Gotchas

### pikepdf and qpdf behaviours that tests silently rely on

- **pikepdf rewrites the document when it opens it.** qpdf pushes inherited `/MediaBox`, `/CropBox`, `/Rotate` and `/Resources` down
  onto pages and creates objects the file does not have. `content.iter_operations` reads `/Resources` off the page and sees
  inherited resources only because of this. Our walk must search the parent nodes itself. `/TrimBox` and `/BleedBox` never inherit.
- **qpdf inlines indirect `/Length` values and drops dictionary keys whose value is null.** A key with a null value means the same
  as an absent key, and our parser does the same.
- **pikepdf turns indirect integers into plain Python ints,** so the parity baseline cannot tell them apart. That is a limit of
  `baseline.py`, not a difference.
- **qpdf repairs silently.** Truncated files, wrong offsets and wrong `/Length` values all open today and get verified as if
  nothing happened. Under the strict parser they fail with a reason. That is a deliberate behaviour change.
- **`PasswordError` is not a `PdfError`,** so a password-protected PDF crashes `verify` today (bug 1 in the brief).

### PDF syntax the parser must handle on purpose

- Multiple content streams are joined with whitespace between them.
- Garbage in content streams: qpdf reports `.020.594` or `1e2` as operators, and hayro skips them. Ours follows qpdf's tokens for
  parity, then refuses or reports, as decided per reason.
- Inline images end at an `EI` found by heuristic. Use `/L` where present.
- `BX`/`EX` allow unknown operators.
- Hybrid files can share a `/Prev` between the table and the stream. A repeated offset is not a loop.
- An object can depend on itself, for example a stream whose `/Length` is a reference to itself. Detect it; it overflowed the
  stack in the spike.
- Read only what the trailer reaches. Unreferenced junk objects are common, and they cannot print.
- An unbalanced `Q` restores the state the stream started with. Guard form recursion by object number, with a depth cap.
- A missing reference is legal and reads as null. Painting a missing XObject is still a hole in the page, which needs a finding
  (bug 3 in the brief).
- Decompression bombs exist in the corpus (`bomb_giant.pdf`). No limits are set yet.

### Numbers and colour

- **Keep f64.** `lopdf` stores reals as f32, and `2.2147` came back as `2.2148`. Box checks compare against a 0.5 pt tolerance, so
  rounding can flip a finding.
- **CMYK JPEG polarity follows `/Decode`,** not Pillow's habit of inverting Adobe JPEGs. Ghostscript settled this. Today's Python
  gets it wrong when `/Decode` is absent (bug 2 in the brief).
- **JPEG decoders are not bit-identical:** one CMYK image differed by 1 code. `--allow-k-only` tests for exactly zero ink, so decide
  how a near-zero value is treated. Do not leave it to the decoder.
- **Contrast rounds Lab to 8 bits.** `contrast.py` converts through Pillow's 8-bit LAB mode, so L* steps by about 0.39. `lcms2` can
  give full precision. Exact parity needs the same rounding; dropping it changes contrast results slightly, so decide which.
- **Total ink and spot colours:** Ghostscript's `tiff32nc` counts a spot as its CMYK alternate, and loses a spot that overprints
  process ink. Decided 2026-11-07: one ink per spot, measured with `tiffsep`. Ghostscript 10.08 fails on Altona with
  `-dOverprint=/simulate`, so do not use that flag.

### Python, packaging and tests

- **`core.verify_file` and `verify_files` are async in the public API.** Keep them async in Python and run the Rust work in a
  thread with the GIL released. The spike's `py.detach` does this.
- **A Python object cannot borrow from the PDF bytes,** so the `Document` pyclass must own its data and store offsets. The strict
  parser already does.
- **Python 3.15 only.** PyO3 0.30 supports it. abi3 and free-threaded builds are untested. `release.yml` already signs under Python
  3.13 because the Sigstore action's dependencies lack 3.15 wheels; expect more of that.
- **Versions come from the git tag** through pdm. maturin reads `Cargo.toml`, so a tag must reach both.
- **Wheels go from one pure wheel to a matrix:** macOS arm64, Linux x86-64 and arm64 (manylinux). None tried yet.
- **The tests build their PDFs with pikepdf** (542 uses in 15 test files). pikepdf stays a development dependency. The 18 test
  files that import `_internal` need rewriting against the new API, or replacing by the parity gate.
- **One public exception changes:** `crop_page` documents `pikepdf.PdfError`, and `cli.py` catches it at three sites.
- **Licences:** `mupdf`, Ghostscript and Kura are AGPL, so they can be called as external tools but never linked or ported.
  Add `cargo deny` so no AGPL crate slips in.
- **Previews bring a second parser:** `hayro` carries `hayro-syntax`. That is harmless, since it never reaches a check.

### Running the external tools

- veraPDF and Ghostscript stay external. A requested check whose tool is missing must fail, never pass. If PDF/A and total ink
  become opt-in, a bare `verify` works without them; that default is still to be confirmed.
- Ghostscript can exit non-zero yet leave a partial raster. Treat any non-zero exit as a failed check, never read the output.
- Kill the whole process group on timeout, and cap what is read from each tool's output.

## Risks

| Risk | Likelihood | Impact | Mitigation | Early signal |
|---|---|---|---|---|
| The strict parser refuses real InDesign or Illustrator output that prints correctly | Medium | High: it changes the parser decision | Stage 0: run recent jobs from `proof-pdf-test-db` through the oracle | Any refusal that qpdf does not also call damaged |
| `paint` and `colour` take far longer than estimated | Medium | High | Port `paint.py` first and measure lines per day after a week | At the end of week one, `paint.py` is too far from done to fit the 3-4 weeks for stage 3 |
| Parity noise: pikepdf quirks look like port bugs | Medium | Medium | The list of expected differences in the brief; classify before fixing | Many small differences on clean files |
| Packaging: manylinux, static Linux, `cp315`-only wheels | Medium | Medium | Do it in stage 1, before any port | A wheel that will not install on a clean machine |
| Hostile input exhausts memory or time | Medium | High for a server | Limits per kind of structure, long fuzzing, tool timeouts | Fuzz findings; `bomb_giant.pdf` |
| Behaviour changes upset users (strict refusals, opt-in PDF/A, spot ink) | Medium | Medium | Changelog first; warnings where real files trip a rule | Issues after release |
| `lcms2` (C) complicates static and other targets | Low | Medium | Static build in stage 1; `moxcms` as fallback research | Link failures in CI |
| `hayro` API churn or abandonment | Low | Low: previews only | Pin it; Ghostscript previews remain possible | Upstream inactivity |
| The estimate is wrong as a whole | High | Medium | Decision points below | — |

## Effort and decision points

The stages and what "done" means for each are in the brief. In short: stage 0 under a week, then 1, 1-2, 3-4, 1, 3-4 and 1-2 weeks.

Decide at these points, each with what would change the plan:

1. **After stage 0, under a week:** does the parser accept real InDesign and Illustrator jobs? If it refuses files that print
   correctly, and those refusals cannot sensibly become warnings, reconsider using a lenient parser.
2. **One week into stage 3:** measure the actual pace of porting `paint.py` against the estimate. Re-plan the rest from that.
3. **After stage 4, about 6-8 weeks:** `verify` is in Rust with its CLI. Either stop and keep `inspect`, `extract`, `crop`,
   `preview`, `layout` and `texlog` in Python on pikepdf, or carry on to drop pikepdf and Pillow altogether.

## Not tested

- Porting `paint`, `colour`, `functions`, `layout`, `tables`, `extract`, `vector`, `inspect`.
- A writer of our own, and copying objects between files.
- Linux and Windows builds, manylinux wheels, abi3, a static Linux binary.
- Long fuzzing and decompression limits.
- Recent InDesign and Illustrator output in quantity.
- Pixel equality for JBIG2 and CCITT images, and the one image size difference on `issue6413.pdf`.
- `moxcms` as a pure-Rust replacement for `lcms2`.

## What exists to start from

- `spikes/crabify/parity/src/strict.rs`: the strict parser, 808 lines, with the walk in `strict_b.rs`.
- `spikes/crabify/proofcore/`: the X-4 port as a PyO3 module and a CLI.
- `spikes/crabify/codecs/`: ICC, TIFF ink, image decoding and crop experiments.
- The parity gate: `baseline.py`, `compare.py`, `check_parity.py`, `oracle.py`, `fuzz.py`.
- Sample files: `tests/fixtures/pdfs/`, `corpus/` via `bin/suites/corpus.sh`, Altona via `bin/suites/fetch.sh`, and private jobs in
  `proof-pdf-test-db`.
- Rust is not installed on this machine; the spikes used a temporary toolchain. Install `rustup` first.
