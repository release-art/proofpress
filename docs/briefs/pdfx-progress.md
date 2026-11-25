# PDF/X plan: progress

The running log of the unattended run that carries out [`pdfx-plan.md`](pdfx-plan.md), started 2026-11-25 with the prompt in
[`pdfx-run.md`](pdfx-run.md). A fresh session resumes from **Status**.

## Stop questions

**Open, 2026-11-25: may both Altona files leave the plan's set of PDF/X-4 positives for one rule?**

- *What happened.* Fixing `colour.py` (task 0.5) to the real 6.4.3.3 rule made both Altona technical2 X-4 files fail it.
  Each has a `DefaultCMYK` that is the output intent's own profile (same ICC.1 7.2.18 MD5), in two nested forms that paint
  a DeviceCMYK image, so by 6.4.3.1 those images are ICCBased with the intent's profile. pdfToolbox 17.1 ("Verify
  compliance with PDFX-4") reports exactly this, twice per file, and nothing else. The pdfx-oracle confirmed it from the
  clause, 6.4.3.1 and ISO 32000-1 8.6.5.6, with high confidence, and asked that the owner approve the change to the plan,
  whose "done" criteria (what feature-complete means, item 2, and phase 3) expect both Altona files to pass X-4.
- *Options.* 1. Record them as classification (b), "the file does not conform": both are expected to raise only
  `x4.icc.cmyk-not-intent-profile`, two instances each, and stay positives for every other row; edit the plan's done
  criteria to say so. 2. Keep them as full positives and treat a DefaultCMYK as exempt from 6.4.3.3, against the text and
  pdfToolbox.
- *Recommendation:* option 1.
- *What I do once answered:* log the decision, edit the plan's done criteria, then carry on with the brief's bug 4 (total
  ink through `tiffsep`) and the end-of-phase-0 gates.

## Status

- **Phase 0, stopped** on the question above. Task 0.5 and the brief's bugs 1-3 are done; bug 4 is next.

## Done

| Task | Commit | Note |
|---|---|---|
| 0.1 | (owner) | 34 interpretation defaults accepted |
| 0.2, 0.3 | `c01b3fa`; test-db `3a1b8d0a` | PDF 1.6 Reference and errata; suites README; the letter to GWG is the owner's |
| 0.4 | `222b6c2` | `x4-good` built; the oracle and pdfToolbox 17.1 ("Verify compliance with PDFX-4": 0 errors, 0 warnings) both pass it |
| 0.5 | `7a53c78`, `8f68de5`, `112d0ba`, `39af9a3` | Bugs 1, 2; `trapped` in XMP; `pdf-version` deleted; 6.4.3.3 identical-profile rule |
| Brief bug 3 | `dded7d0` | `xobject-missing`: a `Do` on a missing XObject fails |

## Register coverage

`checks.toml` does not exist yet (task 1.1).

## Gates

| Gate | Last result |
|---|---|
| `bin/rust/check.sh` | green, 2026-11-25 |
| Python (ruff, pyright, pytest) | green, 2026-11-25: 1151 passed |
| `bin/docker/test.sh` | not run yet (end of phase 0) |

## Decisions taken

Decision log in [`../README.md`](../README.md), 2026-11-25:
- PDF/X 6.25 is the PDF 1.6 Reference's Table C.1 only: no array or dictionary limit, no page size (task 0.2).
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
  image); high confidence; escalated because the plan lists Altona as a positive (stop question).
- Task 0.4, `x4-good` against every X-4 row: conforms, high confidence; escalated the ICC version reading, which
  pdfToolbox then settled in line with the oracle's suggestion.
  Its aside (font `name` table lacked IDs 3-6, `FontBBox` did not match the glyphs) was fixed before commit.

## Deviations from the plan

- Task 2.2's 6.25 line named array and dictionary limits; Table C.1 of PDF 1.6 has none, so the plan text was corrected.
- Task 0.4 needed a real embedded font; `fonttools` became a development dependency to build one, rather than vendoring a
  third-party font.
- Phase 0 also carries the brief's four stage-0 bugs (plan 0.5 says "on top of"); bugs 1-3 are fixed, 4 remains.
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
