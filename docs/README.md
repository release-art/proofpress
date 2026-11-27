# Docs

| Document | What it is |
|---|---|
| [`interpretations.md`](interpretations.md) | How the checks read the standards where they are unclear, by the ids code and tests cite (`X4-Q3`, `L-Q2`) |
| [`notes/rust-architecture.md`](notes/rust-architecture.md) | The Rust engine: tiers, the listener pipeline, events, damage, findings, and the code conventions |
| [`../rust/CHECKS.md`](../rust/CHECKS.md) | Every check, generated from `rust/checks.toml` |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Toolchains, gates, and how to add a check |

## What proof-pdf is

An independent check for print files:

- It never repairs, so it reports defects a converter fixes silently.
- PDF/A is veraPDF's verdict, not ours.
- Total ink is measured from a CMYK render, not read from object colours.
- A check that cannot run is reported as not evaluated, never as a pass.
- Every refusal says what is wrong and where.

## Where samples and standards live

| Kind | Where | Rule |
|---|---|---|
| Built fixtures | `tests/fixtures/pdfs/` | Committed. Tests depend on them |
| Altona suite | `tests/fixtures/suites/.cache/`, by `bin/suites/fetch.sh` | Not redistributable, so fetched and never committed |
| Every other sample, the standards, private notes | `proof-pdf-test-db`, a sibling checkout at `../proof-pdf-test-db` | Never copied out. No test may depend on it |

The standards the checks are written from (ISO 32000, ISO 15930, ISO 19005-2, ISO 14289, ICC.1, the GWG specifications) are in that
repository's `standards/`. Code and docs here cite their clause and page numbers only, never their text.
