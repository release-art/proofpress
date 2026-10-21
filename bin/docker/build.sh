#!/bin/bash
# bin/docker/build.sh [lualatex|full ...]   (default: both) -> releaseart/<target>:local, host arch
set -euo pipefail
cd "$(dirname "$0")/../.."
eval "$(bin/docker/version.sh)" # PKG_VERSION, REVISION
source bin/docker/cache.sh
bin/docker/base-digests.sh
for target in "${@:-lualatex full}"; do
    for t in $target; do
        docker buildx build --target "$t" --load ${CACHE[@]+"${CACHE[@]}"} \
            --build-arg VERSION="$PKG_VERSION" --build-arg REVISION="$REVISION" --build-arg PKG_VERSION="$PKG_VERSION" \
            -t "releaseart/$t:local" .
    done
done
