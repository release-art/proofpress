PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

cd "${PROJECT_ROOT}"

DIRS=(
    "src"
    "tests"
)

pdm run ruff format "${DIRS[@]}"
pdm run ruff check --fix  "${DIRS[@]}"
pdm run pyright