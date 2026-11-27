# PDF/X-1a and X-3 plan: progress

The running log of the unattended run that carries out [`pdfx-1a-3-plan.md`](pdfx-1a-3-plan.md), started 2026-11-27 with
the prompt in [`pdfx-1a-3-run.md`](pdfx-1a-3-run.md). A fresh session resumes from **Status**.

## Stop questions

**Open, 2026-11-27 (end of task 0.2): two of the plan's re-plan points triggered.**

1. **The oracle escalated L-Q3** (transparency in X-1a:2001 and X-3:2002). Neither part has a transparency clause.
   15930-3's informative Annex D says a file with transparency keys is still X-3 compliant, yet recommends that
   verifiers treat non-null transparency as needing correction; the 2003 parts' clause 5 lets 2003 readers accept the
   older files only when they meet 6.16; pdfToolbox fails them. Options: (a) **fail** (error) at both levels, citing
   clause 5 and the data-structure clause, plus Annex D at X-3:2002; (b) **warn** at both; (c) fail at X-3:2002 (Annex D
   asks for it) and warn at X-1a:2001. Recommendation: (a), so a file a 2003 reader may refuse and pdfToolbox fails is
   not passed. What counts as transparency is Annex D.2's list (ExtGState `BM`, `CA`, `ca`, `SMask`, image `SMask`);
   form groups alone are not judged at these two levels.
2. **The register has more new rows than the plan assumed:** 37 of 85 file-rule rows (44 %; 38 % counted per level)
   are new rather than mapped to `x4.*` checks, against the stop line of a third. Most are small: Info keys
   (identification, dates, title, Trapped: 10 rows on one helper), conditional output-intent rows (8), variants of
   existing geometry and graphics-state checks (5), filter lists (4), transparency key bans (3). The colour rows
   (`x1a.colour.print-spaces`, `x3.output-intent.dest-profile-for-colour`) reuse the existing colour walk. Options:
   (a) **carry on with the plan as written**, accepting about a third more work in phase 2 (by judgement, 3–4 days of
   agent time instead of 2–3); (b) re-plan, e.g. drop X-1a:2003 (no sample claims it) or the 2001/2002 editions.
   Recommendation: (a).
3. Lower priority, can be answered later: **Q17**, must a font used only in render mode 3 be embedded (we propose no,
   as X-4 says; the oracle agrees as a judgement); **Q22 at X-1a:2001**, an embedded file (attachment) in an X-1a:2001
   file: the oracle would warn.

Once answered, the run continues with phase 1 (task 1.1), carrying the answers into the decision log and the register.

## Status

- **Stopped at the end of phase 0** for the stop questions above. Tasks 0.1, 0.2 (all but the escalated questions) and
  0.3 are done.

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | test-db `156acfd5` | `docs/pdfx/x1a3-register.md`: 85 file-rule rows, 48 mapped to `x4.*` checks, 37 new |
| 0.2 | this commit; test-db register §4 | L-Q1, Q2, Q4-Q10 and Q11-Q22 settled with the oracle (decision log); L-Q3, Q17 and Q22 at X-1a:2001 escalated |
| 0.3 | test-db `156acfd5` | 88 claims in test-db, pdfToolbox passes 47; `docs/pdfx/x1a3-claims.csv` |

## Register coverage

Not started (phase 1).

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | not run yet in this plan |

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

- The checkout is shared with other sessions; this run commits its own files by path only (`git commit -- <paths>`)
  and checks `git diff` on each before staging.

## Stage reports

### Phase 0 (decisions and groundwork), stopped 2026-11-27

- The register (test-db `docs/pdfx/x1a3-register.md`): 85 file-rule rows across the four parts, 48 mapped to existing
  checks (161 row-level pairs), 37 new (100 pairs); edition quirks (15930-1's misprinted conformance value, the
  RegistryName slash in 15930-6, 15930-1 Table A.1's wrong references); reader rules; 22 questions.
- Decisions: see above. The PDF 1.3 operator list matches the 73 operators `x4.content.known-operators` already uses,
  so that check is reused as is.
- Test material: 88 claiming samples, 47 pdfToolbox positives (none at X-1a:2003, one at X-3:2003); X-1a:2003 and
  most X-3:2003 positives will be built fixtures.
- Gates: no code changed; test-db `bin/check.sh` green.
- Departures from the plan: none, except the stop.
