# PDF/X plan: progress

The running log of the unattended run that carries out [`pdfx-plan.md`](pdfx-plan.md), started 2026-11-25 with the prompt in
[`pdfx-run.md`](pdfx-run.md). A fresh session resumes from **Status**.

## Stop questions

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

- **Phase 4** (content, page model and colour): 4.1 to 4.5 done; 4.6 under way. The walk now places what it paints
  (CTM, clip, path, text and image extents) and reads colours as the print checks do; 23 print checks run in Rust:
  what is painted where (`outside-media`, `xobject-missing`, `artwork-bleed`, `outside-bleed`), the inks
  (`overprint`, `white-overprint`, `rich-black`, `k-only-text`) and the page geometry (`geometry`, `bleed`, `centring`,
  `page-count`, `page-size`, `page-rotate`, `output-condition`) and the colour mode (`blend-space`, `spot-ink`,
  `colour-space`, `rgb-only`, `icc-profile`) and image resolution (`image-dpi`, `image-dpi-max`, `image-stretch`).
  Profile parameters reach the core as JSON (`Settings`, `Params`). Next in 4.6: text and fonts (`text-size`,
  `hairline`, `safe-area`, `missing-glyph`, `font-embedded`, `font-type3`), then function evaluation (for
  `spot-definition`), image decoding (for `gray-only`), and contrast. The brief's pace checkpoint for
  `paint.py` falls on 2026-12-02, one week into phase 4.

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
| 4.6 (part) | the commit after `396aa0d` | Image resolution: placement and sample size in the walk; `image-dpi`, `image-dpi-max`, `image-stretch` ported, parity-proved, `resolution.py` deleted |

## Register coverage

From `cargo test --test completeness -- --nocapture` (implemented / todo / skip):

| Group | Implemented | Todo | Skip |
|---|---|---|---|
| PDF/X-4 and X-4p | 109 | 28 | 4 |
| PDF/X-5 | 0 | 35 | 18 |
| House and structure checks | 42 | 14 | 1 (`internal`) |

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-25 (100% lines, regions, functions) |
| Python (ruff, pyright, pytest) | green, 2026-11-25: 1156 passed |
| `bin/docker/test.sh` | green at the end of phase 3, 2026-11-25: 1156 passed |
| `bin/suites/pdfx.sh` | 2026-11-25: 39 files, 39 accepted, 0 problems (nothing judged yet) |

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

Every Ghent, Altona and GWG Processing Steps file and every fixture opens sound, so the re-plan point (the parser
refusing real print output) has not triggered.

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-25:
- PDF/X 6.25 is the PDF 1.6 Reference's Table C.1 only: no array or dictionary limit, no page size (task 0.2).
- Ghent's X-4 positives are the 35 of 39 claims pdfToolbox passes; GWG151 and GWG152 fail 6.24 (oracle agreed, no
  escalation).
- Altona fails 6.4.3.3 only (two hits per file) and stays a positive otherwise; owner approved; plan edited.
- X4-Q6 stands: no corrigendum to ISO 15930-7:2010 found; 15930-8 Cor 1:2011 exists and is not held (task 0.2).
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
- Task 4.6: `boxes` and `bleedbox` come from the Python PDF/X layer (`pdfx/x4.py`), which the plan does not port; the
  `x4.boxes.*` rows replace them.
- Task 4.6: the profile's on/off switches for print checks stay in Python (`ported.check`); the core runs every
  implemented print check and Python keeps those the profile turns on.
- Task 4.6: the walk now starts a form from the graphics state of its `Do`, colour and overprint included (8.10.1),
  where it used to start from the default; an unbalanced `Q` keeps the state and restores only the CTM, as the Python
  does; an `XObject` entry that is not a stream is missing (7.8.3), not an image. Ghent and Altona are unchanged.
- A check the core cannot judge because it refused the file or read it with gaps now fails in `verify` ("could not be
  checked"), where the Python checked whatever pikepdf repaired: the behaviour change the brief lists.
- The Python 6.4.3.3 rule judges every reachable ICCBased space, not only painted ones (X4-Q3); marked `ponytail:` in
  `colour.py`, for the Rust colour evaluator to do properly.

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
- **Before phase 8:** get ISO 15930-8:2010/Cor 1:2011 (one page; ISO corrigenda are usually free, but iso.org refuses
  scripts). Not blocking until then.
- **Later, X5-Q9:** ISO 15930-1, -3, -4 and -6 are listed in test-db's `docs/standards-wanted.md`; not to buy now.

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

