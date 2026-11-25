#!/bin/bash
# bin/suites/pdfx.sh [args]   run the Rust engine over proof-pdf-test-db's PDF/X sets; see bin/suites/pdfx.py.
# Local only: the sets are private, so this never runs in public CI.
set -euo pipefail
cd "$(dirname "$0")/../.."
exec pdm run python bin/suites/pdfx.py "$@"
