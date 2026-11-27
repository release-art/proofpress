# PDF/X-1a and X-3 plan: progress

The running log of the unattended run that carries out [`pdfx-1a-3-plan.md`](pdfx-1a-3-plan.md), started 2026-11-27 with
the prompt in [`pdfx-1a-3-run.md`](pdfx-1a-3-run.md). A fresh session resumes from **Status**.

## Stop questions

Answered 2026-11-27 (end of phase 0), by the owner through the session that started the run: "Accept both".
- L-Q3: (a), transparency (15930-3 Annex D.2's keys) is an error at X-1a:2001 and X-3:2002; form groups alone are not
  judged there.
- Carry on with the plan as written, all four levels, accepting the 44 % new rows.
- Q17 and Q22 at X-1a:2001 were not answered; the proposed defaults are taken (render-mode-3 fonts need no embedding;
  an embedded file at X-1a:2001 is a warning).

## Status

- Phases 0 and 1 are done. Phase 2 is in progress: **The plan is carried out.** Phases 0-3 are done (see the stage reports). What is left is the owner's (owner actions).

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | test-db `156acfd5` | `docs/pdfx/x1a3-register.md`: 85 file-rule rows, 48 mapped to `x4.*` checks, 37 new |
| 0.2 | this commit; test-db register §4 | L-Q1, Q2, Q4-Q10 and Q11-Q22 settled with the oracle (decision log); L-Q3, Q17 and Q22 at X-1a:2001 escalated, then answered by the owner |
| 1.1 | this commit | Levels `x1a-2001`, `x3-2002`, `x1a-2003`, `x3-2003`; detection of a claimed older level; the report's `level`; the 42 new rows as `todo`; clause lists for 15930-1, -3, -4, -6 |
| 1.2 | this commit | 47 `x4.*` checks and the 21 structure checks applied at the older levels; `cite` maps their clauses; X-4 and X-5 expectations unchanged |
| 1.3 | this commit | Info identification, dates, title and Trapped, the TrapNet rules (12 checks, `checks/info.rs`); bases `x1a-2001-good`, `x3-2002-good`, `x1a-2003-good`, `x3-2003-good` and 13 negatives; scenarios in `x13-identification-trapping.feature` |
| 2.1 | this commit | Output intent and print-element colour at the older levels (14 checks); 18 fixtures, `x13-colour.feature` |
| 2.2 | this commit | Transparency at the older levels (3 checks, `checks/transparency.rs`); 7 fixtures, `x13-transparency.feature` |
| 2.3 | this commit | Cross-reference and object streams, filter lists, JBIG2, 16-bit images, optional content (7 checks, `checks/x13.rs`); 8 fixtures, `x13-structure-filters.feature` |
| 2.4 | this commit | `/TR2`, file specifications, embedded files at X-1a:2001, trim inside crop and annotations at 2001/2002, viewer preferences, font embedding permission (7 checks); 7 fixtures, `x13-keys-boxes.feature` |
| 2.5 | this commit | X-5 targets verified at the older level they claim (L-Q10); `x5g-target-x1a` now conforms |
| 3.1 | (2.x commits) | One positive per level (`x1a-2001-good`, `x3-2002-good`, `x1a-2003-good`, `x3-2003-good`) and a one-defect negative per new check, built from x4-good |
| 3.2 | this commit | pdfToolbox over the 62 older-level fixtures; one fixture of ours had a second defect (an invalid CalRGB), fixed; every other disagreement upheld by the oracle |
| 3.3 | this commit; test-db `a2aef36a` | 83 claiming samples' expectations; `bin/suites/pdfx.sh` reads each file's level |
| 3.4 | this commit | README (generated table, the levels), changelog, X5-Q9 superseded (2.5) |
| 0.3 | test-db `156acfd5` | 88 claims in test-db, pdfToolbox passes 47; `docs/pdfx/x1a3-claims.csv` |

## Register coverage

Task 2.4: the last 7; no `todo` row is left. Task 2.3: 7 more. Task 2.2: 3 more. Task 2.1: 14 more implemented. Task 1.3: 12 new rows implemented. Task 1.2: 47 `x4.*` rows reused (45 implemented, 2 skip), plus the structure checks. Task 1.1: the 42 new ids (the register's 37 new rows, with the output-intent and ICC version rows split, optional
content and 16-bit images from L-Q4, and the X-1a:2001 embedded-file warning) are in `checks.toml` as `todo`; task 1.2 added `x3.output-intent.registry-name-color-org-2003` (Q15).

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-27, phase 3 (100% lines, regions, functions) |
| `bin/docker/test.sh` | green at the end of phase 2, 2026-11-27: 965 passed |
| `bin/suites/pdfx.sh` | phase 3, 2026-11-27: 122 files (39 X-4, 83 X-1a/X-3), 122 accepted, 0 problems |
| Python (ruff, pyright, pytest) | green, 2026-11-27, phase 3: 966 passed |

## Corpus and oracle

Task 0.3, 2026-11-27: of 44,608 distinct test-db PDFs (excluding `unsafe-docs-sample/`), 88 claim one of the four
levels in Info (none in XMP alone): 54 X-1a:2001, 1 plain X-1:2001, 27 X-3:2002, 3 X-3:2003, **0 X-1a:2003**, 3 with
mismatched or bare values. pdfToolbox 17.1's stock profiles pass 32 X-1a:2001, 13 X-3:2002, 1 X-3:2003 and one Ghent
file claiming X-3:2002 with an X-1a conformance key; it crashed on one file. The stock profiles judge by the 2003 edition
and accept files valid at the earlier edition they claim, so they do not separate editions. All 47 passing files are
PDF 1.3 with classic cross-reference tables. Detail in test-db `docs/pdfx/x1a3-register.md` §6.

Running the checks written so far over those 47 (task 2.2): all are detected at their claimed level; two fail
`x13.output-intent.dest-profile-by-icc-name`, both without an embedded profile under `RegistryName
http://www.color.org` but with an identifier that is no ICC reference name (an X-1a:2001 file naming "Coated FOGRA27
(ISO 12647-2:2004)", an X-3:2002 file naming "X-3"). By 15930-1 and -3 6.x a profile is then required; pdfToolbox
judges by the 2003 rule, where the registry name alone suffices. Classified in task 3.3 with the oracle: the files do
not conform to the part they claim.

Task 3.2, 2026-11-27, pdfToolbox 17.1 ("Verify compliance with PDFX-1a" / "PDFX-3") over the 62 fixtures at the older
levels: both tools pass the four bases. One fixture of ours carried a second defect (a CalRGB without `/WhitePoint`,
which pdfToolbox rightly called invalid): fixed. pdfToolbox passes 13 fixtures we fail or warn on: 5 warnings
(recommendations; its stock profiles raise none) and 8 errors, every one a 2001/2002-only rule or one pdfToolbox does
not check (RegistryName with an ICC name, AToB1Tag, colorimetric data without a profile at X-3:2002, a version 4
output profile, an RGB X-1a condition, the RegistryName spellings, 16-bit samples, an unparseable ModDate). It fails 4
we pass or warn on: a page transparency group (Q19), always-on optional content and a font licence restricting
embedding (our warnings), and plain PDF/X-1 (ours: not evaluated, so also a fail). The pdfx-oracle upheld all twelve
questions put to it, none escalated.

Task 3.3: over the 85 samples claiming the older levels (two more are refused: one encrypted, one pdfToolbox crashed on)
our verdict agrees with pdfToolbox's on 83; the two above differ. Their expectations are in test-db
`expectations/pdfx.toml`.

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-27 (one line for task 0.2). The pdfx-oracle was consulted once
on all questions, without our proposed answers; it agreed with the plan's defaults for L-Q1, Q2, Q5 (refined: only the
modification date is compared, as a warning, at the 2003 levels), Q8, Q9, Q10 and with our proposals for Q12-Q16,
Q18-Q21. It went beyond the defaults on L-Q4 (optional content: fail when it can hide print content, warn when always
on; 16-bit samples fail) and on Q11 (ICC version 4 fails, which we had proposed). It escalated L-Q3, Q17 and Q22 at
X-1a:2001 (stop questions).

## Deviations from the plan

(none yet)

## Owner actions

- Push the commits on `dev/crabify` (proofpress) and `main` (test-db) when ready; this run pushed nothing.
- pdfToolbox's trial licence expires 2026-12-09; the comparison of phase 3 is done, so only a later re-run needs it.
- Optional: an X-1a:2003 sample from a real job (none in test-db claims that level; only our built fixtures test it).

## Environment notes

- The document reads only what is reachable from the trailer: a unit test's objects must be referenced (from a page's
  resources, say) or no check sees them.
- Coverage counts the library built for unit tests apart from the one the BDD and completeness tests use: a branch
  only a fixture reaches still needs a unit test, or the gate reports missing lines that LCOV shows as covered.
- The checkout is shared with other sessions; this run commits its own files by path only (`git commit -- <paths>`)
  and checks `git diff` on each before staging.

## Stage reports

### Phase 3 (test material and comparison), finished 2026-11-27

- Fixtures: four positives and 62 older-level fixtures in all, each negative with one defect (one corrected after
  pdfToolbox showed a second); `expected.toml` and seven feature files cover them.
- pdfToolbox and the corpus: see "Corpus and oracle". 83 of 85 verdicts agree; the rest, and every fixture disagreement,
  classified with the oracle.
- `bin/suites/pdfx.sh` now runs each expected file at its own level: 122 files, 0 problems.
- Gates: `bin/rust/check.sh` green at 100%; Python 966 passed; Docker green at the end of phase 2 (965) and re-run after
  this phase (see Gates).
- Departures: none beyond those logged in the decision log.

### Phase 2 (the new rules), finished 2026-11-27

- 2.1 colour and output intent (14 checks), 2.2 transparency (3), 2.3 structure, filters, 16-bit images and optional
  content (7), 2.4 the rest (7), 2.5 X-5 targets at the older levels. Every register row is `implemented` or `skip`.
- 47 fixtures, each one defect, five feature files; every check has a pass and a fail fixture.
- Gates: `bin/rust/check.sh` green at 100% after each task; Python green; Docker green (965); `bin/suites/pdfx.sh`
  39/39 before the new expectations.
- Corpus preview: the 47 pdfToolbox positives raise errors only on the two files classified in 3.3.
- Departures: the hybrid `/XRefStm` alone is not judged (decision log); inline images are not judged by the filter and
  depth rows.

### Phase 1 (levels and identification), finished 2026-11-27

- The four levels exist everywhere a level is named (`checks.toml`, `--pdfx`, `Profile.pdfx_level`, `_core.verify`,
  the BDD steps), and without `--pdfx` a file claiming one is verified at it; the report names its level.
- 47 `x4.*` checks and the 21 structure checks apply at the older levels, citing the older parts' clauses through
  `checks.toml`'s `cite`; X-4 and X-5 expectations are unchanged.
- Info identification, dates, title and Trapped, and the TrapNet rules: 12 checks, the four bases (x4-good identified
  at each level, without XMP) and 13 negatives. Phase 1's done criterion holds: each base raises nothing at its level
  beyond the rows still `todo` (reported as not evaluated).
- Gates: `bin/rust/check.sh` green at 100%; Python green (966); Docker green (965); `bin/suites/pdfx.sh` 39/39.
- Corpus preview (not phase 3's run): the 47 pdfToolbox-passed claiming samples are all detected at their claimed
  level and raise no error from the checks written so far.
- Departures: level names hyphenated (`x1a-2001`); the new rows went into `checks.toml` as `todo` in 1.1 so the levels
  cannot pass early; `x3.output-intent.registry-name-color-org-2003` added for Q15.

### Phase 0 (decisions and groundwork), finished 2026-11-27 (after a stop)

- The register (test-db `docs/pdfx/x1a3-register.md`): 85 file-rule rows across the four parts, 48 mapped to existing
  checks (161 row-level pairs), 37 new (100 pairs); edition quirks (15930-1's misprinted conformance value, the
  RegistryName slash in 15930-6, 15930-1 Table A.1's wrong references); reader rules; 22 questions.
- Decisions: see above. The PDF 1.3 operator list matches the 73 operators `x4.content.known-operators` already uses,
  so that check is reused as is.
- Test material: 88 claiming samples, 47 pdfToolbox positives (none at X-1a:2003, one at X-3:2003); X-1a:2003 and
  most X-3:2003 positives will be built fixtures.
- Gates: no code changed; test-db `bin/check.sh` green.
- Departures from the plan: none, except the stop.
