"""bin/rust/archive.py WHEEL TARGET OUT_DIR   pack the program inside WHEEL as a release download.

WHEEL is the binary-only wheel `maturin build -b bin` makes from `rust/`. Writes
OUT_DIR/proof-pdf-<version>-<target>.tar.gz (.zip for Windows targets) holding `proof-pdf`, LICENSE, NOTICE and
README.md, so the download is byte for byte the program that wheel holds.
"""

import io
import pathlib
import sys
import tarfile
import zipfile

wheel, target, out_dir = pathlib.Path(sys.argv[1]), sys.argv[2], pathlib.Path(sys.argv[3])
version = wheel.name.split("-")[1]
with zipfile.ZipFile(wheel) as whl:
    files = {pathlib.PurePosixPath(n).name: whl.read(n) for n in whl.namelist() if ".data/scripts/" in n}
if not files:
    sys.exit(f"{wheel} holds no program")
docs = ("LICENSE", "NOTICE", "README.md")
files.update({name: pathlib.Path(name).read_bytes() for name in docs})
stem = f"proof-pdf-{version}-{target}"
out_dir.mkdir(parents=True, exist_ok=True)
if "windows" in target:
    out = out_dir / f"{stem}.zip"
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in sorted(files.items()):
            z.writestr(f"{stem}/{name}", data)
else:
    out = out_dir / f"{stem}.tar.gz"
    with tarfile.open(out, "w:gz") as tar:
        for name, data in sorted(files.items()):
            info = tarfile.TarInfo(f"{stem}/{name}")
            info.size, info.mode = len(data), 0o644 if name in docs else 0o755
            tar.addfile(info, io.BytesIO(data))
print(out)
