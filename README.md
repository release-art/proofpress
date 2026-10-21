# proofpress

Open-source pieces of a print-on-demand pipeline.

- **`proof-pdf`**: hold a print PDF to PDF/X-4 and PDF/A. Wraps veraPDF and adds page-box,
  geometry, total-ink, RGB, image-DPI and rich-black checks.
- **Docker images** (planned): `lualatex` (TeX Live) and `full` (adds veraPDF, Ghostscript and `proof-pdf`).

```zsh
pdm install
pdm run proof-pdf verify job/ --trim-mm 105x148 --bleed-mm 3 --exclude '*_preview.pdf'
pdm run proof-pdf preview job/card.pdf --dpi 150
```

`verify` needs `verapdf` and `gs` on PATH. A check whose tool is missing **fails**, it is never skipped.
Geometry is checked only when `--trim-mm` is given.

Development: `git config core.hooksPath .githooks`, then `bin/autoformat.sh` and `bin/test.sh`.

Licence: Apache-2.0. See `NOTICE` for third-party terms.

## Images

`releaseart/lualatex` (TeX Live 2025) and `releaseart/full` (adds Ghostscript, veraPDF and `proof-pdf`) on Docker Hub.
`:latest` is the weekly build of `main` (Monday, with fresh base images); `:1.2.3` tags come from a git tag and never move
`:latest`. Build locally with `bin/docker/build.sh`, check with `bin/docker/smoke.sh`.

## Releasing

Add the release to `CHANGELOG.md`, then push a tag: `git tag v1.2.3 && git push origin v1.2.3`.
The tag is the version. It runs the tests, publishes to PyPI (TestPyPI first), creates a GitHub Release and
pushes both Docker images. `proof-pdf --version` shows the version and the commit it was built from.
