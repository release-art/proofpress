"""Findings of pdfx.check (pikepdf) against the Rust port, on every file named. Run with the wheel on PYTHONPATH."""
import collections, pathlib, signal, sys, time, warnings
import pikepdf, proofcore
from proof_pdf._internal import pdfx
warnings.simplefilter("ignore")
c = collections.Counter(); ex = []; tp = tr = 0.0
for f in sys.argv[1:]:
    t = time.perf_counter()
    try:
        with pikepdf.open(f) as pdf:
            a = [(x.check, str(x.severity), x.message, x.page) for x in pdfx.check(pdf)]
    except (pikepdf.PdfError, pikepdf.PasswordError) as exc:
        a = type(exc).__name__
    except Exception as exc:
        a = "CRASH:" + type(exc).__name__
    tp += time.perf_counter() - t
    t = time.perf_counter()
    try:
        b = proofcore.check_pdfx(f)
    except ValueError as exc:
        b = "REFUSED"
    tr += time.perf_counter() - t
    c["files"] += 1
    if isinstance(a, str) or isinstance(b, str):
        key = f"py={a if isinstance(a, str) else 'ok'} rs={b if isinstance(b, str) else 'ok'}"
        c[key] += 1
        continue
    c["findings"] += len(a)
    if a == b:
        c["identical"] += 1
    else:
        c["DIFF"] += 1
        ex.append((f.rsplit("/", 1)[-1], sorted(set(map(str, a)) ^ set(map(str, b)))[:2]))
print(dict(c)); print(f"python {tp:.2f}s  rust {tr:.2f}s")
for e in ex[:14]:
    print(" ", str(e)[:230])
