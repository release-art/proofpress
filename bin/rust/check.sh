#!/bin/bash
# bin/rust/check.sh   every gate for the Rust crate: format, lints, docs, licences, tests, and 100% coverage.
#
# Needs the toolchain from rust/rust-toolchain.toml plus cargo-nextest, cargo-llvm-cov and cargo-deny on PATH.
# `--fix` refreshes rust/CHECKS.md and the README's check section, formats and applies clippy's fixes first.
set -euo pipefail
cd "$(dirname "$0")/../../rust"

# The python feature links libpython 3.15 into the tests; PyO3 finds it through this interpreter (pdm's, by default).
if [[ -z "${PYO3_PYTHON:-}" && -x ../.venv/bin/python ]]; then
  export PYO3_PYTHON="$PWD/../.venv/bin/python"
fi

if [[ "${1:-}" == "--fix" ]]; then
  # rust/CHECKS.md is build.rs's listing of checks.toml, committed so it can be read without building.
  cargo build -q
  out="$(dirname "$(ls -t target/debug/build/proof-pdf/*/out/CHECKS.md | head -1)")"
  cp "$out/CHECKS.md" CHECKS.md
  # The README's check section, between its markers, from the same file.
  awk -v section="$out/README-checks.md" '
    /^<!-- \/checks -->$/ { skipping = 0 }
    !skipping { print }
    /^<!-- checks -->$/ { while ((getline line < section) > 0) print line; skipping = 1 }
  ' ../README.md > ../README.md.new && mv ../README.md.new ../README.md
  cargo fmt
  cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
fi

cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check

# Coverage counts every kind of test: unit and property tests through nextest, then the cucumber scenarios, which
# nextest cannot run. Nothing is excluded: `#[coverage(off)]` is unstable, and the gate is on stable Rust.
cargo llvm-cov clean --workspace
cargo llvm-cov --no-report nextest --all-features --profile "${NEXTEST_PROFILE:-default}"
cargo llvm-cov --no-report --all-features --test bdd
cargo llvm-cov report --fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100
