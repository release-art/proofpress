"""hayro against gs for previews, pdfpurr against veraPDF for PDF/A-2b, a no-raster ink estimate against the gs peak."""
import asyncio, collections, glob, json, os, pathlib, statistics, subprocess, sys, time, tempfile, warnings
import pikepdf
from PIL import Image, ImageChops, ImageStat
from proof_pdf._internal import colour, paint
warnings.simplefilter("ignore")
S=tempfile.gettempdir(); B=os.environ.get("STANDALONE_BIN","spikes/crabify/standalone/target/release/standalone")
fx=sorted(glob.glob("tests/fixtures/pdfs/*.pdf")); alt=sorted(glob.glob("tests/fixtures/suites/.cache/*.pdf"))
fx=[f for f in fx if "corrupt" not in f]

print("== render: hayro vs gs png16m, 100 dpi")
rows=[]; tg=th=0
with tempfile.TemporaryDirectory(dir=S) as t:
    for f in fx+alt:
        a=time.perf_counter()
        g=subprocess.run(["gs","-q","-dNOPAUSE","-dBATCH","-dSAFER","-sDEVICE=png16m","-r100","-dFirstPage=1","-dLastPage=1",f"-sOutputFile={t}/g.png",f],capture_output=True); dg=time.perf_counter()-a
        a=time.perf_counter(); h=subprocess.run([B,"render",f,"0","100",f"{t}/h.png"],capture_output=True,text=True,timeout=120); dh=time.perf_counter()-a
        if g.returncode or h.returncode: rows.append((f,None,None)); continue
        tg+=dg; th+=dh
        x=Image.open(f"{t}/g.png").convert("RGB"); y=Image.open(f"{t}/h.png").convert("RGBA"); w=Image.new("RGBA",y.size,"white"); w.alpha_composite(y); y=w.convert("RGB")
        if x.size!=y.size: y=y.resize(x.size)
        d=ImageChops.difference(x,y).convert("L"); hist=d.histogram(); n=sum(hist)
        rows.append((f,sum(hist[32:])/n*100,ImageStat.Stat(d).mean[0]))
ok=[r for r in rows if r[1] is not None]
print(f"  rendered {len(ok)}/{len(rows)}  pixels off by >32/255: median {statistics.median(r[1] for r in ok):.2f}% worst {max(r[1] for r in ok):.2f}%  gs {tg:.1f}s hayro {th:.1f}s")
for r in sorted(ok,key=lambda r:-r[1])[:5]: print(f"    {r[0].split('/')[-1][:50]:<52}{r[1]:.2f}% mean {r[2]:.1f}")
print("  failed:",[r[0].split('/')[-1] for r in rows if r[1] is None][:6])

print("== PDF/A-2b: pdfpurr vs veraPDF")
c=collections.Counter(); ex=[]
for f in fx+alt:
    v=subprocess.run(["verapdf","--format","json","-f","2b",f],capture_output=True,text=True)
    try:
        j=json.loads(v.stdout)["report"]["jobs"][0]["validationResult"]; j=j[0] if isinstance(j,list) else j; vera=bool(j["compliant"])
    except Exception: c["verapdf-no-verdict"]+=1; continue
    p=subprocess.run([B,"pdfa",f],capture_output=True,text=True,timeout=120)
    if p.returncode: c["pdfpurr-crash"]+=1; continue
    purr=p.stdout.strip()=="true"; c[f"vera={'pass' if vera else 'fail'} purr={'pass' if purr else 'fail'}"]+=1
    if vera!=purr: ex.append((f.split('/')[-1],vera,purr))
print(" ",dict(c)); print("  sample disagreements:",ex[:6])

print("== ink with no raster: max painted C+M+Y+K vs the gs peak")
def est(page):
    peak=0.0; blind=set()
    for p in paint.paint_events(page):
        if p.kind in("text","path"):
            for fill in ([p.fill] if p.kind=="text" or p.filled else [])+([p.stroke] if p.stroke_width else []):
                if fill.family==paint.CMYK and len(fill.value)>=4: peak=max(peak,sum(fill.value[:4])*100)
                elif fill.family==paint.GRAY and fill.value: peak=max(peak,(1-fill.value[0])*100)
                elif fill.family==paint.RGB: blind.add("rgb")
                else: blind.add("spot/pattern")
        else: blind.add(p.kind)
    return peak,blind
tot=collections.Counter(); bad=[]
for f in fx+alt:
    try: pdf=pikepdf.open(f); peaks=asyncio.run(colour._page_tac_peaks("gs",f))
    except Exception as e: tot["skip"]+=1; continue
    for page,g in zip(pdf.pages,peaks):
        e,bl=est(page); tot["pages"]+=1
        close=abs(e-g)<=2; tot["within 2 points" if close else "off"]+=1
        tot["same verdict at 300%"]+= (e>300)==(g>300)
        if (g>300)and not(e>300): tot["MISSED over-limit page"]+=1
        if (e>300)and not(g>300): tot["false alarm"]+=1
        if bl: tot["pages with content it cannot price"]+=1
        if not close and len(bad)<8: bad.append((f.split('/')[-1][:40],round(e),round(g),sorted(bl)))
print(" ",dict(tot))
for b in bad: print("   ",b)
