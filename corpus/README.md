# Sample PDFs

PDFs for testing the parser and the checks on files we did not make. Nothing here is committed except this file: `.gitignore`
covers the rest, so tests must not depend on it. Calibration against Altona is separate, see `tests/fixtures/suites/`.

Run `bin/suites/corpus.sh` to fetch the public sets. Private samples live in the `proof-pdf-test-db` repository; clone it to
`corpus/local/test-db`. Anything else of your own can go in `corpus/local/` too.

| Folder | What it is | PDFs | Licence |
|---|---|---|---|
| `local/` | Your own samples. InDesign, Illustrator and Acrobat output is what we most need | | Yours; keep client work out of reports |
| `stillhq-pdfdb/` | [pdf-raku/stillhq.com-pdfdb](https://github.com/pdf-raku/stillhq.com-pdfdb), scraped from the defunct stillhq.com | 545 | **None stated.** Fetch it, never redistribute it |
| `py-pdf-samples/` | [py-pdf/sample-files](https://github.com/py-pdf/sample-files) | 38 | CC-BY-SA 4.0 |
| `pdf-differences/` | [pdf-association/pdf-differences](https://github.com/pdf-association/pdf-differences), minimal files where PDF processors disagree | 37 | PDFs CC BY 4.0, repository Apache-2.0 |
| `pdf-cabinet-of-horrors/` | [The Archivist's PDF Cabinet of Horrors](http://opf-labs.org/format-corpus/pdfCabinetOfHorrors/), files with features archives avoid | 24 | CC0 |

The three repositories are plain clones pinned to a commit in `corpus.sh`. The Cabinet is mirrored over plain HTTP with no pin, so its
files can change.
