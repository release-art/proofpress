# Plan: PDF/X verification in Rust, to feature-complete

You are working in `/Users/ilo/Devel/Release.art/proofpress`, on the Rust core in `rust/`. This plan takes it from today's
skeleton to a feature-complete PDF/X verifier: **every file requirement of PDF/X-4, PDF/X-4p (ISO 15930-7:2010), PDF/X-5g,
PDF/X-5pg and PDF/X-5n (ISO 15930-8:2010)**, plus the print checks proof-pdf already has, behind the same CLI and Python
API. Written 2026-11-25.

Read first, in this order:
- [`rust-port.md`](rust-port.md): the decisions, house rules, stages and backlog. This plan details its stages and adds the
  PDF/X work; where they disagree on a decision, the brief and the decision log in [`../README.md`](../README.md) win.
- [`../notes/rust-architecture.md`](../notes/rust-architecture.md): the tiers, the pipeline, the object model, damage,
  findings, and the code conventions. Every task below lands in the tier it names there.
- `CONTRIBUTING.md`: the toolchains and where each kind of test goes. `bin/rust/check.sh` must pass at the end of every task.

The requirements themselves are in three private registers in `proof-pdf-test-db/docs/pdfx/`, because they paraphrase
licensed standards: `x4-register.md` (one row per requirement, with a proposed check id), `x5-register.md`, and
`x4-test-material.md`. This plan cites them by clause and section only. **Never copy a standard's text, or a register's
paraphrase of it, into this repository**; clause and page numbers are fine.

## What feature-complete means

| Scope | Count | Source |
|---|---|---|
| PDF/X-4 and X-4p file requirements ("shall") | 128 (114 both, 2 X-4 only, 12 X-4p only) | `x4-register.md` §1 |
| PDF/X-4 recommendations ("should") | 11, as opt-in warnings | `x4-register.md` §1 |
| PDF/X-5 new or modified requirements | 35 (19 for X-5g and X-5pg, 17 for X-5n) | `x5-register.md` §2 |
| Rules on readers, writers or processes | 2 in X-4, 18 in X-5 | Recorded as `skip` with the reason, not checked |

Done means, all together:

1. **Every register row is a check or a recorded skip.** Each check has its id in `rust/checks.toml`, the clauses it may
   cite, at least one fixture that passes and one that fails it, and a Gherkin scenario.
2. **Every file that should pass, passes:**
   - the Ghent Output Suite files that conform (35 of the 39 that claim X-4, after the disputes in task 0.1 are settled);
   - both Altona files, except for `x4.icc.cmyk-not-intent-profile` (6.4.3.3), which each fails twice by design of the
     file (decision of 2026-11-25);
   - the four Ghent test pages, which carry a signed Acrobat Preflight "PDF/X-4 compliant" record, the only third-party
     PDF/X verdict in reach;
   - our own genuine X-4 and X-5 fixtures.
3. **Every built negative fails with exactly its intended check,** and nothing else.
4. **Today's print checks are ported,** with findings identical to the Python on the corpus (the brief's parity gate).
5. **The CLI and the Python API are complete:** `verify` with PDF/X level selection, and the other commands. pikepdf and
   Pillow are gone from runtime, and a release is cut.

## Working rules

These add to the brief's house rules.

- **One path for every requirement:** register row → `checks.toml` entry → code in the tier that sees the evidence → a pass
  fixture and a fail fixture → a scenario → the generated README row. The register's proposed id (`x4.output-intent.present`,
  `x5.g-proxy.ref-id`) becomes the check id, and is never reused.
- **Three kinds of test material:**

  | Kind | Where | Licence | Runs in |
  |---|---|---|---|
  | Our fixtures, built deterministically | `tests/fixtures/pdfs/` (and `tests/fixtures/pdfx/` for PDF/X) | Apache-2.0 | CI, every push |
  | Public sets fetched at test time, pinned by sha256 | veraPDF corpus without its `Isartor test files/`, BFO, pdf-differences, Altona, Little-CMS profiles | CC BY, MIT, ECI | An opt-in CI job |
  | Private sets | `proof-pdf-test-db`: Ghent, GWG Processing Steps, Isartor, the large corpora, client jobs | Not redistributable | Local corpus runs only |

- **Expected results are data:**
  - `tests/fixtures/pdfx/expected.toml` maps each fixture to the set of check ids it must raise, each with a one-line reason.
  - `proof-pdf-test-db/expectations/pdfx.toml` does the same for the private sets.
  - A test asserts that the actual set equals the expected set, as `tests/fixtures/suites/expected.toml` already does.
- **One defect per negative fixture.** Build each negative from the genuine `x4-good` base (task 0.4) by one targeted
  change, so a failure points at exactly one rule. Fixtures that need several defects say so in their reason.
- **Proxy fixtures from PDF/A corpora count only where the rule is the same.** `x4-test-material.md` §2 marks each mapping
  "same rule" or "same property, different rule". Only the first kind may enter `expected.toml`; the second informs tests
  but never sets an expectation.
- **A disputed file is classified, never silenced:**
  - (a) our bug: fix it, with a regression fixture;
  - (b) the file really doesn't conform: record the expectation;
  - (c) the standard is ambiguous: decide, then record the decision in the decision log and in the check's `checks.toml`
    entry.
- **Fix the cause in the tier that owns it.** A check that has to work around a lower tier's output means the lower tier is
  wrong.

## Phase 0: decisions and groundwork

Nothing here is Rust. It removes the questions that would otherwise stop work midway.

**0.1 Settle the open interpretation questions.** **Decided 2026-11-25: every proposed default below is accepted**, recorded in
the decision log. A later change takes a new decision-log line and an edit here. The registers
number them: X-4 Q1–Q21 (`x4-register.md` §4) and X-5 Q1–Q13 (`x5-register.md` §5). Proposed defaults:

| # | Topic (clause) | Proposed default |
|---|---|---|
| X4-Q1 | Where `Trapped` must be (6.9, 6.10.2, Annex B) | XMP `pdf:Trapped` is the requirement; Info `/Trapped` is judged only when present, for equality |
| X4-Q2 | Detecting pre-separated pages (6.1.2) | Fail only when every page paints one ink and the inks differ across pages |
| X4-Q3 | Print versus non-print scope (6.1.3, 6.2) | Colour and font rows follow print elements only; prohibitions on keys (OPI, PostScript, `TR`, actions) cover the whole file |
| X4-Q4 | Spot alternates follow the colour rules (6.4.3.4) | Yes, as the clause says; `spaces.py`'s opposite stance is not ported |
| X4-Q5 | `/Process` colour space (6.4.3.4) | A device space of the right family, or an `ICCBased` space of that family |
| X4-Q6 | `dc:creator` items (6.10.2) | Exactly one item, as the rule says; check for errata first (task 0.2) |
| X4-Q7 | "Appropriate" dates and title (6.10.4) | Non-empty and syntactically valid; a placeholder title is not a finding |
| X4-Q8 | Which metadata streams may not be filtered (6.10.1) | Every `/Metadata` stream |
| X4-Q9 | Halftone transfer functions (6.13) | Allowed only for non-primary colorants inside a Type 5 halftone |
| X4-Q10 | Matching the version value (6.11) | Exact string; a `DestOutputProfileRef` in a file labelled X-4 is reported as X-4 failures |
| X4-Q11 | Actions (6.18) | Every action fails, `GoTo` and `URI` included; `/Dest` passes |
| X4-Q12 | File specifications under X-4p (6.7, A.2) | X-4p's `URLs` specifications are exempt from the `/EF` rule |
| X4-Q13 | Optional content `/Order` (6.24) | Every OCG reachable from the catalog's `/OCGs` |
| X4-Q14 | Which Table C.1 (6.25) | The PDF 1.6 Reference's table, as cited; get it first (task 0.2) |
| X4-Q15 | JPEG 2000 colour (6.27) | One colour specification box; the `rreq` box plus the codestream's SIZ marker prove baseline |
| X4-Q16 | Width consistency (6.5.3) | Glyphs actually shown; tolerance is a documented constant from the maths (rounding of 1/1000 em units), not a printer number |
| X4-Q17 | `/ICCVersion` (A.2) | Compare the raw bytes |
| X4-Q18 | Box comparisons (6.12, 6.17) | Normalise inverted rectangles; one documented sub-point tolerance shared with the geometry checks |
| X4-Q19 | Blend-mode arrays (6.20) | Every element must be a standard name |
| X4-Q20 | Which DeviceN names are spots (6.4.3.4) | Everything except Cyan, Magenta, Yellow, Black and `None`; `All` counts as a spot |
| X4-Q21 | The missing-BleedBox warning (6.12) | Not an X-4 rule; keep it as a house check outside the `x4.` ids |
| X5-Q1, Q2 | Must X-5g contain a proxy (5, Table A.1, 8.2) | Check the shape of every form with `/Ref`; an X-5g file with none is a warning |
| X5-Q3 | `/EF` on a reference's file spec (8.2 against X-4 6.7) | Not required for the `Ref` target; an embedded target is accepted |
| X5-Q4 | Detecting `3CLR`/`4CLR` misuse (7.2) | By the `ColorantTable` names; a length differing from the channel count is a warning |
| X5-Q5 | A target of a different version (8.4) | A parameter with no default (house rule) |
| X5-Q6 | Property spelling (8.3, 8.4) | The XMP schema's spelling; another case is a distinct finding |
| X5-Q7 | Same printing condition (8.5) | Equal `OutputConditionIdentifier` and `RegistryName`; differing profiles are a warning |
| X5-Q8 | The OCG ban on targets (8.7) | Applies to X-4, X-4p, X-5g and X-5pg targets; either `OCProperties` or any OCG fails |
| X5-Q9 | Targets at X-1a or X-3 (8.2) | "Not evaluated", so the set fails, until those standards are held; list them as wanted |
| X5-Q10 | X-5n bans reaching alternate spaces (7.3) | Yes, as X-4 does |
| X5-Q11 | "Identical to the output profile" when it is external (X-4 6.4.3.3) | Compare against the external file when the set includes it; otherwise "not evaluated" |
| X5-Q12 | What "uses transparency" covers (7.3) | Any print element on the page |
| X5-Q13 | Proxies inside targets (8.2) | Recurse, with a depth limit in `Limits` |

**0.2 Get the missing references.**
- **The PDF 1.6 Reference.** Clause 6.25 cites its Table C.1, which differs from ISO 32000-1's. Adobe published it free;
  register it in test-db's `standards/` with its terms.
- **Corrigenda or amendments to ISO 15930-7 and -8.** Check ISO's catalogue and the PDF Association's errata pages; X4-Q6
  depends on it.
- **The standards behind X5-Q9** (ISO 15930-1, -3, -4 and -6), added to test-db's `docs/standards-wanted.md`, not bought now.

**0.3 Ask GWG for permission (owner).** Ask in writing (admin@gwg.org) to use the Ghent Output Suite and the Processing
Steps suite in public CI. Until the answer, both stay local-only. Also update `tests/fixtures/suites/README.md`: gwg.org has
been reachable since 2026-11-08.

**0.4 Build a genuine PDF/X-4 base fixture.** Every built fixture claiming X-4 today fails it (`x4-test-material.md` §1): the
pikepdf ones carry a monitor profile and incomplete XMP, and `pdfx-x4-rgb-image.pdf`'s source calls it valid when it is not.
- Add `x4-good` to `tests/fixtures/build_pdfs.py`, with:
  - a CMYK printer output intent, using a Little-CMS test profile (MIT; confirm its terms and record them);
  - full XMP: dates, title, document IDs, `pdf:Trapped`, `pdfxid:GTS_PDFXVersion`;
  - Trim and Bleed boxes, a trailer `/ID`, and an embedded font.
- Correct the claims of the existing fixtures: their reasons in `expected.toml`, and the comment in the `.tex` source.
- Owner, if possible: preflight `x4-good` once in Acrobat Preflight or pdfToolbox, and record the result. That makes the base
  an oracle-backed positive, not only ours.

**0.5 Fix the Python before it becomes the parity baseline.** These come on top of the brief's four stage-0 bugs:
- **`check_trapped`** reads Info; 6.9 requires XMP.
- **`colour.py`** warns when a CMYK profile *differs* from the intent's; 6.4.3.3 forbids one that is the *same* (compared by
  MD5).
- **The `pdf-version` rule (TODO)** contradicts clause 5, which forbids judging by the header version; delete it.

The new `x4.*` checks are written from the register, not ported, so the parity gate does not cover `pdfx/x4.py`. Fixing the
first two still matters, because Python releases ship until phase 6.

*Done when* the decisions are logged, the references are in test-db, `x4-good` exists and the corrected fixture claims are
in, and the Python fixes have tests. Judgement: 1 week.

## Phase 1: infrastructure (the brief's stage 1)

**1.1 The check list as data.** `rust/checks.toml` and a `build.rs` that generates the check enum, the README table and
`--list-checks`, as the architecture note's "Checks, profiles and reports" section sets out.
- Fields per entry: `id`, `aliases`, `title` (in our words), `clauses`, `severity`, `applies` (any of `x4`, `x4p`, `x5g`,
  `x5pg`, `x5n`, or `print` for house checks), `params`, and `status`, which is `implemented`, `todo`, or `skip` with a
  `reason`.
- Seed it with **every register row** as `todo`, and the reader and process rules as `skip`.
- Keep the generated code free of branches: data tables, not `match` arms (every arm would need its own test under the
  coverage gate).

**1.2 Clause lists.** Extract the clause numbers (numbers only) of each standard held from test-db's Markdown copies into
`rust/clauses/*.txt`. A test validates every declared clause against them.

**1.3 The completeness gate.** A test that counts `checks.toml` by status and prints the register coverage. It fails when an
entry is `implemented` without a pass and a fail fixture in `expected.toml`, or `skip` without a reason. At the end of each
phase, every row the phase owns must be `implemented` or `skip`.

**1.4 Findings and reports.** Findings, per-check status (`pass`, `fail`, `not_evaluated`), the `structure` field, and the
JSON schema version 1, all from the architecture note. Add `insta` and snapshot the JSON.

**1.5 The fixture and scenario harness.**
- Reusable Gherkin steps in `rust/tests/bdd.rs`: "Given the fixture `x4-no-trapped`", "When it is verified as PDF/X-4",
  "Then check `x4.trapped.xmp` fails on page 1", "And no other check fails".
- One feature file per clause: `rust/tests/features/pdfx/x4-6.09-trapping.feature`.

**1.6 The corpus harness.** `bin/suites/pdfx.sh` runs the engine over test-db's sets and compares against
`proof-pdf-test-db/expectations/pdfx.toml`. It writes a results file stamped with the engine version, ranking outcomes
crash, timeout, refused, read with gaps, accepted (from the brief's backlog). It never runs in public CI.

**1.7 The brief's stage 1:** maturin, wheels for macOS and Linux, the parity gate in CI, `catch_unwind` around checks, the
licence and shared-library checks.

*Done when* the brief's stage 1 is done, `checks.toml` lists all 163 file rows (128 + 35) as `todo` and the reader and
process rules as `skip`, and a scenario runs end to end on `x4-good` (judging nothing yet). Judgement: 1–2 weeks.

## Phase 2: reading the file (the brief's stage 2; tiers 1–4)

**2.1 The parser,** as the architecture note specifies and the brief's to-do list details:
- tier 2's two modes and `raw(n)`;
- filters with budgets;
- the xref chain;
- damage and gaps, refusal reasons, the `Limits` struct, dangling-reference reasons;
- the byte-level findings.

**2.2 The PDF/X rows tiers 1–4 can see:**
- **6.6** name encoding: the raw spelling is already kept, so check `#xx` escapes and UTF-8.
- **6.8** filters: the allowed set, LZW, `Crypt`.
- **6.15** encryption and permissions: only `UR` and `UR3`.
- **6.25** architectural limits, as PDF/X-4 checks, not safety limits. The PDF 1.6 Reference's Table C.1 (in test-db since
  task 0.2) limits integers, reals, strings inside content streams, names and indirect objects, which tiers 2 and 4 see; it has
  no array or dictionary limit. Its `q` nesting, DeviceN and CID rows wait for tiers 5 to 7.
- **6.1.1** "valid PDF": each tier's own validators report here, as `x4.pdf-reference.conforms` findings with `caused_by`
  pointing at the underlying finding.

**2.3 Test material:**
- built negatives from `x4-test-material.md` §3, items 7 (names), 9 (`Crypt`) and 18 (limits);
- veraPDF corpus folders for filters, `F` keys, encryption and limits, marked "same rule" in §2 (PDF/A-1b's limits only after
  comparing them with the PDF 1.6 values);
- the brief's oracle and fuzz runs over every corpus in test-db.

*Done when* the brief's stage 2 is done, and the rows for 6.6, 6.8, 6.15 and 6.25 (numbers, content-stream strings, names, indirect objects) are
`implemented`. Judgement: 2–3 weeks.

## Phase 3: document structure and metadata (tier 5 and helpers)

**3.1 Tier 5,** per the architecture note:
- the object table filled at open, then frozen;
- page-tree inheritance;
- the Python mapping;
- typed views generated from Arlington, once its licence is checked and a commit pinned;
- Arlington shape checks as a tier-5 observer.

**3.2 The XMP helper** (new, in `common`). It parses RDF/XML keeping namespace and prefix identity, handles Text,
language-alternative and Seq values and structs (X-5 needs those), scans the packet header, and parses PDF and XMP dates,
normalised to UTC. It unlocks about 27 rows: **6.9, 6.10, 6.11** (trapping, metadata, document IDs and identification).
`xmp.py` is a reference only, since it loses prefixes.

**3.3 The ICC helper** (new, in `common`). It reads the header (class, data colour space, version), the Profile ID, the MD5
with ICC.1 7.2.18's zeroed fields (ICC.1:2022 is in test-db), the `desc`/`mluc` description and `clrt`. It unlocks
**6.4.1** and **6.4.2.x** (output intents and characterisations), and the version and identity rows of **6.4.3.3**. `lcms2`
stays for colour conversion; this helper only reads fields.

**3.4 Print and non-print elements** (6.1.3, 6.2). Decide, per decision X4-Q3, which objects are print elements. Phase 4's
colour and font rows depend on it.

**3.5 The tier-5 PDF/X rows:**
- 6.12 bounding boxes;
- 6.13 extended graphics state keys;
- 6.14 PostScript XObjects;
- 6.16 image dictionaries (alternates, OPI);
- 6.17 annotations, including TrapNet and PrinterMark rectangles against the Trim and Bleed boxes;
- 6.18 actions and JavaScript;
- 6.21 viewer preferences;
- 6.22 alternate presentations;
- 6.24 optional-content configuration;
- 6.26 XFA;
- 6.7 file specifications and embedded files (X-4's part);
- 6.9's TrapNet and FontFauxing rows.

**3.6 Test material:**
- the 35 conforming Ghent X-4 files, both Altona files, and GWG Processing Steps (optional-content positives, and 6.12
  negatives that come ready-made);
- veraPDF "same rule" folders for annotations, actions, optional content, alternate presentations, XFA, PostScript XObjects,
  the graphics-state keys and the XMP header;
- built negatives from §3, items 2, 8 and 10 to 17.

*Done when* every tier-5, XMP and ICC-header row is `implemented`, and the Ghent and Altona positives raise no `x4.*`
finding from them, except Altona's two `x4.icc.cmyk-not-intent-profile` hits each. Judgement: 3–4 weeks.

## Phase 4: content, page model and colour (the brief's stage 3; tiers 6 and 7)

**4.1 Tier 6**, per the architecture note: the two modes, inline images measured without guessing at `EI`, forms with one
ancestor set, page resources as the fallback for forms without `/Resources`, several content streams per page. It covers
**6.19** (`BX`/`EX`) and the operator part of **6.23** (rendering intents).

**4.2 Tier 7,** the page model: borrowed `PageEvent`s, paint events carrying everything the architecture lists, overprint
settings (`OP`, `op`, `OPM`) per paint, clips, groups and masks.

**4.3 The colour-space evaluator.** One function that judges a resolved space against the output intent, honouring the
`Default*` spaces, the bases of `Indexed` and `Pattern` spaces, spot alternates (X4-Q4), group `/CS`, inline images,
shadings, and the colour implied by JPEG 2000. It covers:
- 6.4.3.1–6.4.3.4 (indirect, device, `ICCBased`, Separation and DeviceN), including the `OPM` rule;
- 6.20 (transparency: blend modes and the blend space).

**4.4 Images and JPEG 2000.** A new helper reads the JPEG 2000 boxes (`jp2h`, `ihdr`, `colr`, `bpcc`, `rreq`) and the
codestream's SIZ marker. It covers **6.27**, the JPEG 2000 part of **6.8**, and **6.16**'s content-dependent rows. DCT and
JBIG2 are checked at header level first; full decoding only if a finding needs it.

**4.5 Pre-separated pages** (6.1.2), per decision X4-Q2.

**4.6 Port today's print checks** with the parity gate, as the brief's stage 3 sets out:
- total ink through Ghostscript's `tiffsep` (bug 4);
- overprint and white overprint, spots and ICC, gray and K-only modes;
- text, small text, hairlines, contrast, resolution;
- geometry, bleed coverage, marks and hidden content.

Brief decision point 2 applies: measure the pace of porting `paint.py` one week in, and re-plan.

**4.7 Test material:**
- Ghent patches for transparency, DeviceN and NChannel, ICC v4, JPEG 2000, 16-bit images and overprint;
- Altona;
- veraPDF colour folders, only the "same rule" ones;
- built negatives from §3, items 4, 5, 16 and 19;
- today's colour fixtures under the parity gate.

*Done when* the rows for 6.4.3, 6.19, 6.20, 6.23, 6.27 and 6.1.2 are `implemented`, the parity gate passes on the corpus, and
the Python versions are deleted. Judgement: 4–6 weeks; the brief's 3–4 plus the colour evaluator and JPEG 2000.

## Phase 5: fonts (clause 6.5)

**5.1 The symbolic `cmap` count** (6.5.4). It needs only the `cmap` table header, so it ships first.

**5.2 Reading font programs** (a fonts helper):
- TrueType and OpenType: `cmap`, `hmtx`, `glyf`/`loca`;
- CFF and Type 1 (`eexec`);
- CID fonts with their CMaps.

Choose crates (for example `skrifa` or `ttf-parser`, and hayro's font code) only after `cargo deny` accepts their licences;
write our own where none fits. It covers:
- 6.5.1, embedding and glyph presence, including inside hidden optional content;
- 6.5.2, complete `.notdef` detection;
- 6.5.3, width consistency (X4-Q16);
- 6.5.4, encodings.

**5.3 Test material:**
- veraPDF's font folders (the same rule, so the verdicts carry over);
- Ghent's OpenType patches;
- today's font fixtures;
- built negatives from §3, item 6.

*Done when* every 6.5 row is `implemented`. Judgement: 3–5 weeks; parsing font programs is the largest single dependency.

## Phase 6: the CLI and the Python API (the brief's stage 4) — the PDF/X-4 milestone

**6.1** The brief's stage 4:
- clap, `proof-pdf.toml` with `schema = 1` and `extends`;
- exit codes, the JSON;
- veraPDF and Ghostscript through `process-wrap`;
- `test_cli.py` passing against the binary.

**6.2 PDF/X level selection.** For example `--pdfx x4|x4p|x5g|x5pg|x5n`; settle the name then. The old `--no-pdfx` and
`pdfx = false` keep working as aliases. A level selects the `checks.toml` entries whose `applies` contains it.

**6.3 The Python API** stays async and keeps its signatures, now a thin layer over the extension.

*Done when* the brief's stage 4 is done and every X-4 row that needs no external file is `implemented`. This is the
**PDF/X-4 milestone**: a release candidate, and the brief's place to stop if needed. Judgement: 1–2 weeks.

## Phase 7: file sets and PDF/X-4p

**7.1 Verifying a set of files.** Resolve file specifications (the 7.11 helper) relative to the main file. Open each referenced
file as its own document: `EventId.doc` already tells documents apart. Keep one visited set across files, and give findings
a file component in their chain. URLs are never fetched: a reference that only a network could confirm is "not evaluated",
which fails.

**7.2 Two modes.**
- *File alone*, which clause 5 of ISO 15930-8 allows: rows that need another file are "not evaluated", and the profile
  says so.
- *File set*: every referenced file must be present.

**7.3 The X-4p rows** (15930-7 Annex A):
- `DestOutputProfileRef` and its keys;
- the `CheckSum` as the MD5 of the external profile, byte for byte;
- `ProfileCS` and `ICCVersion`;
- no `DestOutputProfile`; the `URLs` exemption from X4-Q12;
- the identity test of X5-Q11.

**7.4 Test material,** all built: an X-4p file with its external Little-CMS profile; a wrong checksum; a missing profile; a
URL-only reference; the three veraPDF files that reference an absent path (`x5-register.md` §6) as "file missing" cases.

*Done when* every X-4p row is `implemented` in both modes. Judgement: 1–2 weeks.

## Phase 8: PDF/X-5

**8.1 Identification and inheritance.** The X-5 identification rows (15930-8, 6.2), and the inheritance map in
`x5-register.md` §1 expressed through `applies` in `checks.toml`. A modified X-4 rule becomes a separate check id for the X-5
level, never a flag inside the X-4 check.

**8.2 X-5g and X-5pg** (clauses 8 and 9):
- reference XObject proxies (`Ref` with `ID`, the target's page);
- the XMP identifiers of target and proxy, which need the XMP helper's struct support;
- matching against the target's declared IDs (X5-Q5, X5-Q6);
- no cycles, and the depth limit (X5-Q13);
- the same printing condition (X5-Q7);
- the ban on OCGs in targets (X5-Q8);
- what the proxy's `BBox` means.

**8.3 Checking a target recursively.** A target is verified at the level it claims, with the same engine. X-4, X-4p and X-5
targets are supported; X-1a and X-3 targets are "not evaluated" (X5-Q9).

**8.4 X-5n** (clause 7):
- the n-colorant output intent (`xCLR`, a required `ColorantTable`);
- reading ISO 15076-1 v4 n-colorant profiles, an extension of the ICC helper;
- colorant-name rules in place of 6.4.3.2;
- DeviceN attributes;
- a required `Group` with `/CS` on pages with transparency (X5-Q12);
- the TrapNet `PCM` rule.

**8.5 Test material,** all generated: test-db has no PDF/X-5 file and no n-colorant profile.
- **X-5g/X-5pg:**
  - container and target pairs whose IDs match, differ only in version, or don't match;
  - a cycle;
  - a target with OCGs;
  - targets at X-1a and X-3, which test "not evaluated".
- **X-5n:** files with synthetic 5- and 6-colorant v4 profiles, built with `lcms2`, both external and embedded.

Generated files go to test-db's `samples/generated/x5/`, each with a provenance row, and the redistributable ones also to
`tests/fixtures/pdfx/`.

*Done when* every X-5 row is `implemented` or `skip`, in both modes. Judgement: 3–5 weeks.

## Phase 9: the writer and the other commands (the brief's stage 5)

The writer per the architecture note, then `crop`, `inspect` (as recording listeners), `extract`, `preview` (`hayro`),
`layout` and `texlog`. pikepdf and Pillow leave runtime. Phase 8's generated X-5 fixtures can move from a script to the writer
here.

*Done when* the brief's stage 5 is done. Judgement: 3–4 weeks.

## Phase 10: release (the brief's stage 6)

- Migrate the Python tests; rebuild the Docker images; update the release workflow; remove `spikes/`.
- Generate the README check table from `checks.toml`.
- Write the changelog's behaviour changes: the brief's list, plus PDF/X level selection, `Trapped` now judged in XMP, and
  the new `x4.*` and `x5.*` check ids.
- A last full corpus run over test-db, with the results file kept in test-db.

*Done when* the release dry run passes and `checks.toml` has no `todo`. Judgement: 1–2 weeks.

## Summary

| Phase | Brief stage | Register rows owned | Judgement (weeks) |
|---|---|---|---|
| 0 Decisions and groundwork | 0 | — | 1 |
| 1 Infrastructure | 1 | all seeded as `todo` | 1–2 |
| 2 Reading | 2 | 6.1.1, 6.6, 6.8, 6.15, 6.25 | 2–3 |
| 3 Document structure and metadata | 3 (part) | 6.4.1–6.4.2, 6.7, 6.9–6.14, 6.16–6.18, 6.21, 6.22, 6.24, 6.26; 6.4.3.3 identity | 3–4 |
| 4 Content, page model, colour | 3 | 6.1.2, 6.4.3, 6.19, 6.20, 6.23, 6.27; the ported print checks | 4–6 |
| 5 Fonts | — | 6.5 | 3–5 |
| 6 CLI and API (**PDF/X-4 milestone**) | 4 | the rest of X-4 | 1–2 |
| 7 File sets, X-4p | — | 15930-7 Annex A | 1–2 |
| 8 PDF/X-5 | — | 15930-8, clauses 6–9 | 3–5 |
| 9 Writer and other commands | 5 | — | 3–4 |
| 10 Release | 6 | — | 1–2 |

About **23–36 weeks** for one experienced engineer, or **15–23 to the PDF/X-4 milestone** (phases 0–6). These are judgements,
not measurements: the only measured port is still `x4.py`'s 130 lines. Phases 3 and 5 can overlap phase 4 if two people work
on them. Each phase's tasks are ordered so that the gate stays green after every one.

## When to stop and re-plan

1. **After phase 0:** if decisions X4-Q1–Q21 change the register materially, re-count the rows first.
2. **After phase 2**, the brief's first decision point: if the parser refuses real InDesign or Illustrator output that prints
   correctly.
3. **One week into phase 4:** the pace of porting `paint.py` (the brief's second decision point).
4. **When phase 5 chooses its font crates:** if no crate's licence fits and our own Type 1 and CFF parsers are needed, add
   2–3 weeks by judgement.
5. **At the PDF/X-4 milestone:** release X-4, or carry on to X-4p and X-5. A Ghent test page that fails here is a
   blocker, since it is the only verdict we have from a third-party preflight tool.

## Risks

| Risk | Signal | Response |
|---|---|---|
| No free PDF/X oracle, so our reading of the standard goes unchecked | A Ghent positive fails, or a dispute over a clause | Classify it (working rules); for a contested case, ask the owner to run Acrobat Preflight or pdfToolbox once |
| GWG says no to public CI | The reply to 0.3 | Ghent stays a local corpus run; CI relies on our fixtures and the fetched public sets |
| Font program parsing grows | Phase 5 estimate missed by its second week | Ship 6.5.4 and embedding first; width consistency last |
| Interpretation drift between X-4 and X-5 checks | The same property judged twice | One check per property; X-5 modifies by a separate id, never by a flag |
| Fixtures that claim more than they are | A positive fixture failing the register | Every positive fixture is listed in `expected.toml` with no X-4 findings, so the claim is tested |
