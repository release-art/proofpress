# Sourced by build.sh and test.sh: in GitHub Actions, keep the build layers (above all the TeX Live install, which
# downloads every package from the mirror) in the Actions cache, so a rebuild with the same inputs never touches it.
CACHE=()
if [[ -n "${ACTIONS_RESULTS_URL:-}" ]]; then
    CACHE=(--cache-from "type=gha,scope=smoke-$(uname -m)" --cache-to "type=gha,mode=max,scope=smoke-$(uname -m)")
fi
