"""Run a Rust backend over every file in a baseline and report where it differs from pikepdf.

    python compare.py PARITY_BIN BACKEND BASE.jsonl [--detail N]
"""

import collections
import json
import pathlib
import subprocess
import sys

binary, backend, base = sys.argv[1:4]
detail = int(sys.argv[sys.argv.index("--detail") + 1]) if "--detail" in sys.argv else 0
t = collections.Counter()
bad: dict[str, list[str]] = collections.defaultdict(list)
times = collections.Counter()
reasons = collections.Counter()


def near(a, b):
    if a is None or b is None:
        return a is None and b is None
    return len(a) == len(b) and all(abs(x - y) < 0.01 for x, y in zip(a, b))


for line in open(base):
    r = json.loads(line)
    name = pathlib.Path(r["file"]).name
    t["files"] += 1
    keys = "\n".join(r.get("objects", {}))
    try:
        p = subprocess.run([binary, backend, r["file"]], input=keys, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        t["HANG"] += 1
        bad["hang"].append(name)
        continue
    if p.returncode:
        t["CRASH"] += 1
        bad["crash"].append(f"{name}: {p.stderr.strip().splitlines()[-1][:110] if p.stderr.strip() else p.returncode}")
        continue
    q = json.loads(p.stdout)
    if not r["open"]:
        t["pike_rejects"] += 1
        if q["open"]:
            t["rust_recovers_more"] += 1
            bad["rust opens, pikepdf refuses"].append(f"{name} ({r['err']})")
        continue
    t["pike_opens"] += 1
    if not q["open"]:
        t["OPEN_FAIL"] += 1
        reasons[q["err"].split(":")[0][:48]] += 1
        bad["pikepdf opens, rust refuses"].append(f"{name}: {q['err'][:70]}")
        continue
    if r["npages"] != q["npages"]:
        t["PAGECOUNT"] += 1
        bad["page count"].append(f"{name}: {r['npages']} vs {q['npages']}")
        continue
    t["files_compared"] += 1
    if r["root"] != q["root"]:
        t["root_diff"] += 1
    for a, b in zip(r["pages"], q["pages"]):
        t["pages"] += 1
        same = all(near(a[k], b[k]) for k in ("media", "crop", "trim", "bleed")) and a["rotate"] == b["rotate"]
        if not same:
            t["box_diff"] += 1
            bad["boxes"].append(f"{name}: {a} vs {b}")
    for k, v in r["objects"].items():
        t["objects"] += 1
        if q["objects"].get(k) != v:
            t["object_diff"] += 1
            bad["objects"].append(f"{name} {k}")
    for k, v in r["streams"].items():
        t["streams"] += 1
        w = q["streams"].get(k)
        if w != v:
            t["stream_diff"] += 1
            bad["streams"].append(f"{name} {k}: {v} vs {w}")
    for i, (a, b) in enumerate(zip(r["walks"], q["walks"])):
        t["walked_pages"] += 1
        t["ops"] += a[0]
        if a != b:
            t["walk_diff"] += 1
            bad["walks"].append(f"{name} p{i}: {a} vs {b}")
    for k in ("open_s", "objects_s", "walk_s"):
        times["py_" + k] += r[k]
        times["rs_" + k] += q[k]

sys.stdout.write(f"== {backend}\n{dict(t)}\n{ {k: round(v, 3) for k, v in sorted(times.items())} }\n")
if reasons:
    sys.stdout.write(f"  refusal reasons: {reasons.most_common(14)}\n")
for kind, items in bad.items():
    files = {i.split()[0].rstrip(":") for i in items}
    sys.stdout.write(f"  {kind}: {len(items)} in {len(files)} files\n")
    for i in items[:detail]:
        sys.stdout.write(f"    {i[:170]}\n")
