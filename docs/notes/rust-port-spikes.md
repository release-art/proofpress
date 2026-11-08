# Porting proof-pdf to Rust: spike results

Read 2026-11-06, on macOS arm64 with rustc 1.99.1, Python 3.15.0, pikepdf 10.17.0. The request said `pdf-verify`; nothing has
that name, so this is about `proof-pdf`. Spike code is in `spikes/crabify/`.

**Decided 2026-11-06: a strict parser of our own.** The plan is `docs/briefs/rust-port.md`, which supersedes the estimates and the
stage table here. Sections that compare or recommend `hayro-syntax` and `lopdf` record how that decision was reached.

## Verdict

Feasible, and the risky part is smaller than expected. `hayro-syntax` reads PDFs well enough to replace pikepdf for every
read path, and a strict parser of our own did as well (see below); `lopdf` covers the write paths; `lcms2` reproduces the ICC results exactly; PyO3 builds on Python 3.15.

Estimate: **about 5-6 weeks for `verify` plus its CLI, about 9-12 weeks for everything**, one experienced engineer. That is a
judgement from one measured port (`x4.py`, 130 lines of Python to 145 of Rust), not a measurement of the whole job.

It will not make a normal `verify` noticeably faster, because veraPDF's JVM start is most of the time (see Speed).

## What was tested

Ground truth is `baseline.py`: every object, stream hash, page box and the full `iter_operations` walk with its CTM, as pikepdf
sees them. Each Rust backend ran one file per process, so a panic or hang counts as a result.

Corpus: 115 fixtures, 45 damaged or re-encoded mutants of 5 fixtures, 1,072 pdf.js test files (many malformed on purpose), and
both Altona files.

### Reading: hayro-syntax wins

1,074 files (pdf.js and Altona), 1,852 pages, 3.16M operators:

| | hayro-syntax 0.9 | lopdf 0.47 |
|---|---|---|
| Crashes or hangs | 0 | 3 panics |
| Page walks that differ | 6 | 221 |
| Number precision | f64 | f32, so `2.2147` reads as `2.2148` |
| Password-protected file | refuses (2 exceptions, below) | opens it without the password |
| Image filters | DCT, JPX, JBIG2, CCITT | none |
| Writes PDFs | no | yes |

All 7,144 operators in the fixture set and mutants matched under hayro. The 6 differing walks are corrupt streams, where qpdf
reports garbage such as `.020.594` as operators and hayro skips it. Sampled stream differences were 1-2 trailing bytes under a
wrong `/Length`. Object differences come from pikepdf rewriting the document (below) and from a limit of the baseline, which
loses indirect integers. The classification covers object differences in full, but only the first few stream differences.

Ruled out on paper: `qpdf` (C API has no content parser), `mupdf` (AGPL against this project's Apache-2.0), `pdfium-render`
(no operator-level access).

### A real check, end to end

`proofcore` ports `pdfx.check` and is built as both a `cp315` wheel (756 KB) and a native binary.

Over 1,239 files, findings were **identical on 1,194 of the 1,196 files both sides could check** (5,299 findings). The other 2
are malformed page trees (`issue22011`, `operator_list_cycle`).

### The rest

| Area | Result |
|---|---|
| ICC, CMYK to Lab | `lcms2` exact against Pillow: 1296/1296 on ISO Coated v2 300% and on FOGRA39L |
| ICC in pure Rust | `moxcms` failed with `UnsupportedProfileConnection` in 8-bit and f32. Two attempts only, and its README claims CMYK to Lab, so this is unresolved rather than impossible |
| Peak ink from TIFF | `tiff` crate identical on 6/6 rasters, about 10x faster. Small sample |
| JPEG, JPX | 217 identical extrema, 137 within 3 codes, 11 CMYK (below). Rust also decoded 25 images Pillow could not |
| JBIG2, CCITT | 282 decoded at the right size. Pixels not compared |
| Crop through lopdf | pixel-identical on 5/5 at 100 dpi. Output can be larger, with no `remove_unreferenced_resources` equivalent |

## Speed

`proof-pdf verify`, seconds:

| File | Python checks | Ghostscript | veraPDF | Total (parallel) |
|---|---|---|---|---|
| `good.pdf` | 0.00 | 0.11 | 0.73 | 0.58 |
| Altona, 126 MB | 2.62 | 2.63 | 1.19 | 2.83 |

The content walk is about 7x faster in Rust, and the X-4 check about 3x. Cold start is the visible gain:

| Command on `good.pdf` | Time |
|---|---|
| Rust binary (X-4 check only) | 8 ms |
| `verify --no-pdfa --no-colour` | 200 ms |
| `verify --no-pdfa` | 275 ms |
| `verify` | 800 ms |

The Rust row does less work than the others, so read it as the floor. The binary would still need Ghostscript and veraPDF with
its JVM, so "one file to install" holds only if those layers are off.

## A single binary, without veraPDF and Ghostscript

Linking is already solved: `proofrs` depends only on `libSystem`, and the build with static `lcms2` only on macOS system
libraries. macOS arm64 only so far; a static Linux build is untested. What matters is what each tool does today.

| Job | Today | In-process option | Result |
|---|---|---|---|
| Previews | Ghostscript | `hayro` | Works. 116/116 first pages rendered; median 0.13% of pixels off by more than 32/255, worst 10.6% (colour-managed files); about 3.5x faster |
| Peak ink (`tac`) | Ghostscript CMYK raster | none | Needs a new CMYK compositor |
| PDF/A | veraPDF | none good enough | Needs a new validator |

**Peak ink.** hayro cannot supply it: `Paint::Color` is RGBA, images leave through `with_rgba`, and it has no overprint, so CMYK
is gone before a `Device` sees it. A rough estimate from painted colours alone agreed with Ghostscript within 2 points on 75 of
139 pages and missed 7 over-limit pages, though it cannot price images, shadings or RGB at all. A real replacement rasterises
paths, glyphs, images, shadings and patterns, and blends transparency and overprint in CMYK. Judgement: 4-8 weeks, and matching
Ghostscript on RGB-to-CMYK conversion is a further unknown.

**PDF/A.** veraPDF's PDF/A-2b profile has 144 rules. `pdfpurr` (MIT) disagreed with veraPDF on 8 of 116 files: 6 false passes,
and it failed `pdfa-2b-rgb.pdf`, which veraPDF passes. `pdf-compliance` is AGPL or commercial, and its README disclaims parity
with an independent validator. It was not run. A validator of our own is roughly 8-12 weeks for one flavour by judgement. Even
then "passes our validator" is a weaker claim than "passes veraPDF", which is what a printer or archive will recognise.

Embedding Ghostscript is not a shortcut: it is AGPL or commercial, against this project's Apache-2.0.

**Recommendation:** ship the single binary, with veraPDF and Ghostscript as optional tools it calls when present. Every other
check runs in-process. A check that was asked for but cannot run stays a failure, as `core.py` already promises, so the defaults
must change: PDF/A and peak ink become opt-in, or a bare `verify` fails on a machine without the tools.

## Writing our own parser

**Recommended, in strict form.** An earlier draft of this note argued against it, because repairing malformed files is the hard
part. A verifier does not need repair: a file that needs it can simply fail. PDF/X and PDF/A forbid encryption too, so that code
goes as well. With both gone, a parser took about 800 lines (`parity/src/strict.rs`, plus 164 for the walk and harness).

It reads xref tables and streams, hybrid files, object streams, Flate, LZW, ASCII85, ASCIIHex, RunLength, predictors, the page
tree and content streams. It refuses anything else, with a reason, and reads every object reachable from the trailer at open.

Same harness as above:

| Measure | Result |
|---|---|
| Clean fixtures (114) | All accepted, no false rejects. The object differences are pikepdf's synthesised objects |
| Fixture mutants | Damaged and encrypted ones refused. Object-stream and QDF rewrites accepted |
| pdf.js + Altona, files pikepdf opens | 992 of 1,044 accepted, both Altona files among them |
| The 52 refused | qpdf reports damage on 41; the other 11 are encrypted by policy. None where qpdf saw nothing wrong |
| Operators on accepted files | 3.09M, 5 page walks differ, all on damaged files or the inline-image case below |
| Speed | Content walk about 7x Python, the same as hayro |
| Mutation fuzz, 4,000 files | Every one answered, no panic or hang |

The check on false rejects is independent: pikepdf records a warning whenever qpdf repairs something. It is only as good as qpdf's
idea of damage.

The refusal reasons are themselves useful output: wrong `/Length`, `startxref` pointing at the wrong place, damaged Flate data.
Today those files are repaired silently and verified as if nothing happened.

**Why own it:**
- No patch to carry, and no dependence on one maintainer for the read path.
- `/Info`, repair status and spec violations are all ours to report.
- One object model could also write, dropping `lopdf`. Not tested: a writer is perhaps a few hundred lines by judgement.

**Costs and open items:**
- **Behaviour change.** A file with a damaged xref passes today and would fail. That is arguably right, but it needs a line in the
  changelog and a decision per reason, some of which may be better as warnings.
- **The corpus is thin on non-LaTeX print jobs.** Run it over real InDesign, Illustrator and Acrobat output before committing.
  This is the main remaining risk.
- **Not strict enough yet.** 86 accepted files drew a qpdf warning, mostly a wrong trailer `/Size` (59), which could become a
  warning here. On 5 of them qpdf reconstructed the xref and this parser did not notice. That is unexplained.
- **Unfinished:** the TIFF predictor below 8 bits; inline image ends, which are a heuristic (one file refused for it);
  duplicate dictionary keys, which are silently last-wins although some fixtures have them; no long-running fuzz.
- **Image decoders stay external.** JPEG 2000, JBIG2 and CCITT are about 20,000 lines. Those crates do not depend on `hayro-syntax`,
  though using them alone was not tested.
- **Previews still pull in `hayro`,** and with it `hayro-syntax`, so the binary would hold two parsers. That is harmless: rendering
  is where leniency belongs, and no patch is needed there.

Estimate: 1-2 weeks to production quality, by judgement. That is on top of the stages below: the total is about 10-14 weeks, or
about 6-8 for `verify` and its CLI, as set out in the brief.

**Real output, since this was written:** all 84 card decks in `educational-cards` were accepted (10 GB, 4,037 pages, 1.9M operators,
page counts matching pikepdf). Nearly all are LuaTeX. `corpus/` added 644 more files from other producers: 544 accepted, no crash, and
the producer table is in the brief. Recent InDesign and Illustrator output is still the gap.

## Kura, tested as an external tool

[Kura](https://github.com/alam00000/bentopdf-kura) (v1.3.0, read 2026-11-07) is a C++ archival and preflight engine on PDFium.
It is AGPL-3.0 or commercial, so like Ghostscript it can only ever be an optional external tool here, never linked or ported.
It is about 2.5 months old with one contributor. The release binary matched its published SHA-256 and ran with networking denied
by `sandbox-exec`. Scripts: `spikes/crabify/kura.py`, `spikes/crabify/kura_preflight.py`.

| Test | Result |
|---|---|
| PDF/A-2b verdict against veraPDF, 216 files | Agreed on all 206 where both answered (191 fail, 15 pass). Only 15 passes, so the pass side is thin |
| 1,838 hostile and sample files | No crash, no timeout |
| Refusals against the strict parser | 85 we refuse it opens, reporting the damage as repairable findings. 3 we open it refuses, as pikepdf does; see the brief |
| Preflight, fixtures and Altona, matched thresholds | Hairlines, text under 5 pt and white overprint agree on every file. Total ink misses both Altona files (330%, 363%): it reads object colours and does not render. Resolution misses a 1 dpi inline image mask |
| A 9-page RGB office report | It agrees on the preflight concerns, misses the `endstream` end-of-line defect veraPDF reports, and lists PDF/X-4 requirements `x4.py` does not check: title, creation and modification dates, forbidden link annotations and embedded files |

Kura's README says check mode sees only what its converter can repair, and the report bears that out: a syntax defect fixed
silently on rewrite never becomes a finding. It does not replace veraPDF, and its ink check does not replace the Ghostscript
raster. Its check catalogue is a useful list of what shops ask for, read for ideas only because of the licence.

### From the source

Read 2026-11-07, with the main claims checked against the code and the binary:

- It reads PDFs with qpdf, recovery on, and renders with PDFium.
- It judges a file damaged when a qpdf warning contains "reconstruct". It counts on qpdf's wording.
- Its raster is RGB from PDFium, converted to CMYK through a built-in profile, so even its rendered ink figures are estimates.
- Hits from a check profile do not affect the verdict. `hairline.pdf` with the hairlines profile reports an error-level hit,
  `compliant: true` and exit 0, so a CI job would pass it.
- `docs/check-mode.md` says check mode agrees with veraPDF on 568 of 568 and 954 of 970 files. The only veraPDF code in the
  repository is in `bench/`, which validates converted output, not check-mode verdicts. Our own run did find agreement on 206 files.
- It does detect a missing end of line before `endstream`, as `keyword_line_ends` (`profile_eval_obj.cpp:1506`), contrary to an
  earlier reading. Its PDF/A-2b check simply does not use that evaluator.

The same reading turned up a gap of our own. Ghostscript's `tiff32nc` does not count a spot ink that overprints process ink. The
brief has the measurements. Decided 2026-11-07: one ink per spot, measured with `tiffsep`; it is bug 4 in the brief.

## What a port must get right

- **pikepdf rewrites the document.** By the time `pikepdf.open` returns, inherited attributes can already be pushed down onto
  pages, adding objects the file does not contain. `content.iter_operations` reads `/Resources` off the page and appears to see
  inherited ones only because of this. The Rust walker has to walk the parents itself; the spike does.
- **hayro is more forgiving.** It opened 16 files pikepdf refuses (13 `PdfError`, 1 decode error, 2 `PasswordError`), so some
  files that fail today as "cannot open" would be checked instead. Choose that on purpose. It also hides whether it repaired the
  xref, which a preflight tool may want to report.
- **Why hayro opens `issue19484_1` and `_2` without a password is not explained yet.** Look before relying on it.
- **JPEG decoders differ.** `cmykjpeg.pdf` differs by 1 code on one channel. `--allow-k-only` tests for exactly zero ink, so a
  different decoder can flip the verdict near zero.
- **CMYK polarity follows `/Decode`,** not Pillow's habit of inverting Adobe JPEGs (bug 2 below).

## Gaps and dependencies

- **hayro has no public trailer access**, so `/Info` and `/Trapped` are out of reach. `hayro-info.patch` adds it, a few lines.
  It needs upstreaming or a maintained fork.
- **hayro-syntax has one crates.io owner** and is pre-1.0. It was released this month and is active, but budget for API churn.
- **"Fully in Rust" has a C exception:** `lcms2` is C, statically linked, unless `moxcms` works out. `--min-delta-l` is the main
  consumer. Spot and profile checks need less from it.
- **Two PDF libraries** in one product: hayro to read, lopdf to write. `vector.py`'s `copy_foreign` has no direct lopdf
  equivalent and was not spiked.
- `preview` draws labels with Pillow's bundled font, so it needs a font and a rasteriser in Rust.

## Not tested

Linux or manylinux wheels and abi3; porting `paint`, `colour` and `functions`, which is the bulk of the work; decompression-bomb
limits; pixel equality for JBIG2 and CCITT; the one size difference on `issue6413`; `extract`, `inspect` and `preview`.

## Estimate

Lines of Python: about 4,200 on the `verify` path, 750 CLI, 3,800 for `inspect`, `extract`, `crop`, `preview`, `layout` and `texlog`.

| Stage | Work | Weeks |
|---|---|---|
| 0 | Cargo workspace, maturin in CI and release, wheels, one `Document` pyclass, hayro patch upstream, parity harness in CI | 1 |
| 1 | Content walk, paint, colour, spaces, functions, text, contrast, resolution | 3-4 |
| 2 | Rust CLI for `verify`: clap, TOML profiles, exact flag and output parity, tool orchestration | 1 |
| 3 | `inspect`, `extract`, `crop`, `preview`, `layout`, `texlog` | 3-4 |
| 4 | Test migration (18 of the test files reach `_internal`), Docker, release | 1-2 |

## If it goes ahead

Port bottom-up behind the existing Python API, so `proof_pdf.core` keeps working and the suite stays green throughout. Move whole
checks, not single operators: 3M operators should never cross the Python boundary. Keep `baseline.py` and `compare.py` as the
gate for each one. The Rust CLI comes last, once no check is left in Python.

The public API does change in one place: `crop_page` documents `pikepdf.PdfError`, and `cli.py` catches it.

## Bugs found in the current Python

Both reproduce today and neither is fixed yet.

1. **A password-protected PDF crashes `verify`.** `core._check_pdf` catches `pikepdf.PdfError` and promises "a FAIL, not a
   crash", but `PasswordError` is not a subclass of it. `cli.py` has the same `except` at three sites.
2. **`--allow-k-only` can pass an image that prints full CMY.** `_cmy_problem` reads Pillow's pixels, which are inverted for an
   Adobe CMYK JPEG, and ignores `/Decode`. The two usually cancel out, so the fixtures pass. With `/Decode` removed from
   `gray-k-only-image.pdf`, Ghostscript prints `(255, 255, 255, 75)` and `check_gray_only` reports nothing. 3 of the 11 real CMYK
   JPEGs here have no `/Decode`, including both Altona files.

## Rerunning

Rust is not installed on this machine; the toolchain used here was temporary. With one installed:

```bash
cd spikes/crabify && mkdir -p vendor
cp -R ~/.cargo/registry/src/index*/hayro-syntax-0.9.0 vendor/hayro-syntax
patch vendor/hayro-syntax/src/xref.rs hayro-info.patch
(cd parity && cargo build --release) && (cd codecs && cargo build --release) && (cd proofcore && cargo build --release)
cd ../.. && .venv/bin/python spikes/crabify/baseline.py base.jsonl tests/fixtures/pdfs/*.pdf
.venv/bin/python spikes/crabify/compare.py spikes/crabify/parity/target/release/parity hayro base.jsonl --detail 10
```

The pdf.js corpus is `test/pdfs` from `mozilla/pdf.js`.
