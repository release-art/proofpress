# releaseart/lualatex-python

Everything [`proof-pdf`](https://github.com/release-art/proofpress) needs except proof-pdf: Python (the official `python` image,
Debian), TeX Live 2025 with LuaLaTeX, Ghostscript, and veraPDF with its JRE. A base for images that install it, or your own
packages, with pip.

```dockerfile
FROM releaseart/lualatex-python
RUN pip install --no-cache-dir proof-pdf
```

- Want the CLI already installed? Use [`releaseart/full`](https://hub.docker.com/r/releaseart/full), which is this image plus `proof-pdf`.
- Same TeX tree as [`releaseart/lualatex`](https://hub.docker.com/r/releaseart/lualatex); add more with `tlmgr install`.
- TeX Live comes from the frozen 2025 mirror, so the packages don't drift between builds. The Debian and Python bases do.
- `linux/amd64` and `linux/arm64`. `HOME=/tmp` makes it work with a read-only root filesystem and `--user`.
- Tags: `latest` (the most recent weekly or release build), `tl2025`, and `1.2.3` for each release.
- The image's commit is in `$PROOFPRESS_REVISION` and the `org.opencontainers.image.revision` label.
- Ghostscript is AGPL-3.0-or-later; its source is available with `apt-get source ghostscript`.

Source, issues and licences: https://github.com/release-art/proofpress
