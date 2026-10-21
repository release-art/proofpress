#!/bin/bash
# Prove both local images work: compile with lualatex, then (full) verify the PDF's tooling is present.
set -euo pipefail
tex='\documentclass{article}\usepackage{fontspec}\begin{document}hello\end{document}'
for t in lualatex full; do
    docker run --rm --entrypoint sh "releaseart/$t:local" -c "cd /tmp && printf '%s' '$tex' > a.tex && lualatex -interaction=nonstopmode a.tex >/dev/null && test -s a.pdf"
    echo "ok: $t compiles"
done
docker run --rm --entrypoint sh releaseart/full:local -c 'verapdf --version >/dev/null && gs --version >/dev/null'
docker run --rm releaseart/full:local --help >/dev/null
echo "ok: full has verapdf, gs, proof-pdf"
