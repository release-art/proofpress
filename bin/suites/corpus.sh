#!/bin/bash
# bin/suites/corpus.sh   clone the public PDF sample repos into corpus/ (git-ignored), each at its pinned commit.
#
# They are plain clones, kept out of the repo: stillhq has no licence (see corpus/README.md). Run it again after changing a pin.
set -euo pipefail
cd "$(dirname "$0")/../../corpus"

# name  url  commit
while read -r name url commit; do
  [[ -z "$name" || "$name" == \#* ]] && continue
  [[ -d "$name/.git" ]] || git clone -q "$url" "$name"
  git -C "$name" checkout -q "$commit"
  echo "ok       $name  $(find "$name" -iname '*.pdf' -type f | wc -l | tr -d ' ') PDFs"
done <<'REPOS'
stillhq-pdfdb git@github.com:pdf-raku/stillhq.com-pdfdb.git ca89a9e9a4687744bbb5a6ba63f38daa146a725e
py-pdf-samples git@github.com:py-pdf/sample-files.git 80d67b91f3e28909756ca7807bbe8f43b599d12f
pdf-differences git@github.com:pdf-association/pdf-differences.git 907fe96e52b73e491489eee545c47b119bf9989b
REPOS

# The OPF PDF Cabinet of Horrors has no repository or pin: mirror its listing. A file already here is kept.
url=http://opf-labs.org/format-corpus/pdfCabinetOfHorrors/
mkdir -p pdf-cabinet-of-horrors
for file in $(curl -sSL -A proof-pdf-corpus "$url" | grep -oE 'href="[^"?/]+\.[a-z]+"' | cut -d'"' -f2); do
  [[ -f "pdf-cabinet-of-horrors/$file" ]] || curl -sSLf -A proof-pdf-corpus -o "pdf-cabinet-of-horrors/$file" "$url$file"
done
echo "ok       pdf-cabinet-of-horrors  $(find pdf-cabinet-of-horrors -iname '*.pdf' -type f | wc -l | tr -d ' ') PDFs"
