# Docs

Where proof-pdf's plans, decisions and ideas are written down. Start here. Each section points to the document that holds the detail,
so nothing is said twice.

| Document | What it is |
|---|---|
| [`briefs/rust-port.md`](briefs/rust-port.md) | **The plan**: moving the core to Rust on a parser of our own. Decisions, stages, the parity gate, Python bugs to fix first, and a backlog of checks. It wins wherever documents disagree |
| [`briefs/pdfx-plan.md`](briefs/pdfx-plan.md) | **The implementation plan** to a feature-complete PDF/X verifier (X-4, X-4p, X-5g, X-5pg, X-5n): phases 0-10 with tasks, the register rows each owns, test material and done criteria. The requirement registers it cites are private, in `proof-pdf-test-db/docs/pdfx/` |
| [`briefs/pdfx-run.md`](briefs/pdfx-run.md) | The init prompt for an agent that carries out the plan unattended: what it may decide, the no-hacks rules, when it must stop and ask, and a one-run exception that lets it commit |
| [`notes/rust-architecture.md`](notes/rust-architecture.md) | **The architecture** of the Rust engine: tiers, the listener pipeline, events, damage and findings |
| [`notes/rust-feasibility.md`](notes/rust-feasibility.md) | **The feasibility summary** for planning: what is proven and how well, a module-by-module map, gotchas, risks and decision points |
| [`notes/rust-port-spikes.md`](notes/rust-port-spikes.md) | **The evidence** behind that plan: what was measured, on which files, and the results, including the comparison with Kura |
| [`briefs/bleed-and-suites.md`](briefs/bleed-and-suites.md) | An earlier brief: the bleed-coverage check and calibration against the Ghent and Altona suites |
| [`notes/cover-checks.md`](notes/cover-checks.md) | Notes on checks for book covers |

## Decisions

Newest first. A decision changes only by a new line here.

| Date | Decision | Where |
|---|---|---|
| 2026-11-26 | PDF/X-5n (15930-8 clause 7): "ISO 15076-1" (7.1) is ICC version 4.2 or later, and the external profile is an output (`prtr`) profile as 6.4.2.1 asks via A.1; without the profile, `/ICCVersion` alone can show an earlier version. A URL file specification's `/EF` is read as a candidate profile (7.2). The device bans of 7.3 cover print elements, the alternates they reach (X5-Q10), transparency group spaces and JPEG 2000 data read as `DeviceCMYK`. `/ColorantTable`'s count against the xCLR channels and the 3CLR/4CLR name test are warnings (X5-Q4); `x5.n-subtractive` is skipped, as nothing records it. The fixtures' n-colorant profiles are written by `build_pdfs.py` and must open in LittleCMS | Plan, tasks 8.4, 8.5 |
| 2026-11-26 | ISO 15930-8:2010 Technical Corrigendum 1 (2011), supplied by the owner, corrects 8.3's and 8.4's `stRef` properties to `documentID`, `versionID` and `renditionClass`: X5-Q6's reading, now the text's. A field in another case stays its own finding | Owner; test-db `standards/iso/` |
| 2026-11-26 | PDF/X-5 inheritance (15930-8 6.1, x5-register §1) as `applies`: X-5g takes every X-4 row, X-5pg and X-5n every X-4p row, except those 15930-8 replaces with rows of its own: the version (6.2), 6.7's `Ref` and `/EF` rules for X-5g and X-5pg (8.2), and for X-5n the colour rows 7.1-7.3 replace (channels, device spaces, alternates, `/Process`, TrapNet `PCM`, group `/CS`, JPX CMYK) and A.2's `ProfileCS`, `ColorantTable`, URL `/EF` and profile rows. Every `/URLs` file specification is exempt from 6.7's `/EF` at every level (a reference at X-4 already fails). `x5.g-ref-present` is a warning (X5-Q1); `x5.self-contained-is-x4` is skipped as the same test at X-5g and never true at X-5pg; `x5.g-proxy-bbox-target-page` is skipped, as 8.5 says how a reader places a page, not what a file holds | Plan, task 8.1 |
| 2026-11-26 | PDF/X-5g and X-5pg file sets (8.2-8.7): a `/Ref`'s `/F` is a file name relative to the container (or a URL file specification, or an embedded file, X5-Q3); targets are read once each and verified at the level they claim (X-4, X-4p, X-5g, X-5pg), down to `Limits::target_depth` = 16 files (X5-Q13); X-1a and X-3 targets are not evaluated (X5-Q9). A missing target fails `x5.g-target-identity` (the set is incomplete); a network-only one is not evaluated. A target fails `x5.g-target-conforms` on its errors and unjudged checks, not its warnings. A target of another version needs `--target-versions accept\|reject` (X5-Q5); without it, not evaluated. Page labels (ISO 32000-2 12.4.2) are read for `/Page` labels | Plan, tasks 8.2, 8.3 |
| 2026-11-26 | PDF/X-4p file sets (ISO 15930-7 A.1, A.2): the command line verifies a file with the files it refers to by default (`--file-alone` to verify it alone, as the Python `_core.verify` always does). The external profile is looked for at the `/URLs` that are local: relative references and `file:` URLs, resolved against the PDF's folder and percent-decoded; other schemes are never fetched. The candidate whose whole-file MD5 is the `/CheckSum` is the profile. No local match fails `x4p.fileset.checksum-matches` unless a network URL remains, which leaves it not evaluated. A file verified alone cannot pass X-4p (its file-set rows are not evaluated). `/ProfileName` matches any record of a multi-localized description; `/ProfileCS` and `/ICCVersion` compare as bytes (X4-Q17). Under X-4p the colour-space rows use the external profile or `/ProfileCS`; 6.4.3.3 applies against the external profile by identity, and without it a profile whose MD5 is the `/CheckSum` fails, one under a non-CMYK `/ProfileCS` passes, and anything else is not evaluated. Every `/URLs` element is exempt from 6.7's `/EF` rule (X4-Q12) | pdfx-oracle, no escalation; plan tasks 7.1-7.3 |
| 2026-11-26 | A PDF/X level reports its rows still marked `todo` as not evaluated ("not written yet"), so X-4p and the X-5 levels cannot pass before phases 7 and 8 write them; the text report lists unjudged checks one line per reason. The print rows `bleedbox`, `boxes` and `structure` become `skip`: the `x4.boxes.*` rows, the new `page.media-box` (ISO 32000-2 7.7.3.3, at every level) and the report's structure replace them | Plan task 6.2; found at the milestone |
| 2026-11-26 | The Rust `verify` runs veraPDF and Ghostscript only when asked (`--pdfa`, `--tac`), as the open question below assumes, through `process-wrap` (MIT or Apache-2.0), and reads Ghostscript's plates with `tiff` (MIT, no codecs: `tiffsep` writes them uncompressed). A check that reports a tool's verdict says so in `checks.toml` (`tool`), and is tested with stand-in tools instead of fixtures | Brief, stage 4; plan task 6.1 |
| 2026-11-26 | PDF/X-4 6.25's graphics-state nesting (Table C.1 of the PDF 1.6 Reference: 28) counts every saved state while a page is drawn: each `q`, and each form or tiling pattern being drawn, which saves the state as if by `q`. veraPDF counts only `q` within one content stream, so a file can pass it and fail here. Every `DeviceN` space in the file counts against its 32-component limit, used or not (veraPDF counts used ones), and every CID an embedded CMap maps to against 65,535. Reals nearer to 0 than the table's smallest are not a failure: the table says they read as 0 | pdfx-oracle, recommended for the log; plan task 6 (milestone rows) |
| 2026-11-26 | PDF/X-4 6.5.1's glyph presence skips a glyph (a font's code) only when every appearance of it is in render mode 3: one visible appearance, among any number of invisible ones, and it is judged | Owner, after an oracle escalation |
| 2026-11-26 | Codes under a predefined CMap are judged through Adobe's predefined CMaps, bundled by `hayro-cmap` (Apache-2.0 or MIT; the CMap data is Adobe's, BSD-3-Clause, copyright 1990-2023 Adobe), which also parses embedded CMaps. Font programs are read with `read-fonts` (MIT or Apache-2.0): TrueType and OpenType tables, bare CFF and Type 1; hayro-font was not taken, as it reads PDFs through its own parser | Owner, after an oracle escalation; plan task 5.2 |
| 2026-11-26 | Colour conversion in the core uses `lcms2` (LittleCMS 2.20, built from the crate's bundled source and linked statically, so the extension still links only system libraries). It reproduces Pillow's 8-bit sRGB and CMYK to Lab conversions exactly (1,512 of 1,512 colours, our test CMYK profile included); `moxcms` failed earlier. Image samples: `zune-jpeg` for JPEG and `hayro-jpeg2000` for JPEG 2000, both pure Rust, which give Pillow's samples | Brief, ICC; progress file, task 4.6 |
| 2026-11-26 | PDF/X-4 6.27's "JPX file format, baseline features only" is read as written (ITU-T T.801 M.9.2): plain JP2 files without the `jpx ` and `jpxb` brands and a Reader Requirements box fail `x4.jpx.baseline`, as do bare codestreams. Adobe's output, and so every Altona and Ghent file, passes; veraPDF's PDF/A-2 test files do not | Owner, after an oracle escalation |
| 2026-11-25 | PDF/X-4 6.4.3.4: the rule that same-named Separation arrays share their tint transform and alternate follows X4-Q3, so it judges the separations print elements use (with the `/Colorants` of used DeviceN spaces), not every array in the file. It covers every name, process inks such as Black included. Altona's odd `/Black`, in a vendor thumbnail no page paints, is out of scope. The `PrintingOrder` and `Solidities` rows stay file-wide, as stated | Owner, after an oracle escalation |
| 2026-11-25 | Verifying a file never uses the network. Reference data, such as the ICC characterization registry's reference names, is built into the code with its source and date, and refreshed by hand; a missing local tool (veraPDF, Ghostscript) is reported as a finding for its layer and the other checks still run | Owner |
| 2026-11-25 | PDF/X-4 6.4.2.1: an identifier under the ICC `/RegistryName` that is not one of the registry's reference names is a warning (`x4.output-intent.identifier-in-registry`), not the clause's error. 14 Ghent X-4 positives (Adobe output) put an ECI profile name there and pdfToolbox passes them. A misspelt color.org URL with an ICC name fails; another registry's URL is not judged; "Custom" or an empty identifier is a warning | Owner, after an oracle escalation |
| 2026-11-25 | PDF/X-4 6.16's "alternates show the same area of the same master image" is a `skip`: it needs an image-similarity judgement across resolution, colour space and bit depth, which would guess. The other half of 6.16, no alternate `/DefaultForPrinting true`, is checked | [`briefs/pdfx-progress.md`](briefs/pdfx-progress.md) |
| 2026-11-25 | PDF/X-4 6.4.2.1: an empty `/RegistryName` names no registry and is read as absent, so no registry rule applies to it. Adobe PDF Library writes `/RegistryName ()` for every condition outside a registry; 12 Ghent positives carry it and pdfToolbox passes them. The oracle agreed (medium-high confidence, no escalation). Dates in 6.10.2 compare as instants in UTC when both carry a zone, as written when neither does, and never match when only one does | [`briefs/pdfx-progress.md`](briefs/pdfx-progress.md) |
| 2026-11-25 | A duplicate dictionary key is a warning (`syntax.duplicate-key`), not an error, though ISO 32000-2 7.3.7 says "shall not": LuaLaTeX's `pdfx` package writes catalog keys twice in files that print correctly, qpdf only warns, and pdfToolbox's PDF/X-4 profile with syntax checks does not object. The brief's rule: error until a real file that prints correctly trips it | `rust/checks.toml`; brief, stage 2 |
| 2026-11-25 | One crate with a `python` feature, as the brief proposed: maturin builds `proof_pdf._internal._core` from `rust/`, the package's version is the crate's, and releases ship wheels for macOS arm64 and Linux x86-64 and arm64. PyO3 works under `forbid(unsafe_code)` | Plan, task 1.7; architecture note |
| 2026-11-25 | The Ghent Output Suite's PDF/X-4 positives are the 35 of its 39 X-4 claims that pdfToolbox 17.1 passes, the four signed test pages among them. GWG151 and GWG152 do not conform to 6.24 (their `/Order` leaves out content OCGs) and are negatives for `x4.oc.order-complete`; the pdfx-oracle agreed. The other two fail many rows | Plan, done criteria; test-db `expectations/pdfx.toml` |
| 2026-11-25 | Both Altona technical2 X-4 files do not conform to ISO 15930-7 6.4.3.3: a `DefaultCMYK` equal to the output intent's profile remaps two painted DeviceCMYK images each (6.4.3.1). Classified "the file does not conform": they are expected to raise only `x4.icc.cmyk-not-intent-profile`, twice each, and stay positives for every other row. pdfToolbox 17.1 and the pdfx-oracle agree; approved by the owner | Plan, done criteria and task 3.6; progress file |
| 2026-11-25 | PDF/X-4 6.4.2.1 and 6.4.3.3 accept an ICC profile of major version 2 or 4, any minor version; 5 and anything else fails. The clauses' list of ICC editions is read as the families they belong to. Evidence: pdfToolbox 17.1's "Verify compliance with PDFX-4" passes `x4-good` with its output profile at 2.0, 2.1, 2.4, 4.2, 4.3 and 4.4 and fails it at 5.0; the Ghent positives (2.4) and Altona (2.0) would fail any narrower reading. Agrees with the pdfx-oracle | Plan, task 0.4; progress file |
| 2026-11-25 | PDF/X 6.25 means the PDF 1.6 Reference's Table C.1 only (now held): numbers, strings inside content streams, names, indirect objects, `q` nesting, DeviceN components and CIDs. It has no array or dictionary limit, and the page-size limit sits outside the table, so neither is a PDF/X check | Plan, tasks 0.2 and 2.2 |
| 2026-11-25 | X4-Q6 stands as decided: no corrigendum or amendment to ISO 15930-7:2010 was found. ISO 15930-8:2010 has a Technical Corrigendum 1 (2011); held since 2026-11-26 (line above) | Plan, task 0.2 |
| 2026-11-25 | The 34 PDF/X interpretation questions (X-4 Q1-Q21, X-5 Q1-Q13) take the defaults proposed in the plan, among them: `Trapped` judged in XMP; every action fails; spot alternates follow the colour rules; targets at X-1a or X-3 are "not evaluated" | Plan, task 0.1 |
| 2026-11-25 | Feature-complete means every file requirement of PDF/X-4, X-4p (ISO 15930-7) and X-5g, X-5pg, X-5n (ISO 15930-8), plus today's print checks | Plan |
| 2026-11-25 | Content streams, the page model, checks and reports, limits, the PyO3 and CLI boundaries and the writer settled from a survey of qpdf, pdf.js, PDFBox, MuPDF, hayro, veraPDF, SARIF, ruff and ESLint. Checks report a status separate from severity; a form without `/Resources` uses the page's | Architecture note |
| 2026-11-25 | Reversed the line below: PDF/A stays veraPDF's verdict. Rules must come from the ISO texts, and ISO 19005-1, -3 and -4 are not held | Brief, non-goals |
| 2026-11-25 | proof-pdf validates PDF/A (every part and level) and PDF/UA with its own rules, proved against veraPDF, which stays available as an optional tool | Brief, decisions |
| 2026-11-25 | Findings cite a clause; each check declares the clauses it may cite, validated against a committed list of clause numbers | Architecture note |
| 2026-11-25 | Only file-level damage refuses a file. Damaged objects and streams leave gaps, the file still fails, and reading inside a stream stops at the first damage | Architecture note |
| 2026-11-25 | Eight tiers following ISO 32000-2, each publishing events to synchronous listeners: observers, one proxy, then readers | Architecture note |
| 2026-11-07 | Private samples and licensed material live in a separate repository, `proof-pdf-test-db` (Azure DevOps, Git LFS), never in proofpress | Below |
| 2026-11-07 | Total ink counts each spot colour as one ink, at its tint, measured with Ghostscript's `tiffsep` | Brief, bug 4 |
| 2026-11-07 | From Kura (AGPL), ideas only: no code, no profile text, no thresholds. Run it, if at all, as an optional external tool | Brief, backlog |
| 2026-11-06 | Read PDFs with a strict parser of our own: no repair, no encryption. A file that needs either fails with the reason | Brief, the parser |
| 2026-11-06 | Port the core to Rust. The CLI becomes Rust, and the Python package calls the same core through PyO3 | Brief |

Proposed in the brief and not yet confirmed: our own small PDF writer instead of `lopdf`; `hayro` for previews.

## Open questions

| Question | Default until answered | Where |
|---|---|---|
| Do veraPDF and Ghostscript become optional, so PDF/A and total ink are opt-in? | Yes. Replacing them instead is about 12-20 more weeks, by judgement | Brief, decisions |
| Which parser refusals are errors and which are warnings? | Error, until a real file that prints correctly trips one | Brief, stage 2 |
| Does the parser refuse real InDesign and Illustrator output that prints correctly? | Probably not: all 91 InDesign and Illustrator files in the Ghent Output Suite pass. Client jobs would settle it | Brief, stage 0 |
| Can `moxcms` replace `lcms2`, making the core pure Rust? | No; two attempts failed, but its README claims CMYK to Lab | Note |
| Why do 5 files pass the parser when qpdf had to rebuild their xref, and 3 pass that pikepdf and Kura refuse? | Unexplained | Brief, parser to-do |
| Is Kura worth running as a third opinion, next to veraPDF and qpdf, when judging a parser refusal? | Not set up | Ideas, below |

## Where samples live

| Kind | Where | Rule |
|---|---|---|
| Built fixtures | `tests/fixtures/pdfs/` | Committed. Tests depend on them |
| Altona test suite | `tests/fixtures/suites/.cache/`, by `bin/suites/fetch.sh` | Not redistributable, so fetched and never committed |
| Every other sample, standards, private notes | `proof-pdf-test-db`, a sibling checkout at `../proof-pdf-test-db` (`/Users/ilo/Devel/Release.art/proof-pdf-test-db`) | Never copied out. Every sample gets a row in its `samples/PROVENANCE.csv`. No test may depend on it |

Standards (ISO 32000, ISO 15930 for PDF/X, ISO 19005 for PDF/A, ISO 14289 for PDF/UA, the GWG specifications) go in that repository's `standards/`,
if their licence allows a shared copy. The PDF/X rules in `x4.py` and the GWG presets in the backlog are written from them,
clause by clause.

## Positioning

From comparing with Kura on 2026-11-07, recorded in the note. Kura converts, repairs and covers far more standards. proof-pdf should
not race it on breadth. It is the independent check you run on any file, including one Kura produced:

- It never repairs, so it reports defects a converter fixes silently.
- PDF/A is veraPDF's verdict, not ours.
- Total ink is measured from a CMYK render, not read from object colours.
- A check that cannot run fails; it is never reported as a pass.
- Every refusal says what is wrong and where.

## Ideas not yet in the plan

Worth keeping, but not scheduled. Move one into the brief's backlog when it is wanted.

**Corpus and testing**
- Draw a sample of the 29,711 web PDFs listed in Kura's `bench/MANIFEST.csv`: 1,927 from Common Crawl and 27,784 from the DARPA
  SafeDocs crawl, each with source URL and SHA-256. That is the large set of real-world files the parser has not met. They are web
  documents, not print jobs.
- Run Kura as a third opinion in `spikes/crabify/oracle.py`. Where veraPDF, qpdf and Kura agree a file is damaged, a refusal is fair.
- Use the cases in test-db's `samples/pdf-differences/` as tests when porting the checks: `DefaultColorSpaces` and `IndexedColor` for colour,
  `NegativeFontSize` and `TextClipModeChanges` for text, `Inline-Image-Abbreviations` for the parser.

**Checks, lower priority than the brief's backlog**
- Placement zones: a named rectangle an object must sit inside, such as a logo area or a barcode zone. Today the safe area applies
  only to text.
- Blank-area search: is there an empty rectangle of a given size inside the trim, for an address panel or a stamp?
- Used area: the extent of everything that prints, against the trim.
- Bleed along a die line: ink must cover a band outside a cutting contour, not just past the trim box. Packaging only.
- Corner angles and curve radii on cutting contours. Packaging only.

**PDF/A and PDF/UA**
- Our own rules, if the remaining standards become affordable. ISO 19005-2 (PDF/A-2, the default flavour), 14289-1 and -2 (PDF/UA) and
  32000-1 are held; 19005-1, -3 and -4 are not. test-db's `samples/veraPDF-corpus/` (2,906 files, CC BY 4.0, named by clause and
  expected result) would test each rule, with veraPDF as the oracle. Font programs, XMP and the structure tree are the hidden cost.

**Reporting**
- Render a saved JSON report as HTML or PDF for a print shop, only if one asks for it.
