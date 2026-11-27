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

- Phase 0 is done (the stop questions are answered). Phase 1 is in progress, from task 1.1.

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | test-db `156acfd5` | `docs/pdfx/x1a3-register.md`: 85 file-rule rows, 48 mapped to `x4.*` checks, 37 new |
| 0.2 | this commit; test-db register §4 | L-Q1, Q2, Q4-Q10 and Q11-Q22 settled with the oracle (decision log); L-Q3, Q17 and Q22 at X-1a:2001 escalated, then answered by the owner |
| 1.1 | this commit | Levels `x1a-2001`, `x3-2002`, `x1a-2003`, `x3-2003`; detection of a claimed older level; the report's `level`; the 42 new rows as `todo`; clause lists for 15930-1, -3, -4, -6 |
| 1.2 | this commit | 47 `x4.*` checks and the 21 structure checks applied at the older levels; `cite` maps their clauses; X-4 and X-5 expectations unchanged |
| 1.3 | this commit | Info identification, dates, title and Trapped, the TrapNet rules (12 checks, `checks/info.rs`); bases `x1a-2001-good`, `x3-2002-good`, `x1a-2003-good`, `x3-2003-good` and 13 negatives; scenarios in `x13-identification-trapping.feature` |
| 0.3 | test-db `156acfd5` | 88 claims in test-db, pdfToolbox passes 47; `docs/pdfx/x1a3-claims.csv` |

## Register coverage

Task 1.3: 12 new rows implemented. Task 1.2: 47 `x4.*` rows reused (45 implemented, 2 skip), plus the structure checks. Task 1.1: the 42 new ids (the register's 37 new rows, with the output-intent and ICC version rows split, optional
content and 16-bit images from L-Q4, and the X-1a:2001 embedded-file warning) are in `checks.toml` as `todo`; task 1.2 added `x3.output-intent.registry-name-color-org-2003` (Q15).

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-27, task 1.3 (100% lines, regions, functions) |
| Python (ruff, pyright, pytest) | green, 2026-11-27, task 1.3: 966 passed |

## Corpus and oracle

Task 0.3, 2026-11-27: of 44,608 distinct test-db PDFs (excluding `unsafe-docs-sample/`), 88 claim one of the four
levels in Info (none in XMP alone): 54 X-1a:2001, 1 plain X-1:2001, 27 X-3:2002, 3 X-3:2003, **0 X-1a:2003**, 3 with
mismatched or bare values. pdfToolbox 17.1's stock profiles pass 32 X-1a:2001, 13 X-3:2002, 1 X-3:2003 and one Ghent
file claiming X-3:2002 with an X-1a conformance key; it crashed on one file. The stock profiles judge by the 2003 edition
and accept files valid at the earlier edition they claim, so they do not separate editions. All 47 passing files are
PDF 1.3 with classic cross-reference tables. Detail in test-db `docs/pdfx/x1a3-register.md` §6.

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

- pdfToolbox's trial licence expires 2026-12-09; phase 3's comparison needs it.

## Environment notes

- Coverage counts the library built for unit tests apart from the one the BDD and completeness tests use: a branch
  only a fixture reaches still needs a unit test, or the gate reports missing lines that LCOV shows as covered.
- The checkout is shared with other sessions; this run commits its own files by path only (`git commit -- <paths>`)
  and checks `git diff` on each before staging.

## Stage reports

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
