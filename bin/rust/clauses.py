"""Write rust/clauses/<standard>.txt: the clause numbers of each standard held in proof-pdf-test-db, numbers only.

    python3 bin/rust/clauses.py ../proof-pdf-test-db

Reads the Markdown copies in the test-db's standards/iso/, where each clause heading starts a line with its number in
bold (``**6.4.3.3**``), and each annex is named ``**Annex B**``; a bold number alone is a clause only up to the last numbered clause, since
page folios look the same. No title or text is kept: the lists exist so that
rust/build.rs can refuse a check that cites a clause the standard does not have.
"""

import pathlib
import re
import sys

STANDARDS = {
    "iso-15930-7": "iso-15930-7-2010-cgats.md",
    "iso-15930-8": "iso-15930-8-2010-cgats.md",
    "iso-32000-1": "iso-32000-1-2008-adobe.md",
    "iso-32000-2": "iso-32000-2-2020-ec3.md",
}
HEADING = re.compile(r"^\*\*([0-9]+(?:\.[0-9]+)*|[A-Z](?:\.[0-9]+)+)(?:\*\*| )")
ANNEX = re.compile(r"\*\*Annex ([A-Z])\*\*")


def numbers(text: str) -> list[str]:
    found = {m.group(1) for line in text.splitlines() if (m := HEADING.match(line))}
    # A bare bold number is also how page folios look; keep one only within the range of numbered clauses.
    last = max(int(n.split(".")[0]) for n in found if "." in n and n[0].isdigit())
    found = {n for n in found if "." in n or not n.isdigit() or int(n) <= last}
    found |= set(ANNEX.findall(text))
    return sorted(found, key=lambda n: [(0, int(p)) if p.isdigit() else (1, p) for p in n.split(".")])


def main() -> int:
    iso = pathlib.Path(sys.argv[1]) / "standards" / "iso"
    out = pathlib.Path(__file__).resolve().parents[2] / "rust" / "clauses"
    out.mkdir(exist_ok=True)
    for name, source in STANDARDS.items():
        found = numbers((iso / source).read_text(encoding="utf-8"))
        (out / f"{name}.txt").write_text("".join(f"{n}\n" for n in found))
        print(f"{name}: {len(found)} clauses")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
