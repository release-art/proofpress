# Docs

Where proof-pdf's plans, decisions and ideas are written down. Start here. Each section points to the document that holds the detail,
so nothing is said twice.

| Document | What it is |
|---|---|
| [`briefs/rust-port.md`](briefs/rust-port.md) | **The plan**: moving the core to Rust on a parser of our own. Decisions, stages, the parity gate, Python bugs to fix first, and a backlog of checks. It wins wherever documents disagree |
| [`notes/rust-feasibility.md`](notes/rust-feasibility.md) | **The feasibility summary** for planning: what is proven and how well, a module-by-module map, gotchas, risks and decision points |
| [`notes/rust-port-spikes.md`](notes/rust-port-spikes.md) | **The evidence** behind that plan: what was measured, on which files, and the results, including the comparison with Kura |
| [`briefs/bleed-and-suites.md`](briefs/bleed-and-suites.md) | An earlier brief: the bleed-coverage check and calibration against the Ghent and Altona suites |
| [`notes/cover-checks.md`](notes/cover-checks.md) | Notes on checks for book covers |
| [`../corpus/README.md`](../corpus/README.md) | Public sample PDFs: what each set is, its licence, how to fetch it |

## Decisions

Newest first. A decision changes only by a new line here.

| Date | Decision | Where |
|---|---|---|
| 2026-11-07 | Private samples and licensed material live in a separate repository, `proof-pdf-test-db` (Azure DevOps, Git LFS), never in proofpress | Below |
| 2026-11-07 | Total ink counts each spot colour as one ink, at its tint, measured with Ghostscript's `tiffsep` | Brief, bug 4 |
| 2026-11-07 | From Kura (AGPL), ideas only: no code, no profile text, no thresholds. Run it, if at all, as an optional external tool | Brief, backlog |
| 2026-11-06 | Read PDFs with a strict parser of our own: no repair, no encryption. A file that needs either fails with the reason | Brief, the parser |
| 2026-11-06 | Port the core to Rust. The CLI becomes Rust, and the Python package calls the same core through PyO3 | Brief |

Proposed in the brief and not yet confirmed: our own small PDF writer instead of `lopdf`; `hayro` for previews; `lcms2`, which is C,
for colour; one crate with a `python` feature.

## Open questions

| Question | Default until answered | Where |
|---|---|---|
| Do veraPDF and Ghostscript become optional, so PDF/A and total ink are opt-in? | Yes. Replacing them instead is about 12-20 more weeks, by judgement | Brief, decisions |
| Which parser refusals are errors and which are warnings? | Error, until a real file that prints correctly trips one | Brief, stage 2 |
| Does the parser refuse real InDesign and Illustrator output that prints correctly? | Unknown, and the one result that could change the plan. Needs recent jobs in `proof-pdf-test-db` | Brief, stage 0 |
| Can `moxcms` replace `lcms2`, making the core pure Rust? | No; two attempts failed, but its README claims CMYK to Lab | Note |
| Why do 5 files pass the parser when qpdf had to rebuild their xref, and 3 pass that pikepdf and Kura refuse? | Unexplained | Brief, parser to-do |
| Is Kura worth running as a third opinion, next to veraPDF and qpdf, when judging a parser refusal? | Not set up | Ideas, below |

## Where samples live

| Kind | Where | Rule |
|---|---|---|
| Built fixtures | `tests/fixtures/pdfs/` | Committed. Tests depend on them |
| Altona test suite | `tests/fixtures/suites/.cache/`, by `bin/suites/fetch.sh` | Not redistributable, so fetched and never committed |
| Public sample sets | `corpus/`, by `bin/suites/corpus.sh` | Git-ignored. No test may depend on them |
| Private jobs, standards, private notes | `proof-pdf-test-db`, cloned to `corpus/local/test-db` | Never copied out. Every sample gets a row in its `samples/PROVENANCE.csv` |

Standards (ISO 32000, ISO 15930 for PDF/X, ISO 19005 for PDF/A, the GWG 2022 specification) go in that repository's `standards/`,
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
- Use the cases in `corpus/pdf-differences/` as tests when porting the checks: `DefaultColorSpaces` and `IndexedColor` for colour,
  `NegativeFontSize` and `TextClipModeChanges` for text, `Inline-Image-Abbreviations` for the parser.

**Checks, lower priority than the brief's backlog**
- Placement zones: a named rectangle an object must sit inside, such as a logo area or a barcode zone. Today the safe area applies
  only to text.
- Blank-area search: is there an empty rectangle of a given size inside the trim, for an address panel or a stamp?
- Used area: the extent of everything that prints, against the trim.
- Bleed along a die line: ink must cover a band outside a cutting contour, not just past the trim box. Packaging only.
- Corner angles and curve radii on cutting contours. Packaging only.

**Reporting**
- Render a saved JSON report as HTML or PDF for a print shop, only if one asks for it.
