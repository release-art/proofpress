"""Kura (AGPL, run as an external tool) against veraPDF, the strict parser and proof-pdf. Paths come from the environment.

    KURA=... PARITY_BIN=... python spikes/crabify/kura.py verdicts|refusals FILE...
"""
import collections, json, os, subprocess, sys
KURA = os.environ["KURA"]
PARITY = os.environ.get("PARITY_BIN", "spikes/crabify/parity/target/release/parity")


def kura(f, *args):
    try:
        p = subprocess.run([KURA, "--check", *args, f], capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return "timeout", None
    if p.returncode not in (0, 1, 2, 3):
        return f"crash rc={p.returncode}", None
    try:
        return {0: "ok", 1: "findings", 2: "rejected", 3: "timeout"}[p.returncode], json.loads(p.stdout.strip().splitlines()[-1])
    except (json.JSONDecodeError, IndexError):
        return f"no json rc={p.returncode}", None


def vera(f, flavour):
    p = subprocess.run(["verapdf", "--format", "json", "-f", flavour, f], capture_output=True, text=True, timeout=300)
    try:
        j = json.loads(p.stdout)["report"]["jobs"][0]["validationResult"]
        j = j[0] if isinstance(j, list) else j
        return bool(j["compliant"]), j["details"]["failedRules"]
    except (json.JSONDecodeError, KeyError, IndexError, TypeError):
        return None, None


mode, files = sys.argv[1], sys.argv[2:]
c = collections.Counter(); ex = collections.defaultdict(list)
if mode == "verdicts":
    for f in files:
        v, rules = vera(f, "2b")
        status, j = kura(f, "--level", "2b")
        if v is None or j is None:
            c[f"no verdict (vera={v}, kura={status})"] += 1; continue
        k = bool(j.get("compliant"))
        key = f"vera {'pass' if v else 'fail'} / kura {'pass' if k else 'fail'}"
        c[key] += 1
        if v != k:
            ex[key].append((f.rsplit("/", 1)[-1], rules, [i["code"] for i in j.get("issues", [])][:4]))
else:
    for f in files:
        status, j = kura(f, "--level", "2b")
        q = subprocess.run([PARITY, "strict", f], input="", capture_output=True, text=True, timeout=120)
        ours = "crash" if q.returncode else ("ok" if json.loads(q.stdout)["open"] else "refused")
        k = "refused" if status == "rejected" else status if status not in ("ok", "findings") else "ok"
        c[f"kura {k} / strict {ours}"] += 1
        if k != ours:
            why = (j or {}).get("error") or [i["detail"][:50] for i in (j or {}).get("issues", []) if i.get("severity", 0) >= 3][:1]
            ex[f"kura {k} / strict {ours}"].append((f.rsplit("/", 1)[-1], str(why)[:70]))
for k, v in sorted(c.items(), key=lambda kv: -kv[1]):
    print(f"  {v:5}  {k}")
for k, v in ex.items():
    print(f"  -- {k}: {len(v)}")
    for e in v[:10]:
        print("     ", str(e)[:190])
