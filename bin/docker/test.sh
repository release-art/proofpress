#!/bin/bash
# bin/docker/test.sh [pytest args]   run the whole suite inside the full image, with real veraPDF and Ghostscript.
set -euo pipefail
cd "$(dirname "$0")/../.."
eval "$(bin/docker/version.sh)" # PKG_VERSION
source bin/docker/cache.sh
docker buildx build --target test --load ${CACHE[@]+"${CACHE[@]}"} --build-arg PKG_VERSION="$PKG_VERSION" -t releaseart/test:local .
docker run --rm releaseart/test:local "${@:--q}" -p no:cacheprovider
