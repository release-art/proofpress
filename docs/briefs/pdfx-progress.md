# PDF/X plan: progress

The running log of the unattended run that carries out [`pdfx-plan.md`](pdfx-plan.md), started 2026-11-25 with the prompt in
[`pdfx-run.md`](pdfx-run.md). A fresh session resumes from **Status**.

## Stop questions

None open. Answered 2026-11-25: the ICC version reading (by pdfToolbox's verdicts) and Altona under 6.4.3.3 (owner:
record it as not conforming to that one rule).

## Status

- **Phase 1**: tasks 1.1-1.4 done. Next: 1.5, the fixture and scenario harness (a scenario on `x4-good`), then 1.6 and 1.7.

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

## Register coverage

From `cargo test --test completeness -- --nocapture` (implemented / todo / skip):

| Group | Implemented | Todo | Skip |
|---|---|---|---|
| PDF/X-4 and X-4p | 0 | 138 | 3 |
| PDF/X-5 | 0 | 35 | 18 |
| House print checks | 0 | 37 | 0 |

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-25 |
| Python (ruff, pyright, pytest) | green, 2026-11-25: 1154 passed |
| `bin/docker/test.sh` | green at the end of phase 0 (`766fcef`), 2026-11-25: 1154 passed with real veraPDF and Ghostscript |
| `bin/suites/pdfx.sh` | does not exist yet (task 1.6) |

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-25:
- PDF/X 6.25 is the PDF 1.6 Reference's Table C.1 only: no array or dictionary limit, no page size (task 0.2).
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

Oracle consultations:
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
- The Python 6.4.3.3 rule judges every reachable ICCBased space, not only painted ones (X4-Q3); marked `ponytail:` in
  `colour.py`, for the Rust colour evaluator to do properly.

## Owner actions

- **0.3:** write to admin@gwg.org asking to use the Ghent Output Suite and the Processing Steps suite in public CI.
- **Before phase 8:** get ISO 15930-8:2010/Cor 1:2011 (one page; ISO corrigenda are usually free, but iso.org refuses
  scripts). Not blocking until then.
- **Later, X5-Q9:** ISO 15930-1, -3, -4 and -6 are listed in test-db's `docs/standards-wanted.md`; not to buy now.

## Environment notes

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

