# Contributing

Issues and pull requests are welcome.

```zsh
pdm install
git config core.hooksPath .githooks   # the pre-commit hook runs the checks below
bin/autoformat.sh                     # ruff format, ruff check --fix, pyright
bin/test.sh                           # pytest with coverage
```

- `pdm install` builds the Rust core (`rust/`) into the extension `proof_pdf._internal._core` with maturin, so it needs
  `rustup` too; the toolchain is the one pinned in `rust/rust-toolchain.toml`. Rebuild after changing Rust with
  `pdm install` again.
- Code is formatted and linted with ruff, type-checked with pyright (0 errors), and runs on Python 3.11 and later (development is on 3.15; ruff and
  pyright hold the code to 3.11, and CI tests 3.11 and 3.15). The wheel is one `abi3` build per platform.
- The Rust core lives in `rust/`. Install `rustup` (the toolchain version is
  pinned in `rust/rust-toolchain.toml`) and `cargo-nextest`, `cargo-llvm-cov` and `cargo-deny`, then run `bin/rust/check.sh`
  (`--fix` formats first). The `python` feature's tests link libpython 3.15, found through `PYO3_PYTHON` (pdm's
  `.venv` by default). It runs rustfmt, clippy with the pedantic set denied, `cargo doc`, cargo-deny (licences,
  advisories, sources), the tests, and a **100% coverage gate** on lines, regions and functions, with no exclusions.
  - Unit and property tests (`proptest`) sit next to the code, in `#[cfg(test)]` modules.
  - Tests with mocks (`mockall`) go in `rust/tests/`, which coverage does not count, so generated mock code cannot dilute it.
  - Behaviour is described in Gherkin under `rust/tests/features/`, run by cucumber (`rust/tests/bdd.rs`).
    PDF/X scenarios go in `rust/tests/features/pdfx/`, one file per clause (`x4-6.09-trapping.feature`), with the steps
    `Given the fixture "x4-good"`, `When it is verified as PDF/X-4`, `Then it passes`, `Then check "<id>" fails on page 1`
    and `Then no check fails but "<id>"`. What each fixture must raise is in `tests/fixtures/pdfx/expected.toml`.
  - The library may not panic on input: `unwrap`, `expect`, `panic!` and slice indexing are denied outside tests.
  - Every check is an entry in `rust/checks.toml` (id, title in our own words, the clauses it may cite, severity, the
    levels it applies to, and `implemented`, `todo` or `skip` with a reason). `rust/build.rs` refuses a malformed entry
    or a clause missing from `rust/clauses/`, and turns the file into one constant per check and `rust/CHECKS.md`,
    which `bin/rust/check.sh --fix` refreshes. The clause lists are numbers only, made by `bin/rust/clauses.py` from
    the standards in `proof-pdf-test-db`.
  - Tiers depend only downwards; `rust/tests/architecture.rs` holds the map of which module may use which, and a new module
    must be added to it. The other conventions (what is `pub`, errors, British spelling, no logging, fixtures) are under
    "Code conventions" in `docs/notes/rust-architecture.md`.
- Public API is the top-level modules of `src/proof_pdf`; everything else lives in `proof_pdf/_internal/` and may change
  freely. A new module goes in `_internal` unless it is meant to be imported by users (then add it to `PUBLIC` in
  `tests/test_proof_pdf/test_public_api.py` and document it).
- New behaviour comes with a test, in the language that holds it. A check's logic is tested in Rust (unit tests,
  fixtures and scenarios, above). The Python suite tests the Python surface (the API, the commands, `layout`, `texlog`
  and how `extract` assembles `document.json`) and runs the checks end to end through the core
  (`core_findings` and `core_run` in `tests/test_proof_pdf/conftest.py`). It builds small real PDFs with pikepdf, a
  development dependency only, and stubs the external tools (`verapdf`, `gs`) at the subprocess boundary.
- Add a line under `## [Unreleased]` in `CHANGELOG.md` for anything user-visible.
- Integration tests (`tests/test_proof_pdf/test_integration.py`) run the real CLI on small PDFs against real veraPDF and
  Ghostscript, and skip without them; `bin/docker/test.sh` runs the whole suite in the image so they always run there.
  The PDFs are built by `tests/fixtures/build_pdfs.py` (one passing file, the rest each broken in one way). After changing
  the builder, regenerate with `pdm run python tests/fixtures/build_pdfs.py`; a test fails if the committed PDFs are stale.
  The colour samples beside them are LaTeX (`tests/fixtures/samples/*.tex`, built by its `build.py` into the same
  `tests/fixtures/pdfs/`); see the README there. Both builders leave each other's files alone.
- PDF/X rules are checks in `rust/checks.toml` and code in `rust/src/checks/`; a clause is only filled in from the
  standard itself. Each negative fixture carries one defect. Where a clause is unclear, the reading goes in
  `docs/interpretations.md` under an id the code cites (`X4-Q3`); a changed expectation is either our bug, a file that
  does not conform, or a changed reading there.
- Calibration against the free Ghent and Altona test suites: `bin/suites/fetch.sh` downloads them (they may not be
  redistributed, so they are never committed) and `pdm run pytest tests/test_proof_pdf/test_suites.py` checks each file
  raises exactly the checks in `tests/fixtures/suites/expected.toml`. The tests skip without the files; CI runs them by
  manual dispatch. A new finding on these files is a false positive to fix, or a line in `expected.toml` saying why not.
- `bin/suites/pdfx.sh` runs the Rust engine over `proof-pdf-test-db` (its `expectations/pdfx.toml`, plus any `--set`), each
  file in its own process, ranks the outcomes (crash, timeout, refused, read with gaps, accepted), compares the checks
  that are implemented with the expectations, and writes a results file stamped with the engine version to
  `rust/target/pdfx-corpus/`. Local only: the sets are private.
- Sample PDFs for trying the parser on files we did not make live in the private `proof-pdf-test-db` repository, checked out
  next to this one at `../proof-pdf-test-db`; see its `README.md`. No test may depend on them.
- Image changes: `bin/docker/build.sh && bin/docker/smoke.sh`.
