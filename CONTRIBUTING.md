# Contributing

Issues and pull requests are welcome.

```zsh
pdm install
git config core.hooksPath .githooks   # the pre-commit hook runs the checks below
bin/autoformat.sh                     # ruff format, ruff check --fix, pyright
bin/test.sh                           # pytest with coverage
```

- Code is formatted and linted with ruff, type-checked with pyright (0 errors), and runs on Python 3.15.
- The Rust core lives in `rust/`, the port described in `docs/briefs/rust-port.md`. Install `rustup` (the toolchain version is
  pinned in `rust/rust-toolchain.toml`) and `cargo-nextest`, `cargo-llvm-cov` and `cargo-deny`, then run `bin/rust/check.sh`
  (`--fix` formats first). It runs rustfmt, clippy with the pedantic set denied, `cargo doc`, cargo-deny (licences,
  advisories, sources), the tests, and a **100% coverage gate** on lines, regions and functions, with no exclusions.
  - Unit and property tests (`proptest`) sit next to the code, in `#[cfg(test)]` modules.
  - Tests with mocks (`mockall`) go in `rust/tests/`, which coverage does not count, so generated mock code cannot dilute it.
  - Behaviour is described in Gherkin under `rust/tests/features/`, run by cucumber (`rust/tests/bdd.rs`).
  - The library may not panic on input: `unwrap`, `expect`, `panic!` and slice indexing are denied outside tests.
  - Tiers depend only downwards; `rust/tests/architecture.rs` holds the map of which module may use which, and a new module
    must be added to it. The other conventions (what is `pub`, errors, British spelling, no logging, fixtures) are under
    "Code conventions" in `docs/notes/rust-architecture.md`.
- Public API is the top-level modules of `src/proof_pdf`; everything else lives in `proof_pdf/_internal/` and may change
  freely. A new module goes in `_internal` unless it is meant to be imported by users (then add it to `PUBLIC` in
  `tests/test_proof_pdf/test_public_api.py` and document it).
- New behaviour comes with a test. The suite builds small real PDFs with pikepdf and stubs the external tools
  (`verapdf`, `gs`) at the subprocess boundary.
- Add a line under `## [Unreleased]` in `CHANGELOG.md` for anything user-visible.
- Integration tests (`tests/test_proof_pdf/test_integration.py`) run the real CLI on small PDFs against real veraPDF and
  Ghostscript, and skip without them; `bin/docker/test.sh` runs the whole suite in the image so they always run there.
  The PDFs are built by `tests/fixtures/build_pdfs.py` (one passing file, the rest each broken in one way). After changing
  the builder, regenerate with `pdm run python tests/fixtures/build_pdfs.py`; a test fails if the committed PDFs are stale.
  The colour samples beside them are LaTeX (`tests/fixtures/samples/*.tex`, built by its `build.py` into the same
  `tests/fixtures/pdfs/`); see the README there. Both builders leave each other's files alone.
- PDF/X rules are rows in `src/proof_pdf/_internal/pdfx/x4.py`; a clause is only filled in from the standard
  itself.
- Calibration against the free Ghent and Altona test suites: `bin/suites/fetch.sh` downloads them (they may not be
  redistributed, so they are never committed) and `pdm run pytest tests/test_proof_pdf/test_suites.py` checks each file
  raises exactly the checks in `tests/fixtures/suites/expected.toml`. The tests skip without the files; CI runs them by
  manual dispatch. A new finding on these files is a false positive to fix, or a line in `expected.toml` saying why not.
- Sample PDFs for trying the parser on files we did not make live in the private `proof-pdf-test-db` repository, checked out
  next to this one at `../proof-pdf-test-db`; see its `README.md`. No test may depend on them.
- Image changes: `bin/docker/build.sh && bin/docker/smoke.sh`.
