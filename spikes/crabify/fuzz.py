"""Flip, cut and splice bytes in the fixtures; the strict parser must answer every time, never panic or hang."""
import collections, glob, json, os, pathlib, random, re, subprocess, sys, tempfile
random.seed(7); B=os.environ.get("PARITY_BIN","spikes/crabify/parity/target/release/parity"); t=pathlib.Path(tempfile.mkdtemp())/"fuzz.pdf"
files=[f for f in glob.glob("tests/fixtures/pdfs/*.pdf") if pathlib.Path(f).stat().st_size<400_000]
c=collections.Counter(); seen=collections.Counter()
for i in range(int(sys.argv[1])):
    b=bytearray(open(random.choice(files),"rb").read())
    for _ in range(random.choice([1,4,16,64])):
        k=random.random(); p=random.randrange(len(b))
        if k<.5: b[p]=random.randrange(256)
        elif k<.75: b[p]=random.choice(b"0123456789 /<>[]()R")
        elif k<.9: del b[p:p+random.randrange(1,40)]
        else: b[p:p]=b[random.randrange(len(b)):][:random.randrange(1,40)]
    t.write_bytes(b)
    try: p=subprocess.run([B,"strict",str(t)],input="",capture_output=True,text=True,timeout=20)
    except subprocess.TimeoutExpired: c["HANG"]+=1; continue
    if p.returncode:
        c["CRASH"]+=1
        m=re.search(r"panicked at ([^\n]*)\n([^\n]*)",p.stderr); seen[(m.group(1)+" "+m.group(2))[:110] if m else p.stderr.strip()[-90:]]+=1
    else: c["answered: "+("opened" if json.loads(p.stdout)["open"] else "refused")]+=1
print(dict(c)); [print("  ",n,k) for k,n in seen.most_common(10)]
