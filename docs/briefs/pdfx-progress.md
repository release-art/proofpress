# PDF/X plan: progress

The running log of the unattended run that carries out [`pdfx-plan.md`](pdfx-plan.md), started 2026-11-25 with the prompt in
[`pdfx-run.md`](pdfx-run.md). A fresh session resumes from **Status**.

## Stop questions

None open.

## Status

- **Phase 0**, task 0.4 next (a genuine PDF/X-4 base fixture).

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | (owner) | 34 interpretation defaults accepted |
| 0.2 | this commit; test-db `3a1b8d0a` | PDF 1.6 Reference and its errata in test-db; standards wanted for X-5 |
| 0.3 | this commit | Suites README updated; the letter to GWG is the owner's |

## Register coverage

`checks.toml` does not exist yet (task 1.1).

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-25 (needs `/opt/homebrew/opt/rustup/bin` on `PATH`: Homebrew's rustup does not link `cargo`) |
| Python (ruff, pyright, pytest) | not run yet: no Python changed |
| `bin/docker/test.sh` | not run yet (end of phase 0) |

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-25:
- PDF/X 6.25 is the PDF 1.6 Reference's Table C.1 only: no array or dictionary limit, no page size (task 0.2).
- X4-Q6 stands: no corrigendum to ISO 15930-7:2010 found; 15930-8 Cor 1:2011 exists and is not held (task 0.2).

## Deviations from the plan

- Task 2.2's 6.25 line named array and dictionary limits; Table C.1 of PDF 1.6 has none, so the plan text was corrected.

## Owner actions

- **0.3:** write to admin@gwg.org asking to use the Ghent Output Suite and the Processing Steps suite in public CI.
- **0.4:** preflight `x4-good` once in Acrobat Preflight or pdfToolbox and record the result (once it exists).
- **Before phase 8:** get ISO 15930-8:2010/Cor 1:2011 (one page; ISO corrigenda are usually free, but iso.org refuses
  scripts). Not blocking until then.
- **Later, X5-Q9:** ISO 15930-1, -3, -4 and -6 are listed in test-db's `docs/standards-wanted.md`; not to buy now.

## Environment notes

- `qpdf` is not installed, though the run prompt lists it. Nothing needs it before phase 2's oracle runs
  (`spikes/crabify/oracle.py`); if it is still missing then, that is a stop (`brew install qpdf`).
