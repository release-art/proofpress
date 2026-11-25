# PDF/X plan: progress

The running log of the unattended run that carries out [`pdfx-plan.md`](pdfx-plan.md), started 2026-11-25 with the prompt in
[`pdfx-run.md`](pdfx-run.md). A fresh session resumes from **Status**.

## Stop questions

**Open, 2026-11-25: how do 6.4.2.1 and 6.4.3.3 read an ICC profile's version?** (rows `x4.output-intent.profile-version`
and `x4.icc.profile-version`; ISO 15930-7, 6.4.2.1 p.19 and 6.4.3.3 p.20, page markers of the Markdown copy)

- *What happened.* The pdfx-oracle reviewed `x4-good` (task 0.4), found no defect, and escalated one point. Both clauses
  list four specifications: ICC.1:1998-09 (profile version 2.2), ICC.1:2001-12 (4.0), ICC.1:2003-09 (4.1) and ISO
  15076-1:2005 (4.2). Their notes say the PDF Reference allows every version up to ICC.1:2003-09, and X-4 adds 15076-1:2005.
  Read as an exact list, real positives fail: the Ghent X-4 files' profiles are 2.4.0, Altona's 2.0.0, and `x4-good`'s
  Little-CMS profile 2.1.0, yet four of those Ghent files carry Acrobat Preflight's "PDF/X-4 compliant" record. None of the
  34 decisions covers it.
- *Options.*
  1. **Any version up to 4.2** (2.x, 4.0, 4.1, 4.2 pass; 4.3, 4.4 and 5.x fail). Follows the notes' "up to" and the list's
     newest entry. Risk: Little-CMS writes 4.3 by default, so some real files would fail.
  2. **Major version 2 or 4, any minor** (the oracle's suggestion). Passes every positive in reach, including 4.3 and 4.4,
     which the clause does not name.
  3. **The exact four versions.** Fails Ghent, Altona and `x4-good`; contradicted by the Acrobat records.
- *Recommendation:* option 1. It reads the clause and its notes together, passes every known positive, and fails only
  profiles newer than any edition the standard names. If a real job with a 4.3 profile turns up, revisit.
- *What I do once answered:* log the decision, note it in the register-derived `checks.toml` entries when task 1.1 creates
  them, and carry on with task 0.5 (`check_trapped` in XMP, the inverted CMYK-profile rule, deleting `pdf-version`), then
  the brief's bugs 3 and 4.

## Status

- **Phase 0, stopped** on the question above. Next: the rest of task 0.5 (bugs 1 and 2 are done).

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | (owner) | 34 interpretation defaults accepted |
| 0.2, 0.3 | `c01b3fa`; test-db `3a1b8d0a` | PDF 1.6 Reference and errata; suites README; the letter to GWG is the owner's |
| 0.4 | `222b6c2` | `x4-good` built; oracle found no defect, escalated the ICC version reading (stop above) |
| 0.5, part | `7a53c78` | The brief's bugs 1 (password crash) and 2 (K-only JPEG `/Decode`) fixed, with tests |

## Register coverage

`checks.toml` does not exist yet (task 1.1).

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-25 |
| Python (ruff, pyright, pytest) | green, 2026-11-25: 1144 passed |
| `bin/docker/test.sh` | not run yet (end of phase 0) |

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-25:
- PDF/X 6.25 is the PDF 1.6 Reference's Table C.1 only: no array or dictionary limit, no page size (task 0.2).
- X4-Q6 stands: no corrigendum to ISO 15930-7:2010 found; 15930-8 Cor 1:2011 exists and is not held (task 0.2).

Oracle consultations:
- Task 0.4, `x4-good` against every X-4 row: conforms, high confidence; escalated the ICC version reading (stop question).
  Its aside (font `name` table lacked IDs 3-6, `FontBBox` did not match the glyphs) was fixed before commit.

## Deviations from the plan

- Task 2.2's 6.25 line named array and dictionary limits; Table C.1 of PDF 1.6 has none, so the plan text was corrected.
- Task 0.4 needed a real embedded font; `fonttools` became a development dependency to build one, rather than vendoring a
  third-party font.
- Phase 0 also carries the brief's four stage-0 bugs (plan 0.5 says "on top of"); bugs 1 and 2 were still open and are
  fixed; 3 and 4 remain.

## Owner actions

- **0.3:** write to admin@gwg.org asking to use the Ghent Output Suite and the Processing Steps suite in public CI.
- **0.4:** preflight `tests/fixtures/pdfs/x4-good.pdf` once in Acrobat Preflight or pdfToolbox ("PDF/X-4 compliant") and record
  the result here. It would also settle the stop question for this file, whose profile is version 2.1.
- **Before phase 8:** get ISO 15930-8:2010/Cor 1:2011 (one page; ISO corrigenda are usually free, but iso.org refuses
  scripts). Not blocking until then.
- **Later, X5-Q9:** ISO 15930-1, -3, -4 and -6 are listed in test-db's `docs/standards-wanted.md`; not to buy now.

## Environment notes

- `cargo` is not on the default `PATH`: Homebrew's rustup keeps its proxies in `/opt/homebrew/opt/rustup/bin`.
- `qpdf` 12.4.2 installed by the owner on 2026-11-25.
