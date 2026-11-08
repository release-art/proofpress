# Brief: move proof-pdf's core to Rust, on a strict parser of our own

You are working in `/Users/ilo/Devel/Release.art/proofpress`, the open-source `proof-pdf` package (Python 3.15, pdm,
Apache-2.0), on branch `dev/crabify`. Read `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md` and `docs/notes/rust-port-spikes.md`
first. The note holds the evidence; this brief holds the decisions and the plan. Where they disagree, this brief wins.
`docs/README.md` is the index: the decision log, open questions, and ideas not yet scheduled.

The end state: a Rust core that reads PDFs and runs the checks, a Rust `proof-pdf` CLI, and a Python module that calls the same
core. pikepdf, Pillow and typer stop being runtime dependencies.

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Reading PDFs | Our own strict parser, no repair and no encryption | A verifier reports a broken file, it does not fix it. A 808-line spike passed the tests below |
| Writing PDFs | Our own small writer, on the same object model | `crop` and `vector.py` copy objects between files. `lopdf` stores reals as f32 and panicked on 3 hostile files. Fall back to it only if the writer spike fails |
| Previews | `hayro`, unpatched | Renders 116/116 pages close to Ghostscript. Leniency is fine there. It brings its own `hayro-syntax`, which never reaches a check |
| Images | `zune-jpeg`, `hayro-jpeg2000`, `hayro-jbig2`, `hayro-ccitt` | About 20,000 lines nobody should rewrite |
| ICC | `lcms2`, statically linked | Exact against Pillow, 1296/1296 on two real CMYK profiles. It is C, so "fully Rust" has this one exception unless `moxcms` works out |
| Python binding | PyO3 and maturin | Builds a `cp315` wheel today |

**Open, default assumed:** veraPDF and Ghostscript become optional tools, called when installed. Nothing in-process can replace
them yet (see Non-goals). This one is the user's to confirm. If they want both replaced, that adds roughly 12-20 weeks, by
judgement, and needs its own brief.

## Non-goals

- **Repair.** A file that needs it fails, with the reason.
- **Encryption.** PDF/X and PDF/A forbid it. An encrypted file fails, password or not.
- **Speed.** A normal `verify` is mostly veraPDF's JVM start. Do not justify work with benchmarks.
- **A PDF/A validator.** `pdfpurr` got 8 of 116 verdicts wrong, 6 of them false passes. "Passes veraPDF" is the claim that counts.
- **A CMYK compositor.** Peak ink needs CMYK blending that `hayro` cannot give, about 4-8 weeks by judgement.

## House rules (non-negotiable)

- **Git is the user's.** Never run `git add`, `commit`, `stash`, `push` or any other git write; read-only `git status/diff/log`
  is fine. Leave all work uncommitted in the working tree. Do not ask the user about git.
- **No invented numbers.** A threshold from a printer's spec is a parameter with no default.
- **An unconfirmable check fails, it is never skipped.** This includes a requested check whose tool is not installed.
- **Findings, not exceptions.** A malformed PDF produces a finding, never a panic. A panic on any input is a bug to fix, always.
- Match the surrounding code. Lazy and small beats clever; no speculative abstractions.
- Python toolchain, all must pass: `pdm run ruff format src tests`, `pdm run ruff check src tests`, `pdm run pyright` (0 errors),
  `pdm run pytest`, then `bin/docker/test.sh`.
- Rust toolchain, all must pass: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.
- Docs move with code: `README.md`, `CHANGELOG.md` (under `[Unreleased]`) and `CONTRIBUTING.md`.
- Ask the user only when genuinely blocked. Otherwise pick the sensible default, state it, and continue.

## Layout

One crate until something forces a split; the spike's `proofcore` showed that one library can feed both targets.

```text
rust/
  Cargo.toml             features: python
  src/lib.rs             public: open a document, run checks, return findings
  src/pdf/               parser, object model, filters, writer
  src/content.rs         the content-stream walk
  src/checks/            one file per check module, mirroring _internal/
  src/python.rs          the PyO3 module, behind the feature
  src/bin/proof-pdf.rs   the CLI
```

`proof_pdf.core`, `models` and the other public modules stay as thin Python over the extension, so the public API holds. Open a
document once in a `Document` pyclass and move whole checks across the boundary. Never expose single operators to Python: a large
file has millions.

## The parser

Start from `spikes/crabify/parity/src/strict.rs`. It is spike code, so keep what works and do not treat it as finished.

**Accepts:** xref tables, xref streams, hybrid files, `/Prev` chains, object streams, Flate, LZW, ASCII85, ASCIIHex, RunLength,
PNG and TIFF predictors, the page tree with inherited attributes, content streams and inline images. Image filters stay encoded
until a check decodes them.

**Refuses, with a reason:** anything else. It reads every object reachable from the trailer when it opens, because what nothing
references cannot print.

Each refusal becomes a finding under the existing `structure` check. These are the reasons the spike met on pdf.js and Altona:

| Reason | Files |
|---|---|
| Encrypted | 11 |
| `startxref` does not point at an xref section | 10 |
| Stream `/Length` is wrong | 5 |
| No `startxref` in the last 1024 bytes | 4 |

Others met at lower counts include a bad object offset, no `%PDF-` at byte 0, damaged Flate data, and a self-referencing object.
Every refusal must name the object or offset, so the file can be fixed.

**Decide error or warning for each reason during stage 2.** Start from error. Move a reason to warning only when a real file that
prints correctly trips it, and say why in the code. Two likely first cases: a wrong trailer `/Size` (59 pdf.js files), and `stream`
followed by a bare CR (22 `corpus/` files, 20 of them from one producer, PDF4NET).

Still to do:

- Accept references to missing objects, which are legal and read as null, and keep the finding from bug 3 below working: a `Do`
  on a missing XObject is a hole in the page.
- Check for an end of line before `endstream`, not only after `stream`. veraPDF rule 6.1.7.1-2 requires both, and a real office
  report had 21 of 22 streams without it. Strict and Kura both missed it. Treat it as a finding, not a refusal.
- Explain the 3 pdf.js files strict opens that pikepdf and Kura both refuse (`bug1811668_reduced`, `issue16091`, `issue4573`).
- Make an unknown filter a finding wherever it appears. The `UnknownFilter` files in `corpus/pdf-differences/` place one in each
  kind of stream. One in an object stream refuses the file at open; one in a font, image or form only fails when something reads it.
- Report an inline image that gives a key twice, in full and abbreviated. `InlineAbbreviations.pdf` does; it is read silently here,
  while today's `verify` calls the whole file "corrupt or encrypted" because pikepdf cannot parse it.
- Compare an image's `/Width` and `/Height` with its data. `veraPDFHiResChangedHeight.pdf` passes everything today.
- Find out why 5 files passed here when qpdf had to reconstruct their xref. Do this first; it may be a real gap.
- Report duplicate dictionary keys. They are silently last-wins now, and some of our own fixtures have them, so fix the builder too.
- Replace the inline-image `EI` heuristic, using `/L` where present. One file is refused because of it.
- Add the TIFF predictor below 8 bits.
- Set decompression limits. `bomb_giant.pdf` is the test.
- Add a fuzz target under `cargo fuzz`, and run it for hours, not the 4,000 files the spike managed.

## The parity gate

Every port is proved against the Python it replaces, on the same files, before the Python is deleted.

- `spikes/crabify/baseline.py` and `compare.py`: objects, streams, page boxes and the full walk with its CTM.
- `spikes/crabify/check_parity.py`: findings, exact on check, severity, message and page. Extend it per check.
- `spikes/crabify/oracle.py`: refusals against qpdf's own damage warnings. A refusal where qpdf saw nothing is a suspected false
  reject and needs a reason.
- `spikes/crabify/fuzz.py`: mutated fixtures must never panic or hang.

Corpus: `tests/fixtures/pdfs/`, both Altona files (`bin/suites/fetch.sh`), `corpus/` (`bin/suites/corpus.sh`, see
`corpus/README.md`), the private `proof-pdf-test-db` repository cloned to `corpus/local/test-db`, `test/pdfs` from `mozilla/pdf.js`,
and read-only `/Users/ilo/Devel/BrainyBoo/educational-cards/decks/*/*.pdf`. Never copy those decks or anything from
`proof-pdf-test-db` into the repo, never quote their contents in public, and never redistribute `corpus/stillhq-pdfdb/`, which has no
licence.

Differences that are expected, so do not chase them:

- Objects pikepdf synthesises when it pushes inherited attributes onto pages.
- Indirect `/Length` values, which qpdf inlines.
- Files pikepdf repairs and we refuse. That is the behaviour change, listed in the changelog.
- `--allow-k-only` on an image without `/Decode`, once bug 2 below is fixed.

## Traps

- **pikepdf rewrites the document.** `page.obj.get("/Resources")` sees inherited resources only because of qpdf's push-down, so
  the walk must search the parents itself. `/TrimBox` and `/BleedBox` do not inherit.
- **CMYK images follow `/Decode`,** not Pillow's inversion of Adobe JPEGs. Ghostscript settled this.
- **JPEG decoders are not bit-identical.** `cmykjpeg.pdf` differs by 1 code. `--allow-k-only` tests for exactly zero ink, so say in
  the code how a near-zero value is treated. Do not leave it to the decoder.
- **Corrupt content today raises `PdfError`,** which `core._check_pdf` reports as "cannot open". Define the new finding on purpose.
- **Keep f64 everywhere.** Box and size checks compare against tolerances.

## Stages

Each stage ends with the whole Python and Rust toolchains green. Estimates are judgement from one measured port (`x4.py`, 130
lines to 145), and `paint`, `colour` and `functions`, the bulk of the work, were not spiked.

| Stage | Work | Done when | Weeks |
|---|---|---|---|
| 0 | Fix the four Python bugs. Check the parser on recent InDesign and Illustrator output in `proof-pdf-test-db` | Bugs have tests. Refusals on those files are explained | under 1 |
| 1 | Rust project, maturin in CI, wheels for macOS and Linux, parity gate in CI | A wheel installs from CI on a clean machine | 1 |
| 2 | The parser, finished against the list above | Oracle shows no unexplained refusal. Fuzz runs clean | 1-2 |
| 3 | Content walk, paint, colour, spaces, functions, fonts, text, contrast, resolution, X-4 | Findings exact on the corpus. Python versions deleted | 3-4 |
| 4 | Rust CLI for `verify`: clap, TOML profiles, tool orchestration, optional tools | Flags, config and output match. `test_cli.py` passes against the binary | 1 |
| 5 | Writer, then `crop`, `inspect`, `extract`, `preview`, `layout`, `texlog` | Commands match. Crop pixel-identical. pikepdf and Pillow gone from runtime | 3-4 |
| 6 | Test migration, Docker images, release workflow, remove `spikes/` | Release dry run passes. Docs updated | 1-2 |

Total: **about 10-14 weeks** for one experienced engineer, or about 6-8 to reach `verify` and its CLI (stages 0-4).

Stop after stage 0 if the parser refuses real output that prints correctly, and tell the user what it was. That is the one result
that could change the decision.

## Packaging

None of this was tested beyond a local macOS arm64 wheel, so stage 1 proves it first.

- `pyproject.toml` moves from `pdm-backend` to maturin. The version comes from the git tag today (`tool.pdm.version`), so work out
  how a tag reaches the wheel. `release.yml` pins it with `PDM_BUILD_SCM_VERSION`.
- `release.yml` builds one pure wheel on Ubuntu. It needs a matrix: macOS arm64, Linux x86-64 and arm64 (manylinux).
- Publish the standalone `proof-pdf` binary as a GitHub Release asset too. That is the single binary.
- In the wheel, keep a `proof-pdf` entry point by exposing the CLI's `main` through the extension, so `pip install` still gives the
  command. Untested.
- The `Dockerfile` installs from the lockfile with pdm; `releaseart/full` can copy in the binary instead. Keep Ghostscript and
  veraPDF in the image, because it is the one place they are guaranteed.

## Behaviour changes to announce

- A file that pikepdf repaired silently now fails, with the reason.
- An encrypted file now fails, even with an empty password.
- With the default assumed above, PDF/A and peak ink become opt-in, so a bare `verify` works without the tools. A CI job relying on
  them today must turn them on. This is the biggest change for existing users: put it first in the changelog.
- `crop_page` no longer raises `pikepdf.PdfError`. `cli.py` catches it at three sites. Give the replacement a public exception.
- Previews come from `hayro`, so pixels shift slightly on colour-managed files.

## Bugs to fix first (stage 0)

Fix these in the Python, or the parity gate will carry them into Rust.

1. **A password-protected PDF crashes `verify`.** `PasswordError` is not a `PdfError`, so `core._check_pdf` lets it through, and
   `cli.py` has the same `except` at three sites.
2. **`--allow-k-only` can pass an image that prints full CMY.** `_cmy_problem` reads Pillow's inverted pixels and ignores `/Decode`.
   Removing `/Decode` from `gray-k-only-image.pdf` makes Ghostscript print `(255, 255, 255, 75)` while the check reports nothing.
3. **A page that paints a missing image passes.** `content.lookup_xobject` returns `None` for a name with no object behind it, and
   every check skips it. `corpus/pdf-cabinet-of-horrors/veraPDFHiResWrongObjectID.pdf` points `/Im0` at an object that does not
   exist: veraPDF passes it, and `verify` reports one finding fewer than for the intact file. Add a finding for a `Do` whose
   resource is missing or not an XObject. Kura calls this `page.resources_missing`; ISO 32000-2 7.8.3.
4. **Total ink misjudges spot colours.** Ghostscript's `tiff32nc`, which `_page_tac_peaks` uses, converts each spot to its CMYK
   alternate. That is wrong two ways under the decided definition, one ink per spot:
   - `spot-colour.pdf` reads 160%, and `spot-ink.pdf` 200%, where each spot should count as one ink at 100%.
   - A spot overprinting process ink is dropped: 100% where the press lays down 200%.
   Switch to Ghostscript's `tiffsep` device, which writes one plate per ink, and sum the plates per pixel. In the spike it matched
   today's results on every process-only fixture, read Altona at the same 363%, and got both spot cases right. Do not use
   `-dOverprint=/simulate`: it fixes the overprint case but makes Ghostscript 10.08 fail on Altona. Add the two spot cases as
   fixtures. Note the change in the changelog: jobs with spots can read lower than before.

## Backlog: ideas from Kura

From a read of Kura's check catalogue (`pdfa-engine/core/registry/catalogue.json` at v1.3.0, 266 checks, 76 fixes) and its GWG 2022
profiles. Kura is AGPL, so take ideas only: no code, no profile text, and no numbers copied from it. Write each check from the
public source it cites, and keep the house rule: a threshold is a parameter with no default, unless a cited spec supplies it.

Only five of these were measured against Kura (hairlines, small text, white overprint, total ink, resolution). The rest are
catalogue entries nobody has tested.

**During the port:**

| Idea | Stage | Why |
|---|---|---|
| Give each parser refusal a stable check id (`data-after-eof`, `broken-reference`, `unknown-operator`, `content-stream-error`, `damaged`) | 2 | Profiles can then choose error or warning per reason, which stage 2 has to decide anyway |
| Exit codes: 0 pass, 1 findings, 2 input refused, 3 timeout, 64 usage error | 4 | Today a file that cannot be read and a file with findings both exit 1, so CI cannot tell them apart |
| A "not evaluated" status in the JSON output, distinct from pass and fail | 4 | Makes "an unconfirmable check fails" visible and testable. Contrast already says "not measured" in prose |
| The check list as data: id, title, value type, source clause, and a reason for any default | 4 | Generates the README's check table and a `--list-checks` command, so the docs cannot drift |

**After the port, written once in Rust:**

| Idea | Why |
|---|---|
| Compound press risks: a thin line in more than one ink, small text in more than one ink, small white text, rich black on small text | These smear or fill in when plates misregister. The parts (width, size, inks used) are already measured, so it is combination, not new measurement. Build named checks, not a rule language |
| A separate resolution floor for 1-bit images | `resolution.py` has one floor for every image, so 400 ppi line art passes when it prints jagged. Another parameter with no default |
| GWG 2022 presets as `[profiles.*]` in `proof-pdf.toml`, one per print process | The Ghent Workgroup spec is public, so the numbers come from it with clauses cited. Read the spec itself, not Kura's profiles |
| PDF/X-4 rules for document title, creation and modification dates, forbidden annotations and embedded files | Kura flagged all of these on a real report that `x4.py` passed. Fill the "the rest" row from ISO 15930, clause by clause |
| Font embedding licence (`fsType`) | Embedding a font that forbids it is a legal problem, not a print one. It caused 139 of the refusals in Kura's own benchmark |
| Registration colour (`All`) used outside the marks | Today `All` is mentioned only when a spot ink is disallowed |
| Hidden optional content that would print or would not | Optional content is read only by `inspect` today |
| Negative font size | `corpus/pdf-differences/NegativeFontSize` is a ready test file |
| Barcode quiet zone (ISO/IEC 15416) | Packaging and labels only. Lowest priority |

**From a read of Kura's source.** Three read-only passes over v1.3.0, with the main claims checked against the code and the
binary. Kura reads PDFs with qpdf, recovery on, and uses PDFium only to render. It decides a file was damaged by searching qpdf's
warnings for the word "reconstruct".

| Idea | Stage | Why |
|---|---|---|
| All limits in one place, each per kind of structure: nesting of forms, resources, colour spaces, page tree, outlines; pixels and samples per image, checked from the dictionary before decoding | 2 | We have one `MAX_NESTING`. Pick our own values; do not copy Kura's |
| Say how far a wrong xref offset is off: exact, within N bytes, or nowhere | 2 | Turns a refusal into something a producer can fix |
| Name the security handler when refusing encryption: password, or a DRM scheme such as ADEPT | 2 | More actionable than "encrypted" |
| Warnings, not refusals: a missing binary comment after the header; `/ID` near `/Linearized` differing from the last `/ID`, which means the file was edited after export; data after `%%EOF`, tolerating whitespace and NUL padding | 2 | `000293.pdf` is the data-after-EOF case we refuse today |
| Sub-reasons for content errors: unknown operator (allowed inside `BX`/`EX`), wrong operand count or type, operator out of place, unbalanced `q`/`Q`, `BT`/`ET` and marked content, operands left over, missing resource | 2-3 | Gives the planned `content-stream-error` id a ready taxonomy |
| A generator for pathological seeds, such as forms, patterns and dictionaries nested thousands deep. Replay them and every crash input as an ordinary `cargo test`. Let the fuzz target's first byte pick the check set | 2 | Depth limits must be tested on purpose. Replay runs on stable Rust |
| Turn a panic into an `internal` finding at the PyO3 boundary (`catch_unwind`) | 1 | A panic must never cross into Python |
| Generate the check enum from the check list, so a check with no code fails to compile. A missing or invalid check refuses at load, never runs as "not evaluated" | 4 | Kura does it at runtime, so a missing check degrades silently |
| `schema = 1` in `proof-pdf.toml`, and errors that name the profile and key | 4 | Cheap now, costly once the Rust CLI fixes the format |
| Cap what we read from Ghostscript and veraPDF, kill the whole process group on timeout, and keep partial progress in the timeout report | 4 | Kura kills only the direct child and sets no memory limit |
| Fail the release job on any shared library outside the OS: `otool -L`, `ldd`, `auditwheel`, `delocate`. Add `cargo deny` for licences, so an AGPL dependency cannot slip in, and one check that versions agree | 1 | The single binary needs proving, not assuming |
| A SHA-256 manifest for every `corpus/` folder, verified by `corpus.sh`. Rank run outcomes crash, timeout, refused, accepted, and put the engine version in every results file | 2 | The Cabinet mirror has no pin; Kura's own benchmark page names an old version |

After the port:

| Idea | Why |
|---|---|
| Peak ink averaged over a small window, alongside the single-pixel peak, and the area over the limit | A stray pixel at 301% and a solid 330% field read the same today. Window size is a parameter with no default |
| The output intent's own ink limit, from converting Lab black through its profile: warn when it is above `--max-tac` | The job would breach its own limit by design |
| CMYK images with no black at any pixel but some C, M or Y | Grey converted from RGB without black generation. We check this only in gray mode |
| A glyph that is not a space but draws nothing: no outline and no advance, or a Type 3 glyph that paints nothing | Prints nothing, silently. We catch `.notdef` only |
| Faux condensed or slanted text, and paths whose transform scales the axes differently | `--max-stretch-pct` covers images only |
| Contrast against the rendered background, sampled from the raster and taken through the ICC profile to L* | Removes the "not measured" over images and shadings. Kura finds the background this way, but measures with WCAG on sRGB; do not copy that half |
| Content hidden under later opaque objects | We flag only content outside the MediaBox |
| Bleed coverage on rotated pages | We skip them; with page and object boxes in the same unrotated space, the geometry may simply apply (inferred, test first) |

**Decided 2026-11-07: total ink counts each spot colour as one ink,** at its tint, never through its CMYK alternate. It is a
plate on press, so that is the ink it lays down. The measurements behind it, and the fix, are bug 4 below.

**Not to borrow:** repair and conversion, which is why Kura cannot report a defect it fixes silently; total ink from object colours,
which missed both Altona files; an RGB render converted back to CMYK, which is how Kura's own raster works; text size from the
average of two scales, which misreads condensed type; bleed judged by the furthest object per side, so one background excuses
everything; profile errors and error-level hits that still exit 0; and a general rule language, until a shop actually needs one.

## Evidence so far

| Test | Result |
|---|---|
| Clean fixtures (114) | All accepted |
| Real card decks: 84 files, 10 GB, 4,037 pages, 1.9M operators | All accepted, page counts match pikepdf. Nearly all LuaTeX |
| pdf.js and Altona | 992 of 1,044 accepted. qpdf reports damage on 41 of the 52 refused, the other 11 are encrypted |
| Walk on accepted pdf.js and Altona files | 3.09M operators, 5 page walks differ |
| `corpus/`: 644 files, 59,604 pages | 544 accepted, no crash. One walk error, on a form with an unknown filter, which pikepdf fails too. Of the 100 refused, 56 are encrypted, 37 draw a qpdf warning, 6 pikepdf refuses too, and 1 has junk after its trailer (`000293.pdf`) |
| Mutation fuzz | 4,000 of 4,000 answered, no panic or hang |

Accepted and refused in `corpus/` by producer, encrypted files left out. Producer comes from `/Creator` and `/Producer`:

| Producer | Accepted | Refused |
|---|---|---|
| Acrobat, Distiller | 231 | 4 |
| Unknown or other (includes PDF4NET) | 186 | 34 |
| Ghostscript | 38 | 0 |
| TeX | 36 | 0 |
| QuarkXPress | 27 | 0 |
| Adobe PDF Library | 16 | 0 |
| Illustrator | 4 | 0 |
| LibreOffice | 4 | 0 |
| InDesign | 2 | 0 |

A further 6 are refused by both parsers and have no producer to read.

Still thin: 2 InDesign and 4 Illustrator files, and the stillhq set is old. Recent InDesign and Illustrator jobs belong in
`proof-pdf-test-db`, with a row each in its `samples/PROVENANCE.csv`.

## When you finish a stage

Report what changed, the toolchain results, the parity numbers, and anything refused that nobody expected. Note any place you
departed from this brief and why.
