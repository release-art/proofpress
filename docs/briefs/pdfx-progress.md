# PDF/X plan: progress

The running log of the unattended run that carries out [`pdfx-plan.md`](pdfx-plan.md), started 2026-11-25 with the prompt in
[`pdfx-run.md`](pdfx-run.md). A fresh session resumes from **Status**.

## Stop questions

Answered 2026-11-26 (the PDF/X-4 milestone, end of phase 6): the owner said to carry on to phases 7–10.

Answered 2026-11-26 (oracle escalation, task 5.2, glyph presence 6.5.1 and `.notdef` 6.5.2):
- (a) Render mode 3: a glyph (a font's code) is skipped only when every appearance of it is in mode 3; one visible
  appearance among any number of invisible ones and it is judged (owner).
- (b) Predefined CMaps: bundle Adobe's predefined CMaps and judge their codes like any other (owner); `hayro-cmap`
  bundles them and parses embedded CMaps.
- Settled by the oracle without escalation: a code with no glyph name or the name `.notdef`, GID 0 through a
  `CIDToGIDMap`, CID 0 and a code a CMap does not map are 6.5.2; a name, `cmap` entry (after the `post` table) or CID
  the program lacks is 6.5.1. Unembedded fonts are judged only for what the PDF alone settles (6.5.2).

Answered 2026-11-26:
- 6.27's JPX baseline (oracle escalation, ITU-T T.801 M.9.2, M.11.1): the owner chose to fail plain JP2 files (no `jpx `
  or `jpxb` brand, no Reader Requirements box), as the text reads. Every Altona and Ghent JPEG 2000 stream passes;
  every veraPDF 6-2-8-3 file fails `x4.jpx.baseline`.

Answered 2026-11-25:
- 6.4.3.4's same-name rule for Separations (oracle escalation): the owner chose (a), print elements only, as X4-Q3
  says. Every name counts, process inks included. Altona keeps failing 6.4.3.3 only.

- The ICC registry rule of 6.4.2.1 (oracle escalation): the owner chose (b), a warning. 14 Ghent X-4 positives write an
  ECI profile name under `/RegistryName (http://www.color.org)`; they get `x4.output-intent.identifier-in-registry` as a
  warning and stay positives. Recorded in test-db's expectations with the 12 "Custom" warnings.
- The ICC version reading (by pdfToolbox's verdicts) and Altona under 6.4.3.3 (record it as not conforming to that
  one rule). Answered 2026-11-25: the ICC version reading (by pdfToolbox's verdicts) and Altona under 6.4.3.3 (owner:
record it as not conforming to that one rule).

## Status

- **Phase 9** (the writer and the other commands): the writer, `crop`, `inspect` and `preview` are done; `extract`
  is under way, moved to the core in slices, each parity-checked against the Python as it stood before the slice
  (`document.json` and every written file; vector PDFs compared without their random `/ID`) and committed alone:
  A, pages, document information and XMP, font programs, ICC profiles and output intents (`extract.rs`,
  `_core.extract`); B, images and their soft masks (decoded samples to PNG or TIFF, JPEG, JPEG 2000 and CCITT copied);
  C, the page walk's text runs (with `/ToUnicode`) and spot colours; D, the structure tree and interactive features;
  E, vector artwork (on the writer); F, image thumbnails without Pillow, then pikepdf and Pillow leave the runtime
  dependencies. The Python keeps the layout analysis and the assembly of `document.json`. `layout` and `texlog` stay Python (decision log, 2026-11-26). Phase 8 (PDF/X-5) finished 2026-11-26; see its stage report. Every PDF/X-4, X-4p
  and X-5 row is implemented or skip. Phase 7 (file sets and PDF/X-4p) finished 2026-11-26; see its stage report. The
  owner said to carry on past the PDF/X-4 milestone (phase 6, finished 2026-11-26).

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | (owner) | 34 interpretation defaults accepted |
| 0.2, 0.3 | `c01b3fa`; test-db `3a1b8d0a` | PDF 1.6 Reference and errata; suites README; the letter to GWG is the owner's |
| 0.4 | `222b6c2` | `x4-good` built; the oracle and pdfToolbox 17.1 ("Verify compliance with PDFX-4": 0 errors, 0 warnings) both pass it |
| 0.5 | `7a53c78`, `8f68de5`, `112d0ba`, `39af9a3` | Bugs 1, 2; `trapped` in XMP; `pdf-version` deleted; 6.4.3.3 identical-profile rule |
| Brief bug 3 | `dded7d0` | `xobject-missing`: a `Do` on a missing XObject fails |
| Brief bug 4 | `766fcef` | Total ink from `tiffsep`, one plate per ink; fixture `spot-overprint` |
| Phase 0 report | this file | Below, under "Stage reports" |
| 1.1, 1.2 | `82b4f31` | `rust/checks.toml` (231 checks), `build.rs`, `rust/CHECKS.md`, `rust/clauses/` |
| 1.3 | `d98660b` | Completeness gate, `tests/fixtures/pdfx/expected.toml` |
| 1.4 | `730bbec` | `findings` module: catalogue, `Finding`, outcomes, `Report`, JSON schema 1 with an insta snapshot |
| 1.5 | `a8ebbac` | `checks::verify` (judges nothing yet), Gherkin steps, `x4-base.feature` on `x4-good` |
| 1.6 | `08c3e64`; test-db `a7422dcc` | `bin/suites/pdfx.sh`, `expectations/pdfx.toml` (39 files) |
| 1.7 | `c2aa73a` | maturin, `proof_pdf._internal._core`, release wheels, wheel checks, `checks::guarded` |
| 2.1, slice 1 | `a9da02e` | Tiers 1-5 read files: lexer, object events, filters, xref chain, the walk at open; 15 structure checks |
| (fixtures) | `6c77c09` | `pdfraw.tex` wrote `/Filter` twice; samples rebuilt |
| (owner request) | `33abe20` | Windows x64 in CI and release |
| 2.2, part 1 | `3f7ee45` | 8 X-4 checks implemented (6.6, 6.8, 6.15, 6.25 objects) with 8 negative fixtures and 4 feature files |
| 2.1 | `03ccd61` | An unknown filter is a `stream.filter` finding wherever it appears; `pdfx.py --all` |
| 2.3, part 1 | `803cb78` | 15 structure fixtures; 16 house and structure checks implemented |
| 2.3 | `158d4f7`, `95ebfa3` | Three false refusals and a compressed `/Length` fixed, all found by the qpdf comparison |
| (progress) | `4a7a721` | This file brought up to date |
| 2.3 | `6deb894` | Mutation test on stable Rust; "same rule" files compared; phase 2 report |
| 3.2 | `bf5716a`; test-db `50ca948e` | XMP, date and text-string helpers in `common`; 25 metadata checks (6.9-6.11) with 25 fixtures and 3 feature files |
| 3.3 | `d3f2f13`, `a3b7768`; test-db `650d1bf7` | ICC header helper; 11 output-intent checks (6.4.1, 6.4.2.1) with 12 fixtures |
| 3.1 (part), 3.5 (6.12) | `2371597` | The page tree with inherited attributes; a rectangle helper; 6 box checks with 6 fixtures |
| 3.5 (keys) | `c729133` | 19 checks on keys forbidden anywhere (6.7, 6.13, 6.14, 6.16, 6.18, 6.21, 6.22, 6.26) with 19 fixtures, on a walk of every dictionary with its place |
| 3.5, 3.6 | `4f2c410` | 4 optional content checks (6.24), 2 annotation checks (6.17), 3 TrapNet checks (6.9), with 9 fixtures |
| 3.2 (rest) | `3820c19` | `x4.xmp.conforms` (value types of the predefined schemas) and `x4.info.unlisted-not-in-predefined-schema`; `x4.image.alternates-same-content` is a `skip` |
| 3.3 (registry) | `d63e3d9`; test-db `61b8b555` | The three ICC registry rows, with the registry's 73 reference names built in; 3 fixtures |
| 4.1 (part) | `d15e3a9`; test-db `1117700d` | Tier 6 reads content streams: operands across streams, BX/EX, inline images ended by /L, their size, or the first filter's end-of-data marker (JPEG walked, CCITT decoded with hayro-ccitt); 6.19, 6.23 and 3 house content checks with 9 fixtures |
| 4.2, 4.3 (part) | `b5742dc` | Tier 7 walks pages with a graphics state: paint events with colour space, role, overprint and OPM, `Default` spaces applied, forms and coloured patterns walked with one ancestor set; 5 colour rows (6.4.3.2, 6.4.3.3, 6.4.3.4's alternates) with 6 fixtures; `md5` crate for profile identity |
| 4.3 (spots), 6.20 | `a82f79a`, `ea2c86c` (scope) | 7 spot rows of 6.4.3.4 and both 6.20 rows with 9 fixtures; `x4.spot.colorants-match-devicen` waits for function evaluation |
| 4.4 (part) | `81362d4` | JPEG 2000 header reader (JPX boxes, codestream SIZ); 6 of 6.27's rows with 6 fixtures, agreeing with veraPDF's 6.2.8.3 files |
| 4.4 | `a7499b9`; test-db `a2661129` | `x4.jpx.baseline` (T.801 M.9.2) and `x4.image.jpx-valid` (header only); fixtures now baseline JPX; 3 more fixtures |
| 4.5 | `cd26fbf` | Pre-separated files (6.1.2, X4-Q2): every painting page one colorant, differing between pages; 1 fixture; none in Ghent, Altona or GWG Processing Steps |
| 4.6 (part) | `2d4b7c0` | Tier 7 geometry and font metrics; `outside-media` and `xobject-missing` ported, parity-proved and their Python deleted; `bin/parity/print.py`; 2 fixtures |
| 4.6 (part) | `82d1893` | Colour families and fills in the walk; `Settings` and `Params` for print checks; `overprint`, `white-overprint`, `rich-black`, `k-only-text` ported, parity-proved and their Python deleted |
| 4.6 (part) | `ae91f28` | Page geometry and bleed: 9 checks ported with their parameters, parity-proved and their Python deleted; ICC description reader; Python's `:g` and `repr` for messages; fixtures may give parameters |
| 4.6 (part) | `396aa0d` | The colour-mode checks: spot inks and transparency reasons in the walk, `blend-space`, `spot-ink`, `colour-space`, `rgb-only`, `icc-profile` ported, parity-proved and their Python deleted; inline images' abbreviated colour spaces in arrays spelled out |
| 4.6 (part) | `e3cd459` | Image resolution: placement and sample size in the walk; `image-dpi`, `image-dpi-max`, `image-stretch` ported, parity-proved, `resolution.py` deleted |
| 4.6 (part) | `4295048` | Text: font names, subtypes, embedding and `.notdef` codes; glyph counts and stroke widths in the walk; the six text checks ported, parity-proved, `text.py` deleted; a null `/FontFile` entry counts as absent |
| 4.6 (part) | `9bfdb2f` | PDF functions (types 0, 2, 3 and 4) evaluated in the core; `spot-definition` ported, parity-proved and its Python deleted |
| 4.6 (part) | `0beb109` | CMYK image samples decoded in the core (`zune-jpeg` for JPEG, `hayro-jpeg2000` for JPEG 2000, both MIT or Apache-2.0 and accepted by `cargo deny`); `gray-only` ported, parity-proved and its Python deleted |
| 4.6 (part) | `522af3a` | Colour to L*a*b* in the core through `lcms2` (static, LittleCMS 2.20); `contrast` ported, parity-proved and its Python check deleted (the measuring stays for the inventory and extract reports) |
| 4.3 (rest) | the commit after `522af3a` | `x4.spot.colorants-match-devicen` (6.4.3.4, a should): each `/Colorants` Separation compared with its DeviceN colorant alone at four tints, in Lab; sampled functions interpolate (ISO 32000 7.10.2); 1 fixture |
| 4.7 | `f88d01b` | Phase 4 test material: Ghent's 39 X-4 claims (transparency, DeviceN, ICC v4, JPEG 2000, 16-bit, overprint) and Altona clean under every colour check; the veraPDF "same rule" colour files agree (see Deviations); §3 items 4, 5, 16 and 19 built, the last being 16's positive `x4-transparency-no-group` |
| 4.4 (rest) | `ec48911`, `90e82c4` | `x4.image.dct-valid` and `x4.image.jbig2-valid` (6.8) at the marker and segment level, `/JBIG2Globals` included; 2 fixtures |
| 5.1 | `8ee8994` | 6.5.4's four TrueType encoding rows, the `cmap` count from the program's table directory (`common::sfnt`); the walk's fonts carry their object number; 4 fixtures |
| 5.2 (part) | `3f971fd` | `x4.font.embedded` (6.5.1): a font any visible text run uses is embedded, render mode 3 exempt; 2 fixtures |
| 5.2, 5.3 | `c72d1aa` | Font programs read with `read-fonts` (TrueType and OpenType tables, bare CFF, Type 1) and CMaps with `hayro-cmap` (Adobe's predefined CMaps bundled); `x4.font.glyphs-present`, `x4.font.no-notdef` and `x4.font.widths-consistent` (6.5.1 to 6.5.3); test font programs of our own in `tests/fixtures/fonts/`; 5 fixtures |
| 6 (rows) | `38c2d71` | The X-4 rows the milestone needs that phases 2 and 4 left: 6.25's lexical and graphics limits (the walk now reports each page's deepest graphics-state nesting) and 6.6's recommendation for other names; 7 fixtures |
| 6 (rows) | `7aacf55`, `53843a0` | `x4.pdf-reference.conforms` (6.1.1) implemented: it was coded in phase 2 to fail on every error the reading tiers find, and waited for the content tier, which now reports; 9 fixtures' expectations gain it |
| 6 (rows) | `111c62d` | `x4.complete.resources-inside` (6.3): resource names content uses that its resources do not define, and composite fonts whose CMap is neither embedded nor in ISO 32000-1 Table 118; 1 fixture. Every X-4 row that needs no external file is now implemented |
| 6.1 (part) | `2cff6ac` | The Rust `proof-pdf verify` (feature `cli`, `src/cli/`, binary `src/bin/proof-pdf.rs`): the Python command's options, the print checks they switch on and the PDF/X level (`--pdfx x4` by default, `--no-pdfx`), files and directories with `--exclude` and `--exempt`, `--jobs` threads, the text report and `--json`, and the brief's exit codes. Config files, veraPDF and Ghostscript are next |
| 6.1 (part) | `68fbc6c` | The config file for the Rust `verify`: `proof-pdf.toml` or `[tool.proof-pdf]` in `pyproject.toml`, found walking up or named with `--config`; `schema = 1`, profiles with `--profile` and `extends`, the Python key spellings, `--show-profile` |
| 6.1 (part) | the commit after `68fbc6c` | veraPDF (`--pdfa`) and Ghostscript (`--tac`) for the Rust `verify`, through `process-wrap` (a process group, or a job object on Windows, killed on timeout or drop), beside the core's checks; peak ink read from `tiffsep`'s uncompressed plates with `tiff`; a timeout exits 3. `checks.toml` gains a `tool` field: `pdfa` and `tac` are implemented, tested with stand-in tools rather than fixtures |
| 6.1 | the milestone commit | `test_cli.py` rewritten against the Rust command (behaviour on real fixtures; option parsing is tested in Rust); the Python `verify` passes its arguments through `_core.run`; `--no-config`; the Python spellings `--no-pdfa`, `--colour`, `--fonts`, `--missing-glyphs` kept |
| 6.2 | the milestone commit | Levels `--pdfx x4\|x4p\|x5g\|x5pg\|x5n` (in `2cff6ac`), `--no-pdfx` and `pdfx = false`; a level's `todo` rows are reported as not evaluated, so X-4p and X-5 cannot pass yet; the text report lists unjudged checks by reason |
| 6.3 | the milestone commit | `core.verify_file`/`verify_files` keep their async signatures over `_core.run`; `config.load_profile` reads the profile the Rust command resolves (`--show-profile`); the Python veraPDF, colour, ported and PDF/X modules are deleted |
| 6 (fix) | the milestone commit | `page.media-box` (ISO 32000-2 7.7.3.3, every level; fixture `x4-no-media-box`): a page without a MediaBox passed once the Python `boxes` check went; `bleedbox`, `boxes`, `structure` become `skip`; an empty FlateDecode stream reads as empty |
| 7.1-7.3 | the phase 7 commit | `Settings.files` (alone or a file set rooted at the PDF's folder), `common/url.rs` (local or network, never fetched), `checks/x4p.rs`: the 12 Annex A rows, the external profile found by `/CheckSum`, judges that may answer "not evaluated" (`JUDGES`); the X-4 colour-space rows and 6.4.3.3 under X-4p; 6.7's `/URLs` exemption; `--file-alone` and `file-alone` |
| 7.4 | same commit | 19 `x4p-*` fixtures (the Little-CMS profile beside them by a relative URL), both modes; `expected.toml` gains `alone` and `unjudged`; feature `x4p-annex-a` |
| 8.1 | the phase 8 (X-5g) commit | The inheritance map as `applies` (140 rows reach the X-5 levels), `x5.identification-value`; the `/URLs` exemption of 6.7 made unconditional; three rows settled as warning or skip |
| 8.2, 8.3 | same commit | `checks/x5g.rs` (proxies, `/Ref`, `/ID`, metadata, `RenditionOf`, the X-5 6.7 rule), `checks/targets.rs` (the file set, read once and verified recursively; nine rows), `common/labels.rs`; `Location::File`, `Limits::target_depth`, `--target-versions`; 39 fixtures `x5*`, feature `x5g-clause-8`. X-5g and X-5pg: every row implemented or skip |
| 8.4, 8.5 | the phase 8 (X-5n) commit | X-5n's 15 rows (`x4p.rs` reference and profile rows, `colour.rs`/`jpx.rs` device bans, `spots.rs` DeviceN attributes, `modes.rs` group `/CS`, `annots.rs` TrapNet), `icc::colorant_names`; `x5.n-subtractive` skipped; two generated n-colorant v4 profiles (`tests/fixtures/icc/xclr-5.icc`, `xclr-6.icc`) and 21 fixtures `x5n-*`, feature `x5n-clause-7` |
| 9 (part) | the writer commit | `writer.rs` (graft map, raw streams, damaged objects reported, Identity `/Crypt` dropped) and `writer/crop.rs`; `_core.crop` and `UnreadablePdfError`; `proof_pdf.crop` no longer uses pikepdf. Parity: 588 crops of fixtures and test-db samples rendered by Ghostscript at 72 dpi; 586 pixel-identical, 2 damaged files now refused (pikepdf had repaired them); 14 skipped (pikepdf or Ghostscript could not open them) |
| 9 (part) | the inspect commit | `checks/inventory.rs` on the print walk and the contrast measure; `_core.inspect`; `proof_pdf.inventory` no longer uses pikepdf. Parity with the Python on the fixtures (339 identical, 5 damaged fixtures read strictly) and 10,475 test-db files (see Deviations) |
| 9 (part) | the preview commit | `preview.rs` (feature `preview`): pages rendered by `hayro` over the MediaBox, guides from a pixel-space overlay PDF, panics caught per page; `_core.render`; `preview` and extract's review page no longer run Ghostscript; `_internal/proc.py` deleted |
| 9 (part) | the extract A commit | `extract.rs`, `_core.extract`: pages (inherited `/CropBox` and `/Rotate`), information and XMP, font programs, ICC profiles and output intents, and the reader's damage, which `extract` now lists in `failures`; also the X-5g `/Ref` fix of another session (below) |

## Register coverage

From `cargo test --test completeness -- --nocapture` (implemented / todo / skip):

| Group | Implemented | Todo | Skip |
|---|---|---|---|
| PDF/X-4 and X-4p | 137 | 0 | 4 |
| PDF/X-5 | 32 | 0 | 21 |
| House and structure checks | 54 | 0 | 4 (`internal`, `bleedbox`, `boxes`, `structure`) |

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-26 (100% lines, regions, functions) |
| Python (ruff, pyright, pytest) | green, 2026-11-26: 1073 passed (the Python checks' own tests went with them) |
| `bin/docker/test.sh` | green at the end of phase 8, 2026-11-26: 1073 passed |
| `bin/suites/pdfx.sh` | 2026-11-26: 39 files, 39 accepted, 0 problems |

## Corpus and oracle

Task 2.3, 2026-11-25: `rust/examples/verify.rs` over every PDF in test-db's `samples/` (44,608 files): no crash, no
timeout. Against `qpdf --check` as the oracle for "is this file readable", 326 disagreements at first; three commits
removed 300 of them. What is left, each a deliberate refusal under PDF 1.7 or genuinely unreadable:
- 19 files whose cross-reference entries are 19 bytes, not 20 (7.5.4); web downloads, no print output among them.
- 3 with two spaces in a subsection header (Isartor and veraPDF test files that fail PDF/A on purpose).
- 2 whose `startxref` is 0 and 1 with none; 1 whose `startxref` sits more than 1024 bytes before the end, with
  unrelated bytes after `%%EOF`.
- 3 whose object stream cannot be decoded, and 1 whose referenced object has an invalid entry: gaps, reported as damage.
- 4 encrypted files, refused as X-4 requires.

2026-11-26, X-5 against pdfToolbox 17.1 ("Verify compliance with PDFX-5g", "-5pg" and "-5n") over the 52 X-5g, X-5pg
and X-5n fixtures: 33 verdicts agree. pdfToolbox fails `x5pg-good` alone, as it does not look for a profile named by a
relative `/URLs` (as at X-4p). It passes 18 that we fail: it opens targets but does not compare their identity, page,
printing condition, full conformance, or another version of them; under X-5n it does not compare `/ColorantTable` or
`/ICCVersion` with the external profile. Of the rows that need no target, the oracle upheld four (`/EF` under 6.7, DeviceN
`/Process`, a version 2 n-colorant profile, `/Mac` in `/URLs`) and made `x5.g-proxy-is-form` a warning (decision log).

Every Ghent, Altona and GWG Processing Steps file and every fixture opens sound, so the re-plan point (the parser
refusing real print output) has not triggered.

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-25:
- PDF/X 6.25 is the PDF 1.6 Reference's Table C.1 only: no array or dictionary limit, no page size (task 0.2).
- Ghent's X-4 positives are the 35 of 39 claims pdfToolbox passes; GWG151 and GWG152 fail 6.24 (oracle agreed, no
  escalation).
- Altona fails 6.4.3.3 only (two hits per file) and stays a positive otherwise; owner approved; plan edited.
- X4-Q6 stands: no corrigendum to ISO 15930-7:2010 found. 15930-8 Cor 1:2011 was supplied by the owner on 2026-11-26
  (test-db `15fad03e`): it corrects the `stRef` spellings of 8.3 and 8.4, as X5-Q6 read them.
- ICC profile version (6.4.2.1, 6.4.3.3): major version 2 or 4 passes, any minor; 5 fails. From pdfToolbox's verdicts on
  seven probes (`x4-good` with only the output profile's version field changed: 2.0, 2.1, 2.4, 4.2, 4.3, 4.4 pass; 5.0 fails,
  "newer than version 4"). The profile was confirmed to bite: 12 errors on `good.pdf`, 2 on `pdfx-x4-rgb-image.pdf`.

Third-party preflight (pdfToolbox Server 17.1 CLI, trial, "Verify compliance with PDFX-4"), 2026-11-25:
- `x4-good`: pass, no hits. Settles the owner action of task 0.4.
- `good.pdf`: 12 errors (monitor-class intent, DeviceGray under it, Trapped only in Info, missing XMP dates, title and IDs),
  as `x4-test-material.md` §1 predicts.
- `pdfx-x4-rgb-image.pdf`: DeviceRGB under a CMYK intent, as task 0.4 corrected; also an Info/XMP Keywords mismatch
  (6.10.2) that nobody had listed.

Task 3.2, 2026-11-25:
- Dates (6.10.2) compare as instants in UTC when both carry a zone, as written when neither does, and never equal when
  only one does; fractions of a second in XMP are dropped, since PDF dates cannot carry them.
- Info text strings are read as PDF 1.6 defines them: PDFDocEncoding or UTF-16BE with a byte order mark, no UTF-8.
- Ghent and Altona: 38 of the 39 positives carry `xmp.did:` or `adobe:docid:` document IDs, which 6.10.4 recommends against
  (it says *should* be a uuid-schemed URI). Classified (b): the `x4.xmp.document-id-uuid` warning is right and the verdict
  stays pass; test-db's `expectations/pdfx.toml` records the warning per file. The clause is explicit, so no oracle call.
- `x4.xmp.conforms` (value types of the XMP 2005 schemas) and `x4.info.unlisted-not-in-predefined-schema` (needs the list
  of predefined schema properties) stay `todo`.

Task 3.5, the forbidden keys, against the veraPDF and BFO "same rule" files (44 files): every verdict agrees with the
X-4 reading except these, each explained:
- PDF/A-2b 6-2-5 t03-fail-b: a non-primary type 5 component *without* `/TransferFunction` fails PDF/A; X-4 (X4-Q9) only
  limits where one may be, so it passes. Different rule.
- 6-2-5 t05 (`/RI`): 6.23, rendering intents, waits for phase 4 with the `ri` operator.
- 6-5-1 t02-pass-a: Named actions in outlines pass PDF/A and fail X-4, as the register expects.
- BFO 6-10 t01: its name dictionary is object 16 while `/Size` is 16, so the reference is invalid (7.5.5) and reads as
  null, with an `object.dangling-reference` finding; the file is malformed, classified (b).
Two fixes came from it: the obsolete `SetState` and `NOP` actions are actions, and an indirect `/AA` dictionary's actions
are found (the walk gives an indirect object's entries the keys the object is referred to under).

Task 3.6, GWG Processing Steps 1.0 (41 files, none claims PDF/X): the box and optional content checks give what
test-db's `x4-test-material.md` predicts. 28 patches have both a TrimBox and an ArtBox, 8 have neither (and so does
the suite's own document), 2 have the TrimBox outside the BleedBox, 2 more use an ArtBox alone (a warning); every
optional content configuration passes. GWG151 and GWG152 now fail `x4.oc.order-complete` as expected.

Oracle consultations:
- The ICC registry rows of 6.4.2.1 (2026-11-25): escalated; see "Stop questions". Its other verdicts: an absent
  `/RegistryName` passes; another registry's URL with an ICC name is a warning at most; a misspelled color.org URL with
  an ICC name fails; "Custom" (trimmed, any case) or an empty identifier is a warning. Aside, for test-db: 43 generated
  mutants pair `http://www.color.org` with "sRGB IEC61966-2.1", which is not a registry name.
- Empty `/RegistryName ()` with `/OutputConditionIdentifier (Custom)` (12 Ghent positives, 6.4.2.1): no "shall" broken;
  empty reads as absent; medium-high confidence; no escalation. The "should not use Custom" recommendation is
  `x4.output-intent.identifier-registered`, still `todo`: count its hits on the Ghent set before implementing it.
- The 3 Ghent DeviceN ReadMe files lack `/OutputCondition`, which 6.4.2.1 says should always be present: the warning is
  right, classified (b), and recorded in test-db's expectations.
- GWG151 and GWG152 under 6.24: do not conform (their `/D /Order` leaves out content OCGs); high confidence; classified
  (b) without escalation. veraPDF's PDF/A-2 rule 6.9-3 fails them too.
- Altona under 6.4.3.3: does not conform (DefaultCMYK equal to the intent profile remaps a painted DeviceCMYK
  image); high confidence; escalated because the plan lists Altona as a positive; the owner approved recording it.
- Task 0.4, `x4-good` against every X-4 row: conforms, high confidence; escalated the ICC version reading, which
  pdfToolbox then settled in line with the oracle's suggestion.
  Its aside (font `name` table lacked IDs 3-6, `FontBBox` did not match the glyphs) was fixed before commit.

## Deviations from the plan

- Task 2.2's 6.25 line named array and dictionary limits; Table C.1 of PDF 1.6 has none, so the plan text was corrected.
- Task 0.4 needed a real embedded font; `fonttools` became a development dependency to build one, rather than vendoring a
  third-party font.
- Phase 0 also carried the brief's four stage-0 bugs (plan 0.5 says "on top of"); all four are fixed.
- `checks.toml` lists X-4's reader and process rules only as the 3 rows no file can show (the plan's "2 in X-4", plus
  the mixing-hints recommendation); the register's §2 bullets are not entries. X-5 levels appear in `applies` only on
  X-5 rows for now; which X-4 rows each X-5 level inherits is task 8.1.
- The check catalogue lives in a new base module, `findings`, not in `checks`: every tier raises findings (task 1.4).
- Task 1.7: the parity gate in CI waits for the first ported check (phase 4): with nothing ported there is nothing to
  compare, and the spike's `check_parity.py` compares the spike, not this crate. Wheels are proved locally (macOS arm64:
  built from the sdist, installed in a clean venv, only OS libraries, CLI runs); the CI matrix (manylinux x86-64 and
  arm64, macOS arm64) is written but cannot run until the owner pushes, so "a wheel installs from CI on a clean
  machine" is unverified.
- Task 2.1: tiers publish to listeners only once an observer needs it. For now tier 2 is a pull parser whose events tier
  4's and 5's reader consume directly, and each tier turns its own notes into findings inline (`syntax::note_finding`,
  `file::refusal_finding`, ...). The `Limits` defaults are provisional until task 2.3 measures them.
- `catch_unwind` is in place as `checks::guarded` with its `internal` check (status skip: no file is meant to trigger
  it); `verify` wires it in with the first real check.
- Task 2.2: `x4.limits.lexical` (numbers, strings, names) waits for tier 6: content-stream strings need inline images
  read correctly, so all of 6.25's lexical row moves to phase 4 rather than shipping half. `x4.limits.objects` counts the
  distinct object numbers the sections list as in use (Table C.1: indirect objects in a file). `x4.pdf-reference.conforms`
  is coded (every tier error becomes a 6.1.1 finding) but stays `todo` until the content and font tiers report too.
- The extension is `proof_pdf._internal._core`, not `proof_pdf._core`: every private module lives in `_internal`.
- Fuzzing: no `cargo fuzz` target. It needs a nightly toolchain and `cargo-fuzz`, neither installed nor in the toolchain
  file. Instead `rust/tests/mutation.rs` mutates every built fixture with proptest on stable Rust (byte flips, cuts,
  repeats, truncation, spliced PDF tokens) and requires a report with no `internal` finding within 10 seconds; failures
  shrink and replay as an ordinary `cargo test`, as the brief's replay row asks. 512 cases in the gates;
  `PROPTEST_CASES=5000000` for a long run. A coverage-guided `cargo fuzz` target can follow if the owner installs nightly.
- Decompression limits: every filter decodes within `Limits::decoded_bytes` (1 GiB), tested per filter. The brief's test
  file, pdf.js's `bomb_giant.pdf`, is a JBIG2 bomb; tier 3 leaves image filters encoded, so it reads in milliseconds and
  2.5 MB. It becomes a real test when an image decoder arrives (phase 4 onwards), and stays in the corpus run until then.
- Task 3.1 is done in part: the page tree with inheritance. Its Python mapping waits for a Python caller that needs
  objects (the parity gate, phase 4), and the typed views and shape checks generated from the Arlington PDF Model
  (Apache-2.0, `cdc485e` the candidate pin) wait until a row needs them: the X-4 rows read a handful of keys each, and
  generating views for every PDF object type now would be code without a caller.
- Task 3.3: the mutation test timed out under coverage now and then. The cause was `x4-too-many-objects`: its
  cross-reference stream lists 8,388,608 entries, which took 12 seconds to decode in a debug build before the object
  limit was checked. A cross-reference stream whose `/Index` declares more entries than `Limits::objects` is now refused
  before its data is decoded, and the mutation test runs with a limit of a million (`checks::verify_with`), so that
  refusal is fuzzed too. 2,000,000 release cases ran clean.
- Task 3.3: the ICC helper reads the header only (class, data colour space, version). The Profile ID, the MD5 of
  ICC.1 7.2.18 and the `desc`/`clrt` tags wait for the rows that read them, `x4.icc.cmyk-not-intent-profile` and
  `x4.icc.profile-version`, which follow print elements (X4-Q3) and so belong to phase 4's colour evaluator.
  The ICC registry rows came later in phase 3, with the registry's names built in.
- `x4.output-intent.identifier-in-registry` is a warning, not the clause's error (owner, 2026-11-25).
- Task 4.1: the content streams of a page are read as one buffer joined by white space, since 7.7.3.3 lets an array
  span two streams (Ghent GWG161 does). A token split across streams therefore reads as two tokens and is not reported.
  After `ID`, CR LF is accepted as the one white-space byte only when the data then ends exactly at `EI` and the
  one-byte reading does not. Over 4,484 corpus files the reader leaves 13 contents unread, each a broken test file
  (bomb, JPX tests, deliberate syntax errors, a wrong `/L`).
- Task 4.2: tier 7 carries what the colour rows need; geometry (CTM, bounding boxes, clips), soft-mask content, Type 3
  glyph procedures and annotation appearances come with the checks that read them. Each colour check walks the pages
  again; the suite stays at about a second for the 39 positives, so a shared walk waits until it matters.
- Task 4.3: Altona's two `x4.icc.cmyk-not-intent-profile` hits each now come from the Rust evaluator (`DefaultCMYK` on
  the intent's profile), matching test-db's expectations; no Ghent positive raises a colour finding.
- Task 4.3: `PrintingOrder` arrays are consistent when the names two arrays share come in the same order in both (the
  clause does not define it). `x4.spot.confusing-names` knows Red, Green, Blue, Gray and the process inks in five
  languages; the clause's "any translated names" has no complete list. `x4.spot.colorants-match-devicen` (a should)
  needs tint transforms evaluated and stays `todo`.
- Task 4.3, found while testing the scope change: a colour space whose `Indexed` base is itself made the DeviceN scan
  loop forever. Every walk through colour spaces is now bounded, and a test holds a self-referring space.
- Task 4.4: every 6.27 row reads only the JPX header boxes and the codestream's SIZ marker; nothing is decoded, so
  `x4.image.jpx-valid` judges the header's structure and says so. `x4.jpx.baseline` judges T.801 M.9.2 as the oracle
  read it: brands, Reader Requirements, compression type, `Rsiz`, a usable colour specification, box order; files with
  fragment tables or data references fail as not verified. M.9.2.4's colour list keeps CIEJab, so a CIEJab file raises
  only `x4.jpx.no-ciejab`. The 2021 T.801 changes nothing in M.9.2's baseline.
- Task 4.6, the first port (`outside-media`, `xobject-missing`). Parity on 10,013 files (every test-db sample set but
  `kura-bench` and `iust-pdfcorpus`, and the private decks; the fixtures separately, 244 identical): 9,249 identical, 1,242 findings among
  them, 2 different, both Rust's way. pdf.js `issue6541` paints `/R41` inside a tiling pattern whose own resources do
  not name it; the Python never walks patterns. An `unsafe-docs` file's `/Resources` object reads as empty in qpdf and
  whole here, so the Python called its eleven images missing. The rest were apart, as the brief expects: 465 files the
  core refuses and pikepdf repairs (all but at most the 26 that task 2.3 explained are files `qpdf --check` reports
  damaged), 61 read with gaps, 220 pikepdf cannot open,
  and 20 on which the Python walk crashes (a colour space name that is not UTF-8, a `TypeError` or `IndexError` in
  `paint.py`), an exception `core._check_pdf` does not catch, so `verify` fails on those files today.
- Task 4.6, the second port (`overprint`, `white-overprint`, `rich-black`, `k-only-text`, at the profile's
  defaults): parity on the same 10,013 files, 9,246 identical with 39,851 findings among them, 4 different, all
  `k-only-text` on damaged files: one where pikepdf stops a 2 MB content stream after 31 operators without an error and
  the core reads the whole page; one whose content is damaged partway, where the core stops at the damage by design;
  two whose trailer `/Size` stops short of the objects the pages use, which the core reads as null (task 2.1's
  decision) and qpdf reads anyway. Python's overprint is one flag, `OP` or `op`; its text colours include the fill
  in every render mode and count invisible text; the K-only message counts colours, not runs. All kept for parity,
  to revisit once the port is done.
- Task 4.6, the third port (`geometry`, `bleed`, `centring`, `page-count`, `page-size`, `page-rotate`,
  `output-condition`, `artwork-bleed`, `outside-bleed`, with an A4 geometry, 4 mm binding bleed, uniform pages in
  fours and FOGRA39): 9,205 files identical with 67,381 findings among them, 65 different, every one a file whose
  objects lie beyond its trailer's `/Size`, which the core reads as null (task 2.1's decision) and qpdf reads anyway.
  In 38 of them the catalog itself is beyond `/Size`, so the core sees no pages and the file reads as sound with
  nothing to judge; see "Owner actions".
- Task 4.6, the fourth port (`blend-space`, `spot-ink`, `colour-space`, `rgb-only`, `icc-profile`, with no spot
  allowed, a CMYK job, and an RGB job for `rgb-only`): 9,226 files identical with 184,735 findings among them, 12
  different. Four were ours: an inline image's abbreviated colour space inside an array (`[/I /RGB ...]`) was not
  spelled out; fixed, and three of them now agree. The rest are reading differences already decided: content the core
  stops reading at damage (three, one of them an inline image with the unknown filter `/Lzw`), objects beyond
  `/Size` or listed free (four), qpdf misreading a `/Resources` object (one, Rust right), and forms nested past the
  Python walk's 12 levels (one veraPDF limits file; the core goes to 100). `colour-space` and `rgb-only` now judge
  only their own colour mode, so the core's report holds no RGB-job finding on a CMYK job.
- Task 4.6, the fifth port (`image-dpi`, `image-dpi-max`, `image-stretch`, with a 300 dpi minimum, 450 advisory,
  1200 ceiling and 5% stretch): 9,252 files identical with 123,860 findings among them; the 4 differences are the
  reading differences already explained (damaged content, objects beyond `/Size`, the qpdf misread, the `/Lzw`
  inline image).
- Task 4.6, the sixth port (the text checks, with a 6 pt text floor, 0.25 pt stroke floor and 3 mm safe area):
  9,181 files identical with 124,336 findings among them, 67 different. Two were ours: a font descriptor's
  `/FontFile3 null` counted as a font program, where ISO 32000-2 7.3.7 reads a null entry as absent; fixed. Fourteen
  are fonts whose `/BaseFont` is not UTF-8 (Thai, Shift-JIS, mangled names): Python's `str()` raises, its font loader
  falls back to an unknown font with guessed metrics, and it skips the font in the embedding check; the core reads
  the font and reports it under a lossy name. The rest are the reading differences already explained (49: objects
  beyond `/Size` or listed free, damaged content, pikepdf stopping a content stream early, the qpdf misread).
- Task 4.6, the seventh port (`spot-definition`, with PDF function evaluation in the core): 9,261 files identical
  with 16 findings among them, none different.
- Task 4.6, the eighth port (`gray-only`, K-only CMYK allowed, so every DeviceCMYK image is decoded): 9,242 files
  identical with 175,623 findings among them, 8 different. Seven are the reading differences already explained (damaged
  content, pikepdf stopping a content stream early, the `/Lzw` inline image). The eighth is a form whose `/Resources` is
  present but empty and whose content names `/CS0`: the Python's `form.get("/Resources") or resources` treats the empty
  dictionary as false and resolves `/CS0` in the page's resources, where the core reads the form's own resources
  (ISO 32000-2 7.8.3) and finds no such space; the file does not conform. No decoded image differs: `zune-jpeg` and
  `hayro-jpeg2000` give Pillow's samples (YCCK converted with libjpeg's integer tables). A JPEG that stops before its
  end-of-image marker, and damaged JPEG 2000 data, count as unreadable, as Pillow refuses them. With K-only CMYK not
  allowed: 9,242 identical with 180,086 findings, 9 different, the same eight and one more file already explained in the
  sixth port. The Python's tests of its decoder internals (decoded once, never decoded in strict mode) became a core
  unit test and a test of the strict-mode message.
- Task 4.6: the core is slow on a few corpus files whatever is checked: four files take 38 to 94 seconds for `print`
  (release build) and longer for `x4`, where the Python gray check alone took 3 to 14. One of them passed the parity
  script's 120-second limit under load and was counted apart. Each print check walks the pages again (12 calls to
  `graphics::walk` in `checks`); worth measuring at the `paint.py` pace checkpoint (2026-12-02).
- Task 4.6, the ninth port (`contrast`, with a 100 L* minimum so every measured pair is reported): 9,241 files identical
  with 48,640 findings among them, 6 different, all explained. Three are the reading differences already explained. One
  output intent's ASCIIHex profile has no `>` end-of-data marker (ISO 32000-2 7.4.2), so the core has no CMYK profile
  there; the file does not conform. One file's fonts have names that are not UTF-8, which the Python reads as unknown
  fonts with one-byte codes, so its text boxes, and the grounds under their centres, differ. In one, pikepdf ends three
  ASCII85 inline images at an `EI` inside other data and misses them, where the core ends them at the filter's
  end-of-data marker, so text centred on them is unmeasured in the core. Converted colours match Pillow's exactly. The
  Python's CIEDE2000 is not ported: the check does not report it, and the inventory, which does, stays in Python for the
  CLI stage.
- Task 4.3, `x4.spot.colorants-match-devicen` (pdfx-oracle, 2026-11-26): 6.4.3.4 asks that `/Colorants` Separations
  be "consistent" with their DeviceN without saying how. Each colorant of a DeviceN that print elements use is compared
  alone (the others at no ink) with its Separation at 25, 50, 75 and 100% tint, both converted to Lab absolute
  colorimetric (DeviceCMYK through the PDF/X intent, DeviceGray as the K plate through it, `ICCBased` through its own
  profile, Lab as itself), and warned past 2 ΔE00. Unconvertible alternates are compared component by component when
  they are the same space, else reported as not comparable; a transform that cannot be evaluated is reported. Process
  colorants of an NChannel space are skipped, and a space with a `None` colorant is not judged: Ghent's SPOT and ALL
  test pages use one as a duotone, whose `None` components carry the look through an Indexed lookup (the oracle's
  reading; the first version warned on both, a false positive). Ghent and Altona raise nothing.
- Task 4.3: sampled (Type 0) functions now interpolate between samples, as ISO 32000 7.10.2 requires, where the port
  (like the Python) took the nearest sample; a cubic `/Order` is read as linear. Values on a sample, all the
  `spot-definition` check reads at full tint, are unchanged. Our fixture builder gave multi-ink DeviceN spaces a Type 2
  function, which takes one input; they now have a Type 4 one (`x4-printing-order-clash`, `x4-solidities-clash`).
- Noted by the oracle, not changed: the ported `spot-definition` house check evaluates a DeviceN at every ink's full
  tint at once, as the Python did, so for a multi-ink space it compares the combined colour rather than each ink's own.
- Task 4.6: `boxes` and `bleedbox` come from the Python PDF/X layer (`pdfx/x4.py`), which the plan does not port; the
  `x4.boxes.*` rows replace them.
- Task 4.6: the profile's on/off switches for print checks stay in Python (`ported.check`); the core runs every
  implemented print check and Python keeps those the profile turns on.
- Task 4.6: the walk now starts a form from the graphics state of its `Do`, colour and overprint included (8.10.1),
  where it used to start from the default; an unbalanced `Q` keeps the state and restores only the CTM, as the Python
  does; an `XObject` entry that is not a stream is missing (7.8.3), not an image. Ghent and Altona are unchanged.
- A check the core cannot judge because it refused the file or read it with gaps now fails in `verify` ("could not be
  checked"), where the Python checked whatever pikepdf repaired: the behaviour change the brief lists.
- Task 4.7, the veraPDF PDF/A-2b "same rule" colour files, each read by the X-4 check it maps to: 6-2-2 (operators, rendering intents), 6-2-4-4 t02 and t03, 6-2-5 t05, 6-2-6 and 6-2-8-1 t03 agree file for file; 6-2-2-t01-fail-a and b fail `x4.content.known-operators` rather than the `BX`/`EX` row, which is right. The six 6-2-4-2 t02 (OPM) files have no output intent, so 6.4.3.3's "CMYK printing condition" never applies; with a CMYK GTS_PDFX intent added they agree. 6-2-10 passes 6.20 throughout (PDF/A's missing blend space is not an X-4 rule), except t02-fail-a, whose `/BM /Custom` fails `x4.transparency.blend-modes`: the same rule, though test-db's register says 2b has no such test.
- Task 4.4 (rest), 6.8's JPEG and JBIG2 validity, read at the marker and segment level (T.81 Annex B, T.88 7.2; neither is held in test-db, so no page numbers). Against `djpeg` and `jbig2dec` on every corpus image under the one filter (31,698 JPEG, 3,252 JBIG2): no image we fail decodes. We pass 5 JPEGs whose entropy-coded data is corrupt (the header-level ceiling, marked `ponytail:`) and 21 JBIG2 images `jbig2dec` cannot decode: pdf.js and pdfium refinement test files, which both render, and pdf.js's file-header case. A JBIG2 file header, which PDF 1.6 3.3.6 says should be removed, is read past when its segments are sequential; the random-access organisation fails.
- Task 5.1, 6.5.4 (oracle, 2026-11-26): only fonts that text runs use are judged (6.1.3's print elements, X4-Q3), whatever their render mode, hidden optional content included; Symbolic is the `/Flags` bit 3 alone, no descriptor counting as non-symbolic; the `cmap` row needs an embedded program, `/FontFile2` or an OpenType `/FontFile3`, and a program without a `cmap` fails it. A font written directly in a resource dictionary is not judged (marked `ponytail:`). veraPDF 6-2-11-6 and Isartor 6-3-7 agree file for file but for 6-2-11-6-t02-pass-d and BFO 6-2-11-6-t01-pass, whose non-symbolic `/Differences` PDF/A-2 allows and X-4 does not, as the register says. Ghent and Altona raise nothing.
- Task 5.2, `x4.font.embedded`: Isartor 6-3-4 and veraPDF 6-2-11-4-1 t01 fail it as they should. Isartor 6-3-4-t01-fail-f uses its fonts only in an annotation appearance, a non-print element, and passes. Isartor 6-3-4-t01-fail-g shows unembedded Helvetica inside a Type 3 glyph procedure, which the walk does not enter (phase 4's recorded limit), so it passes for now; glyph presence needs `CharProcs` and takes it up. veraPDF 6-2-11-4-1 t02 (glyphs missing from an embedded program) is `x4.font.glyphs-present`'s.
- Tasks 5.2 and 5.3, glyphs, `.notdef` and widths (oracle and owner, 2026-11-26): every veraPDF 6-2-11-4-1, 6-2-11-5 and 6-2-11-8 file and Isartor 6-3-5 t01 and 6-3-6 fail the row they test; 6-2-11-4-1-t02-fail-c fails `.notdef` and widths, where veraPDF 1.30 also puts it; Isartor 6-3-5 t02 and t03 (PDF/A's CharSet and CIDSet) pass, X-4 having no such rule. Ghent and Altona raise nothing but Ghent's two GWG051 font-report pages, which show all 256 codes of a 58-glyph subset (not X-4 claims). Readings: `WinAnsiEncoding`'s unused codes above octal 40 are its bullet (ISO 32000-1 Annex D); a symbolic TrueType font with neither a (3,0) nor a (1,0) `cmap` leaves its glyphs to the reader (9.6.6.4), so they are not judged; an unembedded font is judged only for `.notdef` its encoding settles; a Type 3 font's widths are its glyph procedures' `d0`/`d1`. Widths agree within 1/1000 em (X4-Q16). Not covered: Type 3 glyph procedures are not walked, so a font used only inside one (Isartor 6-3-4-t01-fail-g) is not judged, and composite codes are split two bytes at a time, so a predefined CMap with one-byte codes (the RKSJ and EUC ones) is read only for its two-byte codes.
- Phase 6, the milestone's rows: 6.25's lexical row, moved to phase 4 in task 2.2, had not been done (phase 4's done list does not name it); it and the graphics row are done now, with the oracle's readings (decision log). On veraPDF 6-1-13 and BFO 6-1-13: integers, names, content strings, nesting and CIDs agree; t03-fail-d (a long string outside any content stream), BFO t05 and t09 (reals nearer 0 than 1.175e-38) and the page-size files pass, which PDF/X-4 allows. 6.6's recommendation judges dictionary keys and content-stream names on UTF-8 only, as their raw spelling is not kept (marked `ponytail:`).
- Phase 6, 6.1.1: with `x4.pdf-reference.conforms` implemented, each damage fixture at PDF/X-4 fails its tier's check and 6.1.1, as plan task 2.2 designed (classified: decided reading, not a regression); the scenarios for them say so. Ghent and Altona pass it.
- Process slip, 2026-11-26: `7aacf55` was committed with a red gate. The gate line piped `bin/rust/check.sh` into `grep`, which hid its exit status; a unit test counting the checks judged from the tiers, and 4 fixtures' expectations, still needed 6.1.1. The next commit fixes both; the gates now run through a script that stops at the first failing step.
- Phase 6, 6.3 (oracle, 2026-11-26): a missing resource is reported once, under 6.3, narrowed to what renders (`/OC` property names only, pattern names only under a Pattern space, device names never). The predefined CMaps are Table 118's 61 names, not every CMap `hayro-cmap` bundles. veraPDF 6-2-11-3-3-t01-fail-a fails it; t02 (WMode) and t03 (a `/UseCMap` naming an embedded CMap, which is in the file) are PDF/A-only rules and pass. Not judged: forms without `/Resources`, appearance streams and Type 3 glyph procedures (marked `ponytail:`). Missing `/Widths` is 6.1.1's, not 6.3's (6.5.3 lets the program supply widths).
- Task 6.1, the Rust `verify`: PDF/A (`--pdfa`) and peak ink (`--tac`) are opt-in, the default the brief assumes and the decision log's open question records; the switches that were `--x/--no-x` pairs keep their default and gain only the flag that changes it (`--no-colour`, `--no-fonts`, `--no-missing-glyphs`, `--no-pdfx`), with the config file to set the rest. The JSON is one record per file: the core's report (schema 1, so severity is `warning`, not the Python's `warn`) with `path`, `ok`, `exempt` and `unreadable`. A file that cannot be read exits 2, as a refused one does. The version is the crate's until the release wiring (6.3) passes the package's in.
- Task 6.1, the config file: a file without `schema` reads as schema 1, so today's files keep working; tables for the commands still in Python (`inspect`, `preview`, `texlog`, `extract`, `crop`) are left alone. `extends` merges `[verify]` key by key, ancestor first; a cycle is an error. `--show-profile` prints the merged `[verify]` table from the file, not the command line's own options.
- The Python 6.4.3.3 rule judges every reachable ICCBased space, not only painted ones (X4-Q3); marked `ponytail:` in
  `colour.py`, for the Rust colour evaluator to do properly.

- Phase 9, `inspect`: parity of the Python `inspect_pdf` (as at `b00d354`) against `_core.inspect` on every test-db
  sample set but `kura-bench` and `iust-pdfcorpus` (10,475 files): 9,463 identical, 242 different, 523 the core refuses,
  223 pikepdf cannot open, 24 on which the Python crashes. Of the 242: 44 differ only in the last digit of an unrounded
  size; 51 name fonts whose names are not UTF-8 (the Python drops them); 136 are the reading differences already
  explained (68 objects beyond `/Size`, 57 damaged objects, 9 damaged content, a duplicate key, a header offset); 3 have
  no MediaBox, where qpdf supplies Letter; and 8 others: 3 non-UTF-8 fonts whose text boxes move the grounds under them,
  1 output intent profile in ASCIIHex without `>`, 1 `/BaseFont` written as a string (both fonts then read as "?"), 2
  pages pikepdf stops reading at an inline image, and 1 where the Python counts 33 glyphs against the wrong font (the
  core agrees with a count from the content). The unit test's expectations were corrected first: they had been written
  without running.

- Phase 9, `preview`: `hayro` against Ghostscript (`png16m`, 72 dpi) on the fixtures, Ghent, Altona and GWG Processing
  Steps (493 files): 488 compared page by page, the worst page's mean absolute difference (0 to 255) under 1 in 101
  files, under 3 in 248, under 10 in 65, and 10 or more in 74. The large ones are Ghent and Altona patches built to test
  colour (transparency, soft masks, 16-bit, DeviceN, ICC, white overprint): `hayro` converts CMYK to RGB differently
  from Ghostscript's default profile, and its colours are softer; GWG192 differs by 32 whether Ghostscript simulates
  overprint or not, so overprint is not the cause. Ghostscript fails to draw `x4-crypt-filter` (hayro draws it) and
  cannot open three damaged fixtures; hayro cannot open `corrupt.pdf`; `x4-no-media-box` gets a different default page
  size. Over every test-db sample set but `kura-bench` and `iust-pdfcorpus` plus the fixtures (10,829 files, 9 dpi,
  guides on): 10,720 rendered, 101 refused (61 unreadable, 40 encrypted), 8 pages too small at 9 dpi or beyond 65,535
  pixels; no panic and no file over 120 seconds.

- Phase 9, `extract` slice A, against the Python as at `5032bd0`, on the fixtures and 16 test-db sets (3,901 files):
  3,406 identical; 311 that the Python extract already failed on and still does (pikepdf's `copy_foreign` on vector
  pieces, images it cannot transcode), which later slices replace; 66 where one side fails (the core refuses 52 damaged
  or password-protected files pikepdf opens or repairs; the Python crashed on 11 the core reads, fonts whose names are
  not UTF-8 among them); 118 different: 67 whose page tree reads short in the core (references beyond `/Size`, damaged
  objects), now named in `failures`; 36 `/Info` values that are not text, which pikepdf printed as `pikepdf.Array(...)`
  and the core leaves out; the rest are v5 profiles now described by their own `desc` tag, font programs whose Flate
  data fails its checksum (refused, as everywhere in the core), and the asset order of files whose `/Resources` sit on a
  `/Pages` node, which qpdf pushes down to the pages before the Python walked them. Vector PDFs are compared without
  their `/ID`, which pikepdf writes at random. The parity script first ran with relative paths from a temporary
  directory, so every file failed on both sides and matched; the numbers above are from the corrected run.

## Owner requests

- 2026-11-25: add a Windows x64 build to CI and the release wheels. Done: a `windows` CI job (Rust tests, a wheel that
  installs and runs) and a `x86_64-pc-windows-msvc` release wheel, checked for non-system DLLs with `pefile`. Unverified
  until pushed: no Windows machine here.

## Owner actions

- **JPEG 2000 standards:** done 2026-11-26 on the owner's word: ITU-T T.800 and T.801 (08/2002), test-db `580f8e51`;
  the owner then supplied T.800 (06/2019) and T.801 (06/2021), test-db `a2661129`.
- **Objects beyond `/Size`:** 65 corpus files (pdf.js, unsafe-docs, jhove error sets; no print output) reference
  objects numbered at or beyond the trailer's `/Size`; the core reads those references as null, as decided in task
  2.1, and qpdf reads the objects. Where the catalog is one of them, the file has no pages for the checks and is
  reported sound. Worth deciding whether a file whose `/Root` does not resolve should be refused.
- **Register note:** test-db's `docs/pdfx/x4-register.md` (untracked there, so left alone) gives
  `x4.spot.consistent-definition` the scope "all reachable"; by the decision of 2026-11-25 it is print elements only.
- **0.3:** write to admin@gwg.org asking to use the Ghent Output Suite and the Processing Steps suite in public CI.
- **Later, X5-Q9:** ISO 15930-1, -3, -4 and -6 are listed in test-db's `docs/standards-wanted.md`; not to buy now.

- Task 7.4: the three veraPDF files that reference an absent path are Reference XObjects, so they are X-5g material:
  phase 8 (8.3, targets) reads them. The one test-db file with a `/DestOutputProfileRef` has a PDF/A intent, not a
  PDF/X one, so X-4p's rows find nothing in it, correctly.
- Task 7.4, pdfToolbox 17.1 ("Verify compliance with PDFX-4p"): it does not resolve the fixtures' relative URL, so it
  judges each alone ("Referenced destination profile is not available" on every file). It agrees with every
  reference row (`/ProfileName`, `/URLs` with `/Mac`, a short `/CheckSum`, `/ProfileCS`, `/ICCVersion`,
  `/ColorantTable`, an embedded profile) except one: it accepts a bare string in `/URLs`, which A.2 rules out (URL
  file specifications are dictionaries with `/FS /URL`, PDF 1.6 Reference 3.10.4). Kept as A.2 reads.

- Task 8.5: the generated X-5 files are built by `tests/fixtures/build_pdfs.py` into `tests/fixtures/pdfs/` (and the two
  n-colorant profiles into `tests/fixtures/icc/`), like every other fixture, rather than copied to test-db's
  `samples/generated/x5/`: they are reproducible, ours, and test-db's `proofpress-fixtures/` snapshot carries them at
  its next refresh. The n-colorant profiles are written in Python, not with `lcms2`, which has no Python binding that
  writes xCLR profiles; LittleCMS (through Pillow) must open each.
- Task 8.2: `x5.self-contained-is-x4` and `x5.g-proxy-bbox-target-page` are skips (decision log); the plan expected them
  checked.

## Environment notes

- The Arlington PDF Model (phase 3.1) is Apache-2.0; master at `cdc485e` (2026-11-19) is the candidate pin.

- `cargo` is not on the default `PATH`: Homebrew's rustup keeps its proxies in `/opt/homebrew/opt/rustup/bin`.
- `qpdf` 12.4.2 installed by the owner on 2026-11-25.
- pdfToolbox Server 17.1 CLI installed and activated by the owner on 2026-11-25 (trial, expires 2026-12-09), at
  `/Applications/callas pdfToolbox Server 17/cli/pdfToolbox`. A PDF/X-4 oracle for local runs:
  `pdfToolbox --noprogress "<cli>/var/Profiles/PDFX compliance/Verify compliance with PDFX-4.kfpx" <file>`. Never in CI.

## Stage reports

### Phase 0 (decisions and groundwork), finished 2026-11-25

- **What changed.** The PDF 1.6 Reference and its errata are in test-db; its Table C.1 settles 6.25 (no array or dictionary
  limits). `x4-good`, a genuine PDF/X-4 base fixture, exists and passes pdfToolbox 17.1. Four fixtures had wrong PDF/X
  claims corrected. In the Python: `trapped` reads XMP; the 6.4.3.3 identical-profile rule replaces an inverted warning;
  `pdf-version` is gone; the brief's four bugs are fixed (password crash, K-only JPEG `/Decode`, missing XObject,
  total ink through `tiffsep`).
- **Toolchains.** Python: ruff, pyright and pytest green (1154). Rust: `bin/rust/check.sh` green, 100% coverage. Docker:
  1154 passed.
- **Parity and corpus.** No Rust port yet, so no parity numbers. Total ink on the corpus that has known answers: test-db's
  overprint cases 0-3 read their expected 100/200/100/200%, Altona still peaks at 363%.
- **Unexpected.** Both Altona X-4 files fail 6.4.3.3 (a `DefaultCMYK` equal to the intent profile), confirmed by
  pdfToolbox and the oracle; the owner approved recording it. `pdfx-x4-rgb-image` also has an Info/XMP Keywords mismatch.
  ISO 15930-8 has a 2011 corrigendum that is not held.
- **Departures.** See "Deviations from the plan". A third-party PDF/X-4 oracle is now in reach (pdfToolbox CLI, trial to
  2026-12-09).

### Phase 1 (infrastructure), finished 2026-11-25

- **What changed.** `rust/checks.toml` lists 232 checks as data (every register row, the X-5 reader rules as skips,
  the house checks, `internal`), validated at build time against clause lists taken from the standards (numbers only)
  and generated into constants and `rust/CHECKS.md`. A completeness gate counts them and holds `implemented` to a
  passing and a failing fixture. Findings, per-check outcomes and the JSON report (schema 1) live in a new base module,
  `findings`. Gherkin steps verify a fixture at a PDF/X level; `x4-good` runs end to end. `bin/suites/pdfx.sh` runs the
  engine over test-db's PDF/X sets against `expectations/pdfx.toml`. The Python package builds with maturin and carries
  the Rust core as `proof_pdf._internal._core`; releases build per-platform wheels and check them.
- **Toolchains.** `bin/rust/check.sh` green at 100% coverage; Python green (1156); Docker green (1156, extension built in
  the image); `bin/suites/pdfx.sh` 39 files, 39 accepted, no problems.
- **Parity and corpus.** Nothing is judged yet, so no parity numbers; the corpus run proves the harness only.
- **Unexpected.** pdfToolbox settled the Ghent positives at exactly the plan's 35 of 39. maturin runs cargo from the
  repository root, so the pinned toolchain had to become rustup's default in CI and Docker.
- **Departures.** The CI wheel matrix is unverified until pushed; the parity gate in CI waits for the first port; the
  extension lives in `_internal`. See "Deviations from the plan".


### Phase 2 (reading the file), finished 2026-11-25

- **What changed.** Tiers 1 to 5 read a file in Rust: the lexer and object reader (raw spellings kept), the filters
  with byte budgets, the cross-reference chain with object and cross-reference streams, damage, gaps and refusals as
  findings, and the walk at open. 8 PDF/X-4 checks (6.6, 6.8, 6.15, the 6.25 object count) and 16 structure checks are
  implemented, each with a fixture that fails it and one that passes it.
- **Toolchains.** `bin/rust/check.sh` green at 100% coverage; Python green (1156); Docker green (1156);
  `bin/suites/pdfx.sh` 39 files, 39 accepted, no problems.
- **Corpus, oracle and fuzzing.** 44,608 corpus files with no crash or timeout; the qpdf disagreements are explained
  above, under "Corpus and oracle", and no print output is refused. The stable mutation test ran 5,000,000 cases (20 minutes) clean.
- **"Same rule" test material.** On the veraPDF, Isartor and BFO files that `x4-test-material.md` §2 marks as the same
  rule for 6.6, 6.8 and 6.15, every verdict agrees with the X-4 reading: LZW in a stream fails, a non-standard filter
  fails, every encrypted file is refused, a font name that is not UTF-8 fails while another name passes, and a DocMDP
  signature fails X-4 though PDF/A-2 passes it. The misses are all content-stream matters that wait for tier 6: LZW in
  an inline image (3 files) and the lexical limits of 6.25 (the 6-1-13 files).
- **Unexpected.** qpdf found 300 false refusals in the first reader, all fixed: free entries with generation 65536,
  shared `/XRefStm` sections, and `/Length` stored in an object stream.
- **Departures.** 6.25's lexical row moves to phase 4; `x4.pdf-reference.conforms` stays `todo`; no `cargo fuzz`
  target (stable mutation test instead); `bomb_giant.pdf` waits for an image decoder. See "Deviations from the plan".

### Phase 3 (document structure and metadata), finished 2026-11-25

- **What changed.** Tier 5 gained the page tree with inheritance and stream data on request; `common` gained the XMP
  reader (prefixes kept), PDF and XMP dates, text strings, ICC headers and rectangles. 75 PDF/X-4 rows were implemented
  in this phase, each with a fixture that fails it: metadata and identification (6.9-6.11), the output intent and the
  ICC registry (6.4.1, 6.4.2.1), boxes (6.12), keys forbidden anywhere (6.7, 6.13, 6.14, 6.16, 6.18, 6.21, 6.22, 6.26),
  optional content (6.24), annotations and trap networks (6.17, 6.9). PDF/X-4 stands at 83 implemented, 54 todo,
  4 skip.
- **Toolchains.** `bin/rust/check.sh` green at 100% coverage; Python green (1156); Docker green (1156);
  `bin/suites/pdfx.sh` 39 files, 39 accepted, no problems, with every warning on the Ghent and Altona positives
  classified and recorded in test-db.
- **Test material.** The veraPDF and BFO "same rule" files agree with the X-4 reading or differ for a recorded reason;
  GWG Processing Steps gives the box and optional content results test-db predicts; GWG151 and GWG152 fail 6.24.
- **Unexpected.** The fuzz timeouts came from the reader decoding 8.4 million cross-reference entries before checking
  the object limit; it now refuses first. Two oracle questions: an empty `/RegistryName` reads as absent, and the
  registry rule went to the owner, who chose a warning. 38 of the 39 positives carry non-uuid document IDs, which 6.10.4
  recommends against.
- **Departures.** Tier 5's Python mapping and the Arlington typed views wait for a caller; the ICC MD5 and description
  wait for the phase 4 colour rows; image alternates' sameness is a `skip`; the registry rule is a warning by decision.
  See "Deviations from the plan" and the decision log.

### Phase 4 (content, page model and colour), finished 2026-11-26

- **What changed.** Tiers 6 and 7 in Rust: content streams in both modes, inline images, forms, the page model with
  paint events, overprint, clips, groups and masks. One colour-space evaluator judges 6.4.3 and 6.20; JPEG 2000 headers
  (6.27), JPEG markers and JBIG2 segments (6.8) are read; pre-separated pages (6.1.2). Every print check that reads the
  PDF was ported with the parity gate and its Python deleted; colour to L*a*b* goes through `lcms2`, and CMYK images
  decode through `zune-jpeg` and `hayro-jpeg2000`. PDF/X-4 stands at 112 implemented, 25 todo, 4 skip.
- **Done criteria.** The rows of 6.4.3, 6.19, 6.20, 6.23, 6.27 and 6.1.2 are implemented but for one `skip`
  (`x4.spot.mixing-hints-informed`, a judgement of care); each port passed the parity gate on the 10,013-file corpus
  with every difference explained (see Deviations); the Python checks are deleted. `paint.py` still serves the
  inventory and extract reports, which belong to the CLI stage.
- **Test material.** Ghent's 39 X-4 claims and Altona raise no colour finding; the veraPDF "same rule" colour files
  agree; §3 items 4, 5, 16 and 19 are built fixtures.
- **Unexpected.** The colorants rule took three corrections (absolute colorimetric, interpolation, and, by the
  oracle, no judgement with `None` colorants); sampled functions now interpolate (ISO 32000 7.10.2) where the Python took the nearest sample; the core is
  slow on a few corpus files at every level (38 to 94 seconds), for the 2026-12-02 checkpoint.
- **Departures.** Inline JPEG images are not read for 6.8 (PDF allows no inline JPEG 2000 or JBIG2); JPEG entropy
  data is not decoded. See "Deviations from
  the plan".

### Phase 5 (fonts), finished 2026-11-26

- **What changed.** Every 6.5 row is implemented: embedding (6.5.1, render mode 3 exempt), glyph presence and
  `.notdef` (6.5.1, 6.5.2), widths (6.5.3) and TrueType encodings (6.5.4), on the fonts text runs use, hidden optional
  content included. Font programs are read with `read-fonts`; CMaps with `hayro-cmap`, which bundles Adobe's
  predefined CMaps. PDF/X-4 stands at 120 implemented, 17 todo, 4 skip.
- **Decisions.** Two oracle escalations went to the owner: a glyph is skipped only when every appearance of it is in
  render mode 3, and predefined CMaps are bundled and judged. The oracle settled the line between 6.5.1 and 6.5.2 and
  6.5.4's scope (print elements only, X4-Q3).
- **Test material.** veraPDF's and Isartor's font folders agree row for row, with the PDF/A-only rules (CharSet,
  CIDSet, `/Differences` that PDF/A-2 allows) as the register predicts; Ghent's OpenType patch and every Ghent and
  Altona positive pass; §3 item 6 built (an unembedded font in render mode 3 passes, one in hidden optional content
  fails).
- **Unexpected.** Font programs needed two crates, not one: hayro's font crate reads PDFs through its own parser, so
  `read-fonts` reads the programs and `hayro-cmap` the CMaps. Test fonts are generated in the fixture builder
  (TrueType, CFF, CID-keyed CFF and a hand-made Type 1), so none carries a third-party licence.
- **Departures.** Type 3 glyph procedures are still not walked; composite codes are split two bytes at a time; a
  font dictionary written directly in a resource dictionary is not judged. See "Deviations from the plan".

### Phase 6 (the CLI and the Python API, the PDF/X-4 milestone), finished 2026-11-26

- **What changed.** `proof-pdf verify` is the Rust core's command (clap): the Python command's options and config keys,
  PDF/X levels, veraPDF and Ghostscript through `process-wrap`, `proof-pdf.toml` with `schema = 1` and `extends`, the
  brief's exit codes and JSON. The Python `verify` and `core.verify_file` are thin layers over `_core.run`. Every X-4 row
  that needs no external file is implemented: 125 implemented, 12 todo (X-4p), 4 skip.
- **Decisions.** PDF/A and total ink run only when asked. A level's unwritten rows are not evaluated, so no level passes
  on rows nobody checks. See the decision log.
- **Test material.** `test_cli.py`, `test_core.py` and `test_integration.py` run the real command on the built fixtures;
  the samples and the Altona and Ghent suites keep their expectations, now with the X-4 findings (classified (b) or (c)
  in `expected.toml`). The PDF/X suite: 39 files, 0 problems.
- **Unexpected.** Retiring the Python `boxes` check left a page without a MediaBox passing at every level; it is now
  `page.media-box`. Selecting X-5g passed an X-4 file on the inherited rows alone; fixed as above.
- **Departures.** None from the plan's tasks; X-4p's rows wait for phase 7 as planned.

### Phase 7 (file sets and PDF/X-4p), finished 2026-11-26

- **What changed.** Every X-4p row is implemented, in both modes: a file set (the default on the command line) and a
  file alone (`--file-alone`, and the Python `_core.verify`). The external profile is found at the local `/URLs` and
  confirmed by `/CheckSum`; nothing is fetched. A check can now answer "not evaluated" (`JUDGES`). PDF/X-4 and X-4p
  stand at 137 implemented, 0 todo, 4 skip.
- **Decisions.** The oracle settled the readings without escalation: mismatch or missing fails only when no network
  URL remains; `/ProfileName` matches any description record; 6.4.3.3 applies against the external profile, and
  without it the `/CheckSum` or a non-CMYK `/ProfileCS` can still decide; 6.4.3.4 needs no profile data. See the
  decision log.
- **Test material.** 19 built fixtures, the Little-CMS profile beside them by a relative URL. pdfToolbox agrees with
  every reference row but the bare-string URL (see "Deviations from the plan").
- **Unexpected.** pdfToolbox does not resolve relative profile URLs, so it cannot judge a file set here.
- **Departures.** The three absent-path veraPDF files are Reference XObjects and go to phase 8.

### Phase 8 (PDF/X-5), finished 2026-11-26

- **What changed.** X-5g, X-5pg and X-5n are checked: the inheritance map as `applies`, the identification, reference
  XObject proxies and their XMP, file sets whose targets are read beside the container (or embedded) and verified at
  the level they claim, recursively, and the n-colorant output intent with its device-space rules. PDF/X-5 stands at 32
  implemented, 0 todo, 21 skip; PDF/X-4 and X-4p at 137, 0, 4.
- **Decisions.** The plan's X5-Q defaults throughout; the owner's corrigendum (ISO 15930-8 Cor 1:2011, added to test-db)
  confirmed X5-Q6. New readings are in the decision log: target resolution and depth 16, `--target-versions`, the
  X-5n profile version 4.2, the 7.3 bans' reach.
- **Test material.** 60 built fixtures (`x5g-*`, `x5pg-*`, `x5n-*`, `x5-target-*`) and two n-colorant profiles of our
  own. test-db holds no PDF/X-5 file, as the register found.
- **Unexpected.** No row is `todo` any more, so the "not written yet" safeguard is tested on a made-up catalogue. The
  library is compiled twice for coverage (unit tests and integration tests); new code needs unit tests for every path.
- **Departures.** See "Deviations from the plan": generated files stay in proofpress's fixtures; two rows are skips.
