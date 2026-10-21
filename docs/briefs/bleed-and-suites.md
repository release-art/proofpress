# Brief: bleed-coverage check and external-suite calibration

You are working in `/Users/ilo/Devel/Release.art/proofpress`, the open-source `proof-pdf` package (Python 3.15, pdm,
Apache-2.0). It verifies print PDFs: PDF/A via veraPDF, PDF/X-4 identity, geometry, colour, text and proofing checks, an
`inspect` command, a LaTeX log linter and a config file. Read `README.md`, `CONTRIBUTING.md` and `CHANGELOG.md` first, then
the code you will touch. Two pieces of work, in this order: **A** a bleed-coverage check, **B** a run of the free Ghent and
Altona test suites to find false positives.

## House rules (non-negotiable)

- **Git is the user's.** Never run `git add`, `commit`, `stash`, `push` or any other git write; read-only `git status/diff/log`
  is fine. Leave all work uncommitted in the working tree. Do not ask the user about git.
- **No invented numbers.** A threshold, margin or size floor that comes from a printer's spec is a parameter with no default.
  A tolerance that is a property of the maths (a sub-point rounding allowance) is a documented constant.
- **An unconfirmable check fails, it is never skipped.** A tool missing, a file unreadable or a measurement impossible is
  reported, not passed. Estimates are labelled as estimates.
- **Findings, not exceptions.** A malformed PDF must produce a finding or an `inspect` error, never a traceback. Any crash you
  find on an external file is a bug to fix, always.
- Match the surrounding code: comment density, naming, idiom. Lazy and small beats clever; no speculative abstractions.
- Toolchain, all must pass before you finish: `pdm run ruff format src tests`, `pdm run ruff check src tests`,
  `pdm run pyright` (0 errors), `pdm run pytest`. Then `bin/docker/test.sh` (the whole suite in the image with real veraPDF,
  Ghostscript and LuaLaTeX; Docker Hub may rate-limit pulls, then say so rather than skipping it).
- Docs move with code: update `README.md`, `CHANGELOG.md` (under `[Unreleased]`) and `CONTRIBUTING.md` where behaviour or
  workflow changes.
- Ask the user only when genuinely blocked. Otherwise pick the sensible default, state it, and continue.

## How the code is laid out (what you will reuse)

- `src/proof_pdf/paint.py`: `Walker` / `paint_events(page)` yield `Paint` events (text, filled/stroked paths, images, shadings)
  with page-space `bbox`, fill, overprint, font, stroke width, and track `q`/`Q` and Form XObject scope. **Clipping is not
  modelled yet.** `content.py` does the content-stream walk.
- `src/proof_pdf/structure.py`: page boxes, `own_box(page, "TrimBox")`, `media_box(page)`, geometry (`check_geometry`,
  `_check_bleed` already checks the *declared* bleed size, check id `bleed`; do not collide with it).
- `src/proof_pdf/core.py`: `_check_pdf` runs the in-process checks per profile; `models.Profile` holds the options;
  `cli.py` maps flags to `Profile` (config-file keys come for free from the flag names via `config.py`).
- `tests/fixtures/build_pdfs.py` builds the committed fixture PDFs deterministically (`--check` is enforced by a test).
  `tests/test_proof_pdf/test_integration.py` runs the real CLI against real tools; unit tests construct PDFs with pikepdf.
- Existing PDFs for sanity (read-only, never copy into the repo):
  `/Users/ilo/Devel/BrainyBoo/educational-cards/decks/*/*_full.pdf` (LaTeX card decks; the card package places art with a
  clip to the bleed box). Use them only to confirm the new check does not misfire on real, bleed-correct files, and to
  describe anything it flags. There is no record of what the printer accepted, so they are not expected-pass fixtures.

## Part A: the bleed-coverage check

**Problem.** `verify` checks that the declared TrimBox/BleedBox sizes are right, but not that the *artwork* actually runs to
the bleed edge. A background that stops at the trim edge passes every current check and then shows a white sliver after
cutting. This is the most likely remaining miss.

**Requirements**

1. New opt-in check, flag `--require-bleed` (off by default; config key `require-bleed`), check id `artwork-bleed`, severity
   ERROR. Add `Profile.require_bleed`, wire core/CLI, add it to the summary's layer list.
2. Scope per page that has a `TrimBox` and whose bleed edge (`BleedBox`, else `MediaBox`) is larger than the trim on at least
   one side. Pages without a TrimBox, or with no bleed zone, are skipped (other checks report those); a rotated page
   (`/Rotate` non-zero) is skipped with a documented note, not guessed at.
3. What counts: filled paths, images and shadings. Not text, not strokes. A fill in white or in the paper colour counts (a
   white background that stops at the trim leaves the same sliver).
4. Rule per side (left, right, bottom, top): an object whose visible extent reaches the trim edge on that side (within a
   documented sub-point tolerance) must extend into the bleed zone to the bleed edge (within the same tolerance). Report
   objects that reach the trim but stop short, including partial bleed ("1.2 mm of the 3 mm"). Objects inset from the trim edge
   are ignored.
5. **Clipping must be modelled**, because the real card decks clip art to a box and a check that ignores clips will
   misjudge them. Extend the walker's state with a clip region: `W`/`W*` followed by the path-ending operator intersects the
   current clip with the path's page-space extent; the clip is restored by `q`/`Q` and Form XObject scope. Rectangles are
   handled exactly; for any other clip path use its bounding extent and document that. Add a *separate* `visible` extent to
   `Paint` (the bbox intersected with the clip) and use it for this check. Keep `bbox` unchanged so existing checks do not
   shift; note in the report whether contrast background detection should move to `visible` (do not change it unasked).
6. Findings are per page, name the sides, the number of objects and the shortfall in mm, and say what to do ("artwork must
   run to the bleed edge"). Collapse sides into one finding per page.
7. Document the limits next to the code and in the README: transparency/soft masks, non-rectangular clips, and the fact that
   it judges geometry, not the printed result.

**Tests (all required)**

- Unit tests on constructed PDFs: full bleed passes; fill stops at the trim fails (each of the four sides); partial bleed
  fails with the right shortfall; inset object ignored; image whose raw footprint exceeds the bleed but is clipped back to
  the trim fails; image clipped to the bleed box passes; rectangle clip restored by `q`/`Q`; clip scoped to a Form XObject;
  non-rectangular clip uses its extent; page without TrimBox and page with no bleed zone skipped; rotated page skipped;
  strokes and text ignored; white fill counted.
- Fixtures in `tests/fixtures/build_pdfs.py` (regenerate, keep deterministic): `no-bleed` (background stops at trim) and
  a bleed-correct one (the existing `good` card already has a full-page background; confirm it passes with the flag).
- Integration tests through the real CLI, and a config-file case.
- Run the check over a sample of the real decks above; report which pass, which are flagged, and why. Do not "fix" the decks.

## Part B: calibrate against the Ghent and Altona test suites

**What they are.** Free PDF/X-4 *output-workflow* test files: the Ghent PDF Output Suite 5.0 (`https://gwg.org/gos5/`, six
test pages with 48 patches plus single patches) and the Altona Test Suite 2.0 technical page (free from ECI,
`https://eci.org/doku.php_id=en:downloads.html`, files such as `eci_altona-test-suite-v2_technical2_x4.pdf` and the
one-patch-per-page variant). They are valid PDFs built to exercise a RIP, **not** collections of defective files. Running
`proof-pdf` over them finds our false positives; it cannot find misses. Many patches are deliberately unusual (overprint,
spot inks, transparency, RGB and ICC-based elements), so a flag is not automatically a false positive.

**Requirements**

1. **Licence first.** Read the terms on each download page and in the bundled documentation, and report them verbatim in your
   notes before committing any file. The decision rule (the user chose "commit a subset"):
   - If redistribution of the files, or of selected patches, is clearly permitted: vendor a small, representative subset
     under `tests/fixtures/suites/` with a `README.md` giving source URL, version, licence text or link, retrieval date and
     sha256 of every file. Keep it small (tens of KB to a few MB), diverse across features, and skip anything large.
   - If the terms are unclear or forbid it: **do not commit the files.** Instead write a fetch script
     (`bin/suites/fetch.sh` or a small Python module) with pinned URLs and sha256 checksums that downloads into a git-ignored
     cache, and make the tests skip when the cache is absent. Add an opt-in/manual CI job for it. Say which route you took
     and why.
2. **Availability.** `gwg.org` returned HTTP 503 with `Retry-After: 3600` once. Respect `Retry-After`, do not hammer, and if
   a source stays unreachable stop and report; never invent or reconstruct file contents. Verify checksums of what you get.
3. **Run.** For every PDF: `proof-pdf verify <file> --no-pdfa` (they are PDF/X-4, not PDF/A; the PDF/X structural layer is
   expected to pass and is the strongest signal), with the colour and text layers on and no geometry/threshold options; and
   `proof-pdf inspect <file>`. Also run the default `--pdfa` once to confirm nothing crashes. Collect every finding.
4. **Classify each finding** into exactly one class and record it:
   (a) *correct*: the patch really violates a rule we state (e.g. a patch with white overprint should raise
   `white-overprint`); (b) *false positive*: our check is wrong or too broad; (c) *limitation*: a known unmodelled feature,
   documented, not fixed; (d) *crash or traceback*: always a bug.
5. **Fix the clear defects** in the checks (class b and d), with a regression test per fix that uses a minimal constructed PDF
   (not the vendored file) wherever possible. Do not paper over a check by weakening it for one patch.
6. **Expected-results file.** `tests/fixtures/suites/expected.toml` (or next to the cache script) maps each file to the set
   of checks it is expected to raise, each with a one-line justification. A test `tests/test_proof_pdf/test_suites.py`,
   marked so it skips cleanly when the files are absent, asserts: no file crashes, and the actual set of checks per file
   equals the expected set.
7. **Budget.** At most three rounds of run → classify → fix. Then stop. Report what remains, ranked by how likely it is to
   matter for real print jobs, and let the user decide. Do not keep going "until clean".

## Out of scope (do not start)

Triage of real decks as a labelled corpus; a PDF/X-4 rule subset or full validator; trapping, die-line or imposition checks;
changing existing checks' thresholds or defaults; moving contrast to the clipped extent; publishing anything (PyPI, Docker,
GitHub).

## Definition of done

- Part A: the check, flag, config key, docs, unit + fixture + integration tests, walker clip tracking with its own tests, and
  the real-deck sanity report.
- Part B: licence report; vendored subset or fetch script; classified findings; fixes with regression tests; the expected-
  results file and its test; the three-round report of what remains.
- Everything formatted, linted, type-checked, and the full suite green locally and in `bin/docker/test.sh`; fixtures
  regenerated and `python tests/fixtures/build_pdfs.py --check` clean; README/CHANGELOG/CONTRIBUTING updated.

## Final report (what the user reads)

State, in this order: what you built (files and flags); the licence finding and which route you took; a table of suite files
by finding class with counts; every code fix and its regression test; what is left, ranked; the real-deck sanity results; any
decision you made on the user's behalf; anything you could not verify. Be plain about failures and skipped steps.
