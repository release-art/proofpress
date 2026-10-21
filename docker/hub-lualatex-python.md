# releaseart/lualatex-python

Everything [`proof-pdf`](https://github.com/release-art/proofpress#proof-pdf) needs except `proof-pdf` itself: Python, LuaLaTeX
(the same TeX Live tree as [`releaseart/lualatex`](https://hub.docker.com/r/releaseart/lualatex)), Ghostscript and veraPDF with a JRE.
A base for images that `pip install proof-pdf` or your own packages. Want the CLI preinstalled? Use
[`releaseart/full`](https://hub.docker.com/r/releaseart/full).

```dockerfile
FROM releaseart/lualatex-python
RUN pip install --no-cache-dir proof-pdf
```

- `linux/amd64` and `linux/arm64`. Tags: `latest` (the most recent weekly or release build), `tl2025`, and `1.2.3` for each release.
- `HOME=/tmp` makes it work with a read-only root filesystem and `--user`.
- Contains Ghostscript (AGPL-3.0); its source is available from Debian (`apt-get source ghostscript`).

Source, issues and licences: https://github.com/release-art/proofpress
