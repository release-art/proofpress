#!/bin/bash
# Print PKG_VERSION= and REVISION= for the checked-out commit (needs tags: fetch-depth 0 in CI).
#   on a clean v1.2.3 tag:  PKG_VERSION=1.2.3
#   anywhere else:          PKG_VERSION=<nearest v* tag or 0.0.0>+g<short sha>
# REVISION is the full commit, with -dirty appended for a modified tree.
cd "$(dirname "$0")/../.."
rev=$(git rev-parse HEAD 2>/dev/null || echo unknown)
[ -z "$(git status --porcelain 2>/dev/null)" ] || rev="$rev-dirty"
if [[ $rev != *-dirty ]] && tag=$(git describe --exact-match --tags --match 'v*' 2>/dev/null); then
    version=${tag#v}
else
    tag=$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null || true)
    version="${tag#v}"
    version="${version:-0.0.0}+g$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
fi
echo "PKG_VERSION=$version"
echo "REVISION=$rev"
