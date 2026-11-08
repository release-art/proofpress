"""What pikepdf + Pillow decode from every image-filter XObject: size, bands, per-band extrema, as JSON lines."""
import json, sys, signal, warnings
import pikepdf
warnings.simplefilter("ignore")
F = {"/DCTDecode", "/JPXDecode", "/CCITTFaxDecode", "/JBIG2Decode"}
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError()))
for path in sys.argv[1:]:
    try:
        pdf = pikepdf.open(path)
    except Exception:
        continue
    for o in pdf.objects:
        try:
            if o._type_name != "stream" or str(o.get("/Subtype")) != "/Image":
                continue
            f = o.get("/Filter")
            names = [] if f is None else [str(x) for x in f] if f._type_name == "array" else [str(f)]
            if not F & set(names):
                continue
            rec = {"file": path, "obj": f"{o.objgen[0]}.{o.objgen[1]}", "filter": names[-1], "cs": str(o.get("/ColorSpace"))[:12]}
            signal.alarm(30)
            try:
                img = pikepdf.PdfImage(o).as_pil_image()
                ext = img.getextrema()
                ext = [list(ext)] if not isinstance(ext[0], tuple) else [list(e) for e in ext]
                rec |= {"mode": img.mode, "size": list(img.size), "ext": ext}
            except Exception as exc:
                rec["err"] = type(exc).__name__
            signal.alarm(0)
            print(json.dumps(rec), flush=True)
        except Exception:
            continue
