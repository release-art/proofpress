#!/bin/bash
# bin/suites/fetch.sh   download the Ghent and Altona test suites into tests/fixtures/suites/.cache (git-ignored).
#
# The files are not redistributable (see tests/fixtures/suites/README.md), so they are fetched, never committed.
# Every download is checked against the pinned sha256; a mismatch deletes the file and fails. A server that answers
# 429/503 is not retried: its Retry-After is printed and the script stops, so run it again later.
set -euo pipefail
cd "$(dirname "$0")/../../tests/fixtures/suites"
mkdir -p .cache

sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }

# name  url  sha256
while read -r name url sum; do
  [[ -z "$name" || "$name" == \#* ]] && continue
  dest=".cache/$name"
  if [[ -f "$dest" && "$(sha256 "$dest")" == "$sum" ]]; then
    echo "ok       $name"
    continue
  fi
  code=$(curl -sSL -A "proof-pdf-calibration/0.1" -D .cache/.headers -o "$dest" -w '%{http_code}' "$url")
  if [[ "$code" != 200 ]]; then
    rm -f "$dest"
    echo "$name: HTTP $code (Retry-After: $(tr -d '\r' < .cache/.headers | awk -F': ' 'tolower($1)=="retry-after"{print $2}'))" >&2
    exit 1
  fi
  if [[ "$(sha256 "$dest")" != "$sum" ]]; then
    rm -f "$dest"
    echo "$name: sha256 mismatch -- the publisher changed the file; review before re-pinning" >&2
    exit 1
  fi
  echo "fetched  $name"
done <<'FILES'
# Altona Test Suite 2.0, technical page 2 (ECI). Pinned on first retrieval 2026-10-21.
eci_altona-test-suite-v2_technical2_x4.pdf https://eci.org/lib/exe/eci_altona-test-suite-v2_technical2_x4.pdf 57233105f699c6110bbeeeca769587ccfc8e543f1e58b791c812607bfefe294f
eci_altona-test-suite-v2_technical2_one-patch-per-page_x4.pdf https://eci.org/lib/exe/eci_altona-test-suite-v2_technical2_one-patch-per-page_x4.pdf 22a86029dcc30cf817265d4035a1a76e373da3f449ce8deabd9606d121d1da5a
FILES
rm -f .cache/.headers
