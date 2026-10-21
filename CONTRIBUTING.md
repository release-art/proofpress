# Contributing

Issues and pull requests are welcome.

```zsh
pdm install
git config core.hooksPath .githooks   # the pre-commit hook runs the checks below
bin/autoformat.sh                     # ruff format, ruff check --fix, pyright
bin/test.sh                           # pytest with coverage
```

- Code is formatted and linted with ruff, type-checked with pyright (0 errors), and runs on Python 3.15.
- New behaviour comes with a test. The suite builds small real PDFs with pikepdf and stubs the external tools
  (`verapdf`, `gs`) at the subprocess boundary.
- Add a line under `## [Unreleased]` in `CHANGELOG.md` for anything user-visible.
- Integration tests (`tests/test_proof_pdf/test_integration.py`) run the real CLI on small PDFs against real veraPDF and
  Ghostscript, and skip without them; `bin/docker/test.sh` runs the whole suite in the image so they always run there.
  The PDFs are built by `tests/fixtures/build_pdfs.py` (one passing file, the rest each broken in one way). After changing
  the builder, regenerate with `pdm run python tests/fixtures/build_pdfs.py`; a test fails if the committed PDFs are stale.
- Image changes: `bin/docker/build.sh && bin/docker/smoke.sh`.
