# releaseart/lualatex

TeX Live 2025 with LuaLaTeX on Debian. No Java, no Python. About 750 MB uncompressed.

```zsh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work releaseart/lualatex lualatex -interaction=nonstopmode book.tex
```

- Includes TeX Live's `collection-latex`, `luatex`, `latexrecommended` and `fontsrecommended` sets, plus `adjustbox`, `microtype`,
  `tcolorbox`, `pgf`, `fontspec`, `geometry` and `xcolor`. Add more in a derived image with `tlmgr install`.
- TeX Live comes from the frozen 2025 mirror, so the packages don't drift between builds. The Debian base does.
- `linux/amd64` and `linux/arm64`. `HOME=/tmp` makes it work with a read-only root filesystem and `--user`.
- Tags: `latest` (the most recent weekly or release build), `tl2025`, and `1.2.3` for each release.
- The image's commit is in `$PROOFPRESS_REVISION` and the `org.opencontainers.image.revision` label.

Need to check the PDFs it produces? See [`releaseart/full`](https://hub.docker.com/r/releaseart/full).

Source, issues and licences: https://github.com/release-art/proofpress
