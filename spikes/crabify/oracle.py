"""Does qpdf agree that what the strict parser refuses is damaged? pikepdf records a warning whenever qpdf repairs."""
import collections, json, os, re, subprocess, sys, warnings
import pikepdf
warnings.simplefilter("ignore")
B=os.environ.get("PARITY_BIN","spikes/crabify/parity/target/release/parity")
c=collections.Counter(); silent=[]; reasons=collections.Counter(); missed=[]
for base in sys.argv[1:]:
  for line in open(base):
    r=json.loads(line); name=r["file"].rsplit("/",1)[-1]
    p=subprocess.run([B,"strict",r["file"]],input="",capture_output=True,text=True,timeout=60)
    if p.returncode: c["CRASH"]+=1; continue
    q=json.loads(p.stdout)
    if not r["open"]:
        c["both refuse" if not q["open"] else "strict opens, pikepdf refuses"]+=1; continue
    with pikepdf.open(r["file"]) as pdf:
        len(pdf.pages)
        for o in pdf.objects: pass
        w=pdf.get_warnings()
        enc=pdf.is_encrypted
    c["files pikepdf opens"]+=1
    if q["open"]:
        c["strict accepts"]+=1
        if w: c["  ...though qpdf warned"]+=1; missed.append((name,w[0][:90]))
    else:
        reasons[q["err"].split(":")[0][:44]]+=1
        if enc: c["strict refuses: encrypted (by policy)"]+=1
        elif w: c["strict refuses: qpdf ALSO reports damage"]+=1
        else: c["strict refuses: QPDF SILENT (possible false reject)"]+=1; silent.append((name,q["err"][:60]))
for k,v in c.items(): print(f"  {k}: {v}")
print("  reasons:",reasons.most_common(14))
print("  possible false rejects:"); [print("    ",s) for s in silent]
print("  accepted despite a qpdf warning (sample):"); [print("    ",s) for s in missed[:8]]
