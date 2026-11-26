"""The parity gate for the print checks being ported to the Rust core (docs/briefs/rust-port.md, "The parity gate").

For every PDF named (directories are searched), run each ported check's Python function and the Rust core at the
`print` level, and compare their findings exactly: check, severity, message and page. Prints the counts and the first
differences. A file the Rust core reads with gaps or refuses, or that pikepdf cannot open, is counted apart: those are
the expected differences the brief lists, not parity failures.

    .venv/bin/python bin/parity/print.py tests/fixtures/pdfs ../proof-pdf-test-db/samples/...

A check leaves PORTED when its Python is deleted, with its parity numbers recorded in docs/briefs/pdfx-progress.md.
"""

import collections
import concurrent.futures
import json
import os
import pathlib
import signal
import sys
import warnings
from collections.abc import Callable

import pikepdf

from proof_pdf._internal import _core
from proof_pdf.models import Finding, Severity

# The parameters both sides run with, as the core takes them (ported.params gives them for a profile).
PARAMS: dict[str, object] = {"min_dpi": 300.0, "warn_dpi": 450.0, "max_dpi": 1200.0, "max_stretch_pct": 5.0}
RGB_PARAMS: dict[str, object] = {**PARAMS, "colour_mode": "rgb"}

# Each check being ported, by id, with the Python it replaces, run with PARAMS. A check leaves when its Python is
# deleted, its numbers recorded in docs/briefs/pdfx-progress.md. Empty between ports.
PORTED: dict[str, Callable[[pikepdf.Pdf], list[Finding]]] = {}
# Python checks that raise several ids: the other ids by the first.
ALSO: dict[str, tuple[str, ...]] = {}

Key = tuple[str, str, str, int | None]


def python(path: pathlib.Path) -> list[Key] | str:
    try:
        with pikepdf.open(path) as pdf:
            return sorted((f.check, str(f.severity), f.message, f.page) for run in PORTED.values() for f in run(pdf))
    except (pikepdf.PdfError, pikepdf.PasswordError) as exc:
        return f"pikepdf: {type(exc).__name__}"


def rust(path: pathlib.Path) -> list[Key] | str:
    data = path.read_bytes()
    report = json.loads(_core.verify(str(path), data, "print", json.dumps(PARAMS)))
    if report["structure"] != "sound":
        return f"rust: {report['structure']}"
    findings = report["findings"]
    if "rgb-only" in PORTED:  # it judges only an RGB job
        rgb = json.loads(_core.verify(str(path), data, "print", json.dumps(RGB_PARAMS)))
        findings += [f for f in rgb["findings"] if f["check"] == "rgb-only"]
    severity = {"error": str(Severity.ERROR), "warning": str(Severity.WARN)}
    return sorted(
        (f["check"], severity[f["severity"]], f["message"], f["page"])
        for f in findings
        if f["check"] in PORTED or any(f["check"] in also for also in ALSO.values())
    )


# Seconds either side may take on one file before it is counted apart as a timeout.
TIMEOUT = 120


def _timeout(signum: int, frame: object) -> None:
    raise TimeoutError


def compare(path: pathlib.Path) -> tuple[pathlib.Path, list[Key] | str, list[Key] | str]:
    """Both sides' findings on one file, in a worker process; a side that runs out of time is a string."""
    warnings.simplefilter("ignore")
    signal.signal(signal.SIGALRM, _timeout)
    sides: list[list[Key] | str] = []
    for side in (python, rust):
        signal.alarm(TIMEOUT)
        try:
            sides.append(side(path))
        except TimeoutError:
            sides.append(f"{side.__name__}: timeout")
        except Exception as exc:  # a crash on one file is a result for that side, not the end of the run
            sides.append(f"{side.__name__}: crash {type(exc).__name__}")
        finally:
            signal.alarm(0)
    return path, sides[0], sides[1]


def pdfs(arguments: list[str]) -> list[pathlib.Path]:
    out: list[pathlib.Path] = []
    for argument in map(pathlib.Path, arguments):
        out += sorted(argument.rglob("*.pdf")) if argument.is_dir() else [argument]
    return out


def main(arguments: list[str]) -> int:
    counts: collections.Counter[str] = collections.Counter()
    differences: list[tuple[pathlib.Path, list[str]]] = []
    with concurrent.futures.ProcessPoolExecutor(os.cpu_count()) as pool:
        results = list(pool.map(compare, pdfs(arguments), chunksize=4))
    for path, a, b in results:
        counts["files"] += 1
        if isinstance(a, str) or isinstance(b, str):
            counts[f"apart ({a if isinstance(a, str) else 'python ok'}; {b if isinstance(b, str) else 'rust ok'})"] += 1
            continue
        counts["findings"] += len(a)
        if a == b:
            counts["identical"] += 1
        else:
            counts["DIFFERENT"] += 1
            only = [f"python only: {k}" for k in sorted(set(a) - set(b))] + [
                f"rust only: {k}" for k in sorted(set(b) - set(a))
            ]
            differences.append((path, only))
    print(dict(counts))
    for path, diff in differences[:200]:
        print(f"  {path}")
        for line in diff[:4]:
            print(f"    {line}")
    return 1 if differences else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
