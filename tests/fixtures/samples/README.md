# Colour samples

One small LuaLaTeX document per colour situation, built into `../pdfs/` (with the other fixture PDFs) and checked by `tests/test_proof_pdf/test_samples.py`.
Both the `.tex` sources and the PDFs (`tests/fixtures/pdfs/`) are committed, so CI runs on the PDFs and needs no LaTeX; the sources are there to
read, confirm and change.

- **`expected.toml`** says what `proof-pdf verify` must report for each sample in each colour mode (`cmyk`, `rgb`, `gray`; and `gray_k_only`, gray with `--allow-k-only`, defaulting to `gray`), as `check/severity`
  (an empty list is a pass). `options` are the `Profile` fields for that sample.
- **Real packages first.** `xcolor-*`, `tikz-*`, `pgfplots-*`, `colortbl-*`, `tcolorbox-*`, `hyperref-*`, `figure-*`,
  `image-*`, `pdfx-*` and `pdfa-*` show what the standard tools actually write. LuaLaTeX ignores the ICC profile of a
  JPEG/PNG, so the ICC-tagged images, Separation inks, output intents, inline images, stencil masks and raw shadings and
  patterns come from `pdfraw.tex` (documented at its top).
- **`% needs: name ...`** in a source builds those samples first, so it can `\includegraphics{name.pdf}`. A
  `NAME.xmpdata` beside `NAME.tex` is for `pdfx`.
- **To change or add one:** edit the `.tex`, add or update its entry in `expected.toml`, then
  `pdm run python tests/fixtures/samples/build.py [name ...]` (needs `lualatex` and a CMYK profile, from TeX Live's
  `colorprofiles` or Ghostscript). `SOURCES.json` holds a digest of each source, and a test fails until the PDF is
  rebuilt.

Things the samples established about the tools (each is a test):

- Default `xcolor`: `red`, `green`, `blue` are RGB but `cyan`, `magenta`, `yellow` and `black` are CMYK, so a "default"
  document is a mix. `[rgb]` and `[cmyk]` convert everything; `[dvipsnames]` colours are CMYK, `[svgnames]` are RGB.
- pgf shadings follow the xcolor model, but pgf **patterns** are always `[/Pattern /DeviceRGB]`, even under `[cmyk]`.
- Transparency (`opacity`, soft masks) leaves the page without a blend colour space unless the document sets one.
- hyperref link boxes are annotations (`/C` is RGB) without the Print flag: they do not print and are not checked.
- `pdfx` gives a real PDF/X-4 (FOGRA39 intent, page boxes, XMP identity) or PDF/A-2b (sRGB intent) that passes the
  tool's standards layers.
