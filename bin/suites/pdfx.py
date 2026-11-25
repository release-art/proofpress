"""Run the Rust engine over proof-pdf-test-db and compare with its expectations. Local only: never in public CI.

    python3 bin/suites/pdfx.py [--set NAME ...] [--level x4] [--timeout 60] [--jobs N]

Without --set it runs the files named in proof-pdf-test-db/expectations/pdfx.toml; each --set adds every distinct PDF of
that folder under samples/ (from samples/index.jsonl; files marked malicious are left out). Each file runs in its own
process, so a crash or a hang is an outcome, not the end of the run. Outcomes are ranked, worst first: crash, timeout,
refused, read with gaps, accepted. A file in the expectations must fail exactly the expected checks among those the
engine evaluated; a check not yet implemented is not compared. Results go to rust/target/pdfx-corpus/, stamped with the
engine version. Exits 1 on any crash, timeout or mismatch.
"""

import argparse
import concurrent.futures
import datetime
import json
import os
import pathlib
import subprocess
import sys
import time
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
TESTDB = pathlib.Path(os.environ.get("PROOF_PDF_TEST_DB", ROOT.parent / "proof-pdf-test-db"))
ENGINE = ROOT / "rust" / "target" / "release" / "examples" / "verify"
RANK = ("crash", "timeout", "refused", "gaps", "accepted")


def engine_version() -> str:
    git = subprocess.run(["git", "-C", str(ROOT), "describe", "--always", "--dirty"], capture_output=True, text=True)
    manifest = tomllib.loads((ROOT / "rust" / "Cargo.toml").read_text())
    return f"{manifest['package']['version']}+{git.stdout.strip() or 'unknown'}"


def run(path: pathlib.Path, level: str, timeout: float) -> dict:
    start = time.monotonic()
    try:
        done = subprocess.run([str(ENGINE), level, str(path)], capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"outcome": "timeout", "seconds": timeout}
    seconds = round(time.monotonic() - start, 2)
    if done.returncode not in (0, 1, 2):
        return {"outcome": "crash", "seconds": seconds, "detail": done.stderr.strip()[-300:]}
    report = json.loads(done.stdout)
    outcome = {"refused": "refused", "damaged": "gaps"}.get(report["structure"], "accepted")
    return {"outcome": outcome, "seconds": seconds, "report": report}


def compare(expected: dict, report: dict) -> str:
    evaluated = {c["id"] for c in report["checks"]}
    failed = {c["id"] for c in report["checks"] if c["status"] != "pass"}
    want = set(expected.get("expect", {})) & evaluated
    return "" if failed == want else f"expected {sorted(want)}, got {sorted(failed)}"


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--set", action="append", default=[], help="a folder under samples/ to run whole")
    parser.add_argument("--level", default="x4")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    args = parser.parse_args()

    expectations = tomllib.loads((TESTDB / "expectations" / "pdfx.toml").read_text())
    files = {path: expectations[path] for path in expectations}
    if args.set:
        index = [json.loads(line) for line in (TESTDB / "samples" / "index.jsonl").read_text().splitlines()]
        for record in index:
            wanted = record["set"] in args.set and record["kind"] == "pdf"
            if wanted and record["duplicate_of"] is None and not record["malicious"]:
                files.setdefault(record["path"], None)

    subprocess.run(["cargo", "build", "--release", "--example", "verify"], cwd=ROOT / "rust", check=True)
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        futures = {path: pool.submit(run, TESTDB / path, args.level, args.timeout) for path in files}
        results = {path: future.result() for path, future in futures.items()}

    version = engine_version()
    out = ROOT / "rust" / "target" / "pdfx-corpus"
    out.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now(datetime.UTC).strftime("%Y%m%dT%H%M%SZ")
    lines = [f"# proof-pdf engine {version}, level {args.level}, {stamp}", "path\toutcome\tverdict\tseconds\tmismatch"]
    bad = 0
    for path in sorted(files, key=lambda p: (RANK.index(results[p]["outcome"]), p)):
        result = results[path]
        report = result.get("report")
        mismatch = compare(files[path], report) if report and files[path] is not None else ""
        bad += result["outcome"] in ("crash", "timeout") or bool(mismatch)
        verdict = report["verdict"] if report else ""
        lines.append(
            f"{path}\t{result['outcome']}\t{verdict}\t{result['seconds']}\t{mismatch or result.get('detail', '')}"
        )
    target = out / f"results-{stamp}.tsv"
    target.write_text("\n".join(lines) + "\n")
    counts = {rank: sum(r["outcome"] == rank for r in results.values()) for rank in RANK}
    print(f"{len(files)} files: " + ", ".join(f"{n} {rank}" for rank, n in counts.items()) + f"; {bad} problems")
    print(f"results: {target}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
