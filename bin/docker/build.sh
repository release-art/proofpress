#!/bin/bash
# bin/docker/build.sh [lualatex|full ...]   (default: both) -> releaseart/<target>:local, host arch
set -euo pipefail
cd "$(dirname "$0")/../.."
for target in "${@:-lualatex full}"; do
    for t in $target; do
        docker buildx build --target "$t" --load \
            --build-arg VERSION="$(git describe --tags --always --dirty 2>/dev/null || echo dev)" \
            --build-arg REVISION="$(git rev-parse HEAD 2>/dev/null || echo unknown)" \
            -t "releaseart/$t:local" .
    done
done
