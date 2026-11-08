"""Kura's report profiles against proof-pdf at the same thresholds, per file and per concern. KURA and KURA_PROFILES from the env."""
import asyncio, collections, json, os, pathlib, subprocess, sys, warnings
from proof_pdf import core
from proof_pdf._internal import colour
from proof_pdf.models import Profile
warnings.simplefilter("ignore")
K, PROFILES = os.environ["KURA"], pathlib.Path(os.environ["KURA_PROFILES"])
# concern: (Kura profile, Kura check name, test on a proof-pdf finding)
CONCERNS = {
    "hairline <=0.125 pt": ("report-hairlines", "Stroke thinner than 0.125 pt", lambda f: f.check == "hairline"),
    "text < 5 pt": ("report-small-text", "Text smaller than 5 pt", lambda f: f.check == "text-size" or "smaller than 5" in f.message),
    "total ink > 300%": ("report-ink-coverage", "Total ink above 300%", lambda f: f.check == "tac" and f.severity == "error"),
    "white overprint": ("report-overprint", "White object set to overprint", lambda f: "white" in f.message.lower() and "overprint" in f.message.lower()),
    "image < 300 ppi": ("report-low-resolution-images", "Image below 300 ppi", lambda f: f.check in ("dpi", "resolution", "image-dpi") and f.severity == "error"),
}
profile = Profile(pdfa=False, pdfx=False, min_text_pt=5.0, min_stroke_pt=0.125, min_dpi=300)
c = collections.Counter(); diff = collections.defaultdict(list); ids = collections.Counter()
for f in sys.argv[1:]:
    path = pathlib.Path(f)
    findings, opened = core._check_pdf(path, profile)
    if opened:
        try:
            findings += asyncio.run(colour.check_tac("gs", f, profile.max_tac))
        except colour.GhostscriptError:
            pass
    for x in findings:
        ids[x.check] += 1
    for concern, (prof, name, test) in CONCERNS.items():
        p = subprocess.run([K, "--check", "--profile", str(PROFILES / f"{prof}.json"), f], capture_output=True, text=True, timeout=120)
        try:
            hits = json.loads(p.stdout.strip().splitlines()[-1]).get("analysis", [])
        except (json.JSONDecodeError, IndexError):
            c[(concern, "kura gave no answer")] += 1
            continue
        k = any(name in h["detail"] for h in hits)
        ours = any(test(x) for x in findings)
        c[(concern, f"proof-pdf {'flags' if ours else 'clean'} / kura {'flags' if k else 'clean'}")] += 1
        if k != ours:
            msg = next((x.message for x in findings if test(x)), "") or next((h["detail"] for h in hits if name in h["detail"]), "")
            diff[concern].append((path.name, "only proof-pdf" if ours else "only kura", msg[:90]))
for concern in CONCERNS:
    print(f"== {concern}")
    for (cn, k), v in sorted(c.items()):
        if cn == concern:
            print(f"   {v:4}  {k}")
    for d in diff[concern][:8]:
        print("     ", d)
print("proof-pdf check ids seen:", dict(ids))
