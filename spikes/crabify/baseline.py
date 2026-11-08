"""Ground truth: what pikepdf (and proof_pdf's own walker) sees in each PDF, as JSON lines, for the Rust spikes to match.

    .venv/bin/python spikes/crabify/baseline.py OUT.jsonl FILE.pdf ...          one record per file
    .venv/bin/python spikes/crabify/baseline.py --dump-ops FILE.pdf PAGE        the canonical op lines of one page
"""

import hashlib
import json
import pathlib
import signal
import sys
import time

import pikepdf

from proof_pdf._internal import content

IMAGE_FILTERS = {"/DCTDecode", "/JPXDecode", "/CCITTFaxDecode", "/JBIG2Decode"}


def num(v) -> str:
    return f"{float(v):.4f}"


def canon(o, top=False) -> str:
    """One object as text; a nested indirect object is a reference, so this is the file's graph and not its expansion."""
    if isinstance(o, bool):
        return "true" if o else "false"
    if isinstance(o, (int, float)) or type(o).__name__ == "Decimal":
        return num(o)
    if o is None:
        return "null"
    if isinstance(o, pikepdf.Object):
        kind = o._type_name  # isinstance(o, pikepdf.Stream) is also true of a plain dictionary
        if not top and o.is_indirect:
            return f"R{o.objgen[0]}.{o.objgen[1]}"
        if kind == "name":
            return str(o)
        if kind == "string":
            return "(" + bytes(o).hex() + ")"
        if kind == "array":
            return "[" + " ".join(canon(v) for v in o) + "]"
        if kind in ("dictionary", "stream"):
            # qpdf inlines a stream's indirect /Length and corrects a wrong one, so it says nothing about the parser.
            keys = sorted(k for k in o.keys() if not (kind == "stream" and k == "/Length"))
            return "<<" + " ".join(f"{k} {canon(o[k])}" for k in keys) + ">>"
        if kind in ("integer", "real"):
            return num(o)
        if kind == "boolean":
            return "true" if bool(o) else "false"
        if kind == "null":
            return "null"
    if isinstance(o, str):
        return "(" + o.encode("utf-8", "surrogatepass").hex() + ")"
    return f"?{type(o).__name__}"


def op_line(op: content.Operation) -> str:
    if op.operator == "INLINE IMAGE":
        return f"BI|{op.depth}"
    ctm = ",".join(f"{v:.3f}" for v in op.ctm)
    return f"{op.operator}|{op.depth}|{ctm}|" + " ".join(canon(v) for v in op.operands)


def box(page, key):
    b = page.obj.get(key)
    return None if b is None else [round(float(v), 4) for v in b]


def record(path: pathlib.Path) -> dict:
    t0 = time.perf_counter()
    rec: dict = {"file": str(path)}
    try:
        pdf = pikepdf.open(path)
    except (pikepdf.PdfError, pikepdf.PasswordError) as exc:
        return rec | {"open": False, "err": type(exc).__name__}
    with pdf:
        rec["open"] = True
        rec["open_s"] = time.perf_counter() - t0
        rec["encrypted"] = pdf.is_encrypted
        objects, streams = {}, {}
        encrypt = pdf.trailer.get("/Encrypt").objgen if "/Encrypt" in pdf.trailer else None
        t1 = time.perf_counter()
        for o in pdf.objects:
            if not isinstance(o, pikepdf.Object) or o.objgen == (0, 0):
                continue
            key = f"{o.objgen[0]}.{o.objgen[1]}"
            kind = o._type_name
            if o.objgen == encrypt or (kind == "stream" and str(o.get("/Type")) == "/XRef"):
                continue  # plumbing no check reads, and not encrypted the way the rest of the file is
            try:
                objects[key] = hashlib.sha1(canon(o, top=True).encode()).hexdigest()[:12]
                if o._type_name == "stream":
                    f = o.get("/Filter")
                    names = [] if f is None else [str(x) for x in f] if f._type_name == "array" else [str(f)]
                    if IMAGE_FILTERS & set(names):
                        streams[key] = "raw:" + hashlib.sha1(bytes(o.read_raw_bytes())).hexdigest()[:12]
                    else:
                        streams[key] = hashlib.sha1(bytes(o.read_bytes())).hexdigest()[:12]
            except (pikepdf.PdfError, RuntimeError, ValueError) as exc:
                streams[key] = f"ERR:{type(exc).__name__}"
        rec["objects"], rec["streams"], rec["objects_s"] = objects, streams, time.perf_counter() - t1
        rec["npages"] = len(pdf.pages)
        rec["root"] = sorted(str(k) for k in pdf.Root.keys())
        rec["pages"] = [
            {
                "media": [round(float(v), 4) for v in p.mediabox],
                "crop": [round(float(v), 4) for v in p.cropbox],
                "trim": box(p, "/TrimBox"),
                "bleed": box(p, "/BleedBox"),
                "rotate": int(p.obj.get("/Rotate", 0)),
            }
            for p in pdf.pages
        ]
        t2 = time.perf_counter()
        walks = []
        for page in pdf.pages:
            h, n = hashlib.sha1(), 0
            try:
                for op in content.iter_operations(page):
                    h.update(op_line(op).encode() + b"\n")
                    n += 1
                walks.append([n, h.hexdigest()[:12]])
            except pikepdf.PdfError as exc:
                walks.append([n, f"ERR:{type(exc).__name__}"])
        rec["walks"], rec["walk_s"] = walks, time.perf_counter() - t2
    return rec


def main() -> None:
    if sys.argv[1] == "--dump-ops":
        with pikepdf.open(sys.argv[2]) as pdf:
            for op in content.iter_operations(pdf.pages[int(sys.argv[3])]):
                sys.stdout.write(op_line(op) + "\n")
        return
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError()))
    with open(sys.argv[1], "w") as out:
        for f in sys.argv[2:]:
            signal.alarm(120)
            try:
                out.write(json.dumps(record(pathlib.Path(f))) + "\n")
            except (TimeoutError, RecursionError, UnicodeError, ValueError, TypeError, OverflowError) as exc:
                sys.stderr.write(f"baseline skipped {f}: {type(exc).__name__}: {exc}\n")
            signal.alarm(0)
            out.flush()


main()
