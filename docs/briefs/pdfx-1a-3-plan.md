# Plan: PDF/X-1a and PDF/X-3

You are working in `/Users/ilo/Devel/Release.art/proofpress`, on the Rust verifier that [`pdfx-plan.md`](pdfx-plan.md) built.
This plan adds the four older PDF/X conformance levels that print shops still receive most:

| Level | Standard | Built on |
|---|---|---|
| PDF/X-1a:2001 | ISO 15930-1:2001 | PDF 1.3 |
| PDF/X-3:2002 | ISO 15930-3:2002 | PDF 1.3 |
| PDF/X-1a:2003 | ISO 15930-4:2003 | PDF 1.4 |
| PDF/X-3:2003 | ISO 15930-6:2003 | PDF 1.4 |

Written 2026-11-27. Start it after `pdfx-plan.md` phase 10 (the release) is done.

Read first:
- [`pdfx-plan.md`](pdfx-plan.md): its working rules apply here unchanged (one path per requirement, expected results as
  data, one defect per negative fixture, disputed files classified, never silenced).
- [`pdfx-progress.md`](pdfx-progress.md): the decisions taken while building X-4 and X-5, many of which carry over.
- [`../notes/rust-architecture.md`](../notes/rust-architecture.md), and `CONTRIBUTING.md` for the gates.

The standards are in `proof-pdf-test-db/standards/iso/` as CGATS editions (`iso-15930-{1,3,4,6}-*-cgats.md`), with the
PDF References they build on in `standards/adobe/` (1.3, 1.4 and the 1.4 errata of 2003-06-18). **Never copy a standard's
text, or a register's paraphrase of it, into this repository**; clause and page numbers are fine.

## Why this is smaller than X-4

All four parts share one skeleton with X-4 (15930-7 descends from them): pre-separation, completeness, the output intent,
fonts, trapping, identification, page boxes, encryption, alternate images, annotations, actions, `BX`/`EX`. Most `x4.*`
checks test a property these parts also restrict, so the bulk of the work is mapping, not writing.

What is genuinely new:

- **Identification and trapping in the Info dictionary**, not XMP: the version (and, for 2001, the conformance) key, the
  Info dates and title, and Info `/Trapped`, where `Unknown` is banned.
- **Colour per level.** X-1a allows only CMYK, gray and spot colour in print elements; X-3 adds RGB and calibrated colour
  under its own rules (15930-3 6.2, 15930-6 6.2).
- **Transparency.** The 2003 parts ban it outright (6.16); the 2001/2002 parts predate it.
- **Filters.** 15930-1 6.8 allows a fixed list; the 2003 parts ban two (6.5).
- **Smaller rules:** viewer preferences (6.17 in the 2003 parts), digital signatures (15930-3 6.16), `TrimBox` *or*
  `ArtBox`, the `TrapNet` details.

There is **no general rule against features newer than PDF 1.3 or 1.4**. Each part's clause 5 lets a file carry other
valid features that don't affect print, and forbids judging by the header or catalog version. What newer features may not
do is set by the specific clauses above, plus the questions in task 0.1.

## Phase 0: decisions and groundwork

**0.1 A requirement register.** Write `proof-pdf-test-db/docs/pdfx/x1a3-register.md` (private, beside `x4-register.md`):
- one row per requirement of each part, with clause, page, and whether it is a file, reader or writer rule;
- for each file rule, the `x4.*` check that already tests the same property, or "new";
- a column per level, since most rules appear in all four parts with small differences;
- the open questions below, with any the reading adds.

Note the edition quirks in the register: the CGATS 15930-4 is a "national adoption", not marked identical, and its
introduction carries an extra paragraph; the CGATS 15930-1 pages are stamped with a 2017 edition label. Where a rule's
wording looks different from what other tools enforce, check it with the oracle before writing the check.

**0.2 Settle the interpretation questions**, with the oracle, and log each in the decision log. Proposed defaults:

| # | Topic (clauses) | Proposed default |
|---|---|---|
| L-Q1 | Plain PDF/X-1 (15930-1, which allows OPI references) | Out of scope: a file claiming PDF/X-1 but not X-1a is "not evaluated" |
| L-Q2 | Cross-reference and object streams (PDF 1.5) in a file whose data structure clause names a cross-reference table (15930-1 6.2, the others 6.1) | Ask the oracle first; proposed: an error, since a PDF 1.3/1.4 reader cannot find the objects |
| L-Q3 | Transparency in X-1a:2001 and X-3:2002, which have no transparency clause | Judged like the 2003 rule (a 1.3 reader would print it wrongly, so it affects reproduction); cite clause 5 and the data structure clause |
| L-Q4 | Other post-1.4 features that do affect print: JPEG 2000, JBIG2, optional content, 16-bit images | Judged per feature by the filter and colour clauses where one applies; otherwise ask the oracle. No blanket version check |
| L-Q5 | XMP in these files (2003 parts, Annex B is informative) | Not required; when present and it disagrees with Info, a warning |
| L-Q6 | Which `x4.*` checks are reused | A check is reused only when the property *and* the test are the same; its `applies` gains the new levels and its `clauses` the new citations. Anything that differs gets a new id |
| L-Q7 | New check id prefixes | `x1a.*` for X-1a only, `x3.*` for X-3 only, `x13.*` for rules shared by all four that differ from X-4 |
| L-Q8 | A file claiming X-1a:2001 checked at X-1a:2003 (15930-4 clause 5 asks readers to accept it) | Each file is judged by the part it claims; the 2003 reader clause is a reader rule, `skip` |
| L-Q9 | Pre-separation, print versus non-print scope, actions, spot names | X4-Q2, Q3, Q11 and Q20 carry over unchanged |
| L-Q10 | X-5 targets at these levels (X5-Q9) | Evaluated at the level they claim, now that the standards are held; update the decision of 2026-11-26 |

**0.3 Find the test material.** Count test-db samples that claim each level (Info `/GTS_PDFXVersion`), list them in
`x1a3-register.md`'s test-material section, and pick the positives: pdfToolbox 17's "Verify compliance with PDFX-1a" and
"PDFX-3" profiles give a third-party verdict for each. Ghent's older suites and GWG files are the likeliest clean
positives.

*Done when* the register exists, every question has a logged decision, and the corpus counts are recorded. Judgement:
3–5 days.

## Phase 1: levels and identification

**1.1 The levels.** Add `x1a-2001`, `x3-2002`, `x1a-2003` and `x3-2003` to `Level`, `checks.toml`'s `applies`, `--pdfx` and the
Python API. Auto-detection from Info picks the level when `--pdfx` is not given.

**1.2 Map the inherited checks.** For every register row marked as an existing `x4.*` check, widen `applies` and
`clauses`. Run the whole suite: every X-4 and X-5 expectation must be unchanged.

**1.3 Identification, dates, title, `/ID`, Trapped**, in Info, per level, including the `TrapNet` rules.

*Done when* `x4-good` relabelled as each level reports only the findings the register predicts. Judgement: 1 week.

## Phase 2: the new rules

**2.1 Colour per level:** the X-1a print-element colour rule, the X-3 RGB, `ICCBased` and calibrated rules, and the output
intent's own requirements for each part (the X-4 output-intent checks cover most of it; differences get new ids).

**2.2 Transparency** (6.16 in the 2003 parts, and L-Q3).

**2.3 Filters** (15930-1 6.8, 15930-3 6.4, the 2003 parts 6.5), and L-Q2 and L-Q4.

**2.4 The rest:** file specifications, viewer preferences, digital signatures, page boxes (`TrimBox` or `ArtBox`, not
both), alternate images, `PS` operator and PostScript XObjects, encryption, annotations, actions, `BX`/`EX` — each either
mapped in 1.2 or a new check here.

**2.5 X-5 targets at these levels** (L-Q10): `targets.rs` verifies them at their claimed level instead of "not evaluated".

*Done when* every register row is `implemented` or `skip`. Judgement: 2–3 weeks.

## Phase 3: test material and comparison

**3.1 Fixtures.** One positive per level, built from `x4-good` in `tests/fixtures/build_pdfs.py`, and one negative per new
check, each with one defect, in `tests/fixtures/pdfx/expected.toml`.

**3.2 pdfToolbox comparison** over the fixtures and the positives from 0.3, as was done for X-5. Every disagreement is
classified (our bug, file doesn't conform, decided ambiguity) and the uncertain ones go to the oracle.

**3.3 Corpus run** over test-db's files that claim these levels. Expectations for private files go in
`proof-pdf-test-db/expectations/pdfx.toml`.

**3.4 Docs:** the README check table (generated), the changelog, and the X5-Q9 decision updated.

*Done when* the fixtures pass, pdfToolbox disagreements are all classified, and the gates are green. Judgement: 1–2 weeks.

## Summary

| Phase | Judgement |
|---|---|
| 0 Register, decisions, test material | 3–5 days |
| 1 Levels and identification | 1 week |
| 2 The new rules | 2–3 weeks |
| 3 Test material and comparison | 1–2 weeks |

About **5–7 weeks** for one engineer. For comparison, `pdfx-plan.md` judged phases 7–8 (X-4p and X-5) at 4–7 weeks, and an
unattended agent finished them in about a day; expect this plan to take an agent 2–3 days of run time.

## When to stop and re-plan

1. **After 0.2,** if the oracle escalates L-Q2 or L-Q3, or if more than a third of the register's rows turn out to be "new"
   rather than mapped: the estimate assumed most are mapped.
2. **If the CGATS 15930-4 differs from ISO in a rule** we would check: the ISO text wins, and we don't hold it.
3. **If more than a few pdfToolbox-passed corpus positives fail one check:** our reading is probably stricter than
   practice; take it to the oracle before changing anything.

## Risks

| Risk | Signal | Response |
|---|---|---|
| Real-world X-1a files are often slightly off, so the corpus run raises many disputes | A long list of failures in 3.3 | Classify in batches by check; most will be one decision each |
| A reused `x4.*` check is subtly different at these levels | A level-specific expectation fails in 1.2 | Split it into a new id (L-Q6); never add a level flag inside a check |
| The national-adoption wording of 15930-4 | A rule reads differently from 15930-6 where they should match | Oracle; if it matters, buy ISO 15930-4 |
