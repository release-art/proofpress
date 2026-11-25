#!/bin/bash
# bin/rust/check.sh   every gate for the Rust crate: format, lints, docs, licences, tests, and 100% coverage.
#
# Needs the toolchain from rust/rust-toolchain.toml plus cargo-nextest, cargo-llvm-cov and cargo-deny on PATH.
# `--fix` formats and applies clippy's fixes first.
set -euo pipefail
cd "$(dirname "$0")/../../rust"

if [[ "${1:-}" == "--fix" ]]; then
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
