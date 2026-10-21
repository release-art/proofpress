# releaseart/full

[`releaseart/lualatex`](https://hub.docker.com/r/releaseart/lualatex) plus Ghostscript, veraPDF (with a JRE) and the
[`proof-pdf`](https://github.com/release-art/proofpress#proof-pdf) CLI, which holds print PDFs to PDF/X-4 and PDF/A.
The entrypoint is `proof-pdf`.

```zsh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/full verify job/ --trim-mm 105x148 --bleed-mm 3
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/full preview card.pdf --dpi 150
docker run --rm releaseart/full --version
```

To compile and verify in one container, override the entrypoint:

```zsh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work --entrypoint sh releaseart/full -c \
  'lualatex -interaction=nonstopmode book.tex && proof-pdf verify book.pdf'
```

- `verify` exits 1 when a file fails, and `--json` gives machine-readable findings.
- `linux/amd64` and `linux/arm64`. Tags: `latest` (the most recent weekly or release build), `tl2025`, and `1.2.3` for each release.
- `proof-pdf --version` shows the package version and the commit the image was built from.
- Contains Ghostscript (AGPL-3.0); its source is available from Debian (`apt-get source ghostscript`). Full third-party terms are in
  `/usr/share/doc/proof-pdf/NOTICE`.

Source, issues and licences: https://github.com/release-art/proofpress
