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
