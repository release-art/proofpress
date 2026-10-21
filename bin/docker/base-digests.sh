#!/bin/bash
# Print the digests the Dockerfile's floating base tags resolve to right now.
cd "$(dirname "$0")/../.."
for arg in DEBIAN_IMAGE PYTHON_IMAGE; do
    image=$(sed -n "s/^ARG $arg=//p" Dockerfile)
    echo "$image $(docker buildx imagetools inspect "$image" --format '{{.Manifest.Digest}}')"
done
