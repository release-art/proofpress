#!/bin/bash
# Prove both local images work: compile with lualatex, then (full) verify the PDF's tooling is present.
set -euo pipefail
tex='\documentclass{article}\usepackage{fontspec}\begin{document}hello\end{document}'
for t in lualatex lualatex-python full; do
    docker run --rm --entrypoint sh "releaseart/$t:local" -c "cd /tmp && printf '%s' '$tex' > a.tex && lualatex -interaction=nonstopmode a.tex >/dev/null && test -s a.pdf"
    # tlmgr needs both to download packages; without them `tlmgr install` fails quietly (exit 0).
    docker run --rm --entrypoint sh "releaseart/$t:local" -c 'command -v curl && command -v xz && latexmk -v' >/dev/null
    echo "ok: $t compiles, has latexmk"
done
docker run --rm --entrypoint sh releaseart/lualatex-python:local -c 'python --version >/dev/null && verapdf --version >/dev/null && gs --version >/dev/null && ! command -v proof-pdf'
docker run --rm --entrypoint sh releaseart/full:local -c 'verapdf --version >/dev/null && gs --version >/dev/null'
docker run --rm releaseart/full:local --version | grep -E "^proof-pdf [0-9]+\.[0-9]+\.[0-9]+.*\(commit [0-9a-f]{12}"
docker run --rm --entrypoint sh releaseart/lualatex:local -c 'test -n "$PROOFPRESS_REVISION"'
echo "ok: full has verapdf, gs, proof-pdf"
