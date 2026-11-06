"""Build the colour sample PDFs from their LaTeX sources, for tests that run the checker over real LuaLaTeX output.

    pdm run python tests/fixtures/samples/build.py [name ...]        # rewrite pdfs/ (needs lualatex and gs)

Each ``NAME.tex`` here becomes ``pdfs/NAME.pdf``. The sources are the specification: ``expected.toml`` says what
``proof-pdf verify`` must report for each, in each colour mode. Both the sources and the PDFs are committed, and the
tests run on the PDFs, so CI needs no LaTeX. The PDFs depend on the TeX Live that built them and are not reproduced
byte for byte; instead ``pdfs/SOURCES.json`` records a digest of each source, and a test fails when a ``.tex`` changed
without its PDF being rebuilt.

The images and ICC profiles the sources refer to are made here: Pillow writes the rasters and the sRGB profile, a
second RGB profile and a second CMYK profile are the first ones under another description (so they are another profile
by identity), and the CMYK profile is Ghostscript's own ``default_cmyk.icc``, found on the machine.
"""

import concurrent.futures
import hashlib
import io
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

from PIL import Image, ImageCms

HERE = pathlib.Path(__file__).parent
PDFS = HERE / "pdfs"
# Samples that need a CMYK ICC profile, which only Ghostscript's install can supply.
NEEDS_CMYK_PROFILE = frozenset({"cmyk-image-profile", "cmyk-profile-mismatch"})


def names() -> list[str]:
    return sorted(p.stem for p in HERE.glob("*.tex") if p.stem != "pdfraw")


def digests() -> dict[str, str]:
    """A digest of each sample's source and the macro file every sample may use."""
    shared = (HERE / "pdfraw.tex").read_bytes()
    return {n: hashlib.sha256((HERE / f"{n}.tex").read_bytes() + shared).hexdigest() for n in names()}


def available() -> bool:
    return shutil.which("lualatex") is not None


def cmyk_profile() -> pathlib.Path | None:
    """Ghostscript's own CMYK profile, wherever this machine keeps it (the system's generic one only as a last resort).

    The committed sample PDFs embed this profile, so it is Ghostscript's and not an operating system's.
    """
    candidates: list[pathlib.Path] = []
    if gs := shutil.which("gs"):
        root = pathlib.Path(gs).resolve().parent.parent
        candidates += root.glob("share/ghostscript/*/iccprofiles/default_cmyk.icc")
        try:  # Homebrew keeps the install elsewhere than the symlink: take it from Ghostscript's own search path
            listing = subprocess.run([gs, "-h"], capture_output=True, text=True, check=False).stdout
        except OSError:
            listing = ""
        for token in listing.split():
            path = pathlib.Path(token.rstrip(":"))
            if path.parent.name == "Resource":  # .../share/ghostscript/Resource/Init
                candidates.append(path.parent.parent / "iccprofiles" / "default_cmyk.icc")
    candidates += [
        pathlib.Path("/usr/share/color/icc/ghostscript/default_cmyk.icc"),
        pathlib.Path("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"),
    ]
    return next((c for c in candidates if c.is_file()), None)


def _renamed(data: bytes) -> bytes:
    """*data* under another description, same length: the description is stored ASCII or UTF-16 and is reversed."""
    label = ImageCms.getProfileDescription(ImageCms.ImageCmsProfile(io.BytesIO(data))).strip()
    for encoding in ("ascii", "utf-16-be"):
        old = label.encode(encoding)
        if old in data and old != old[::-1]:
            return data.replace(old, label[::-1].encode(encoding), 1)
    raise ValueError(f"cannot find the description {label!r} in the profile")


def _assets(directory: pathlib.Path) -> None:
    Image.new("RGB", (64, 64), (200, 60, 30)).save(directory / "rgb.png")
    Image.new("CMYK", (64, 64), (0, 150, 220, 0)).save(directory / "cmyk.jpg", quality=95)
    Image.new("L", (64, 64), 128).save(directory / "gray.png")
    srgb = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    (directory / "srgb.icc").write_bytes(srgb)
    (directory / "rgb-other.icc").write_bytes(_renamed(srgb))
    if cmyk := cmyk_profile():
        (directory / "cmyk.icc").write_bytes(cmyk.read_bytes())
        (directory / "cmyk-other.icc").write_bytes(_renamed(cmyk.read_bytes()))


def _compile(name: str, work: pathlib.Path) -> pathlib.Path:
    result = subprocess.run(
        ["lualatex", "-interaction=nonstopmode", "-halt-on-error", f"{name}.tex"],
        cwd=work,
        capture_output=True,
        text=True,
        check=False,
        env={**os.environ, "SOURCE_DATE_EPOCH": "1700000000", "FORCE_SOURCE_DATE": "1"},
    )
    if result.returncode != 0:
        raise RuntimeError(f"lualatex failed on {name}.tex:\n{result.stdout[-2000:]}")
    return work / f"{name}.pdf"


def build(out_dir: pathlib.Path, wanted: list[str] | None = None) -> dict[str, pathlib.Path]:
    """Compile the samples (all, or *wanted*) into *out_dir*; returns name -> PDF.

    Samples that need a CMYK profile are left out when the machine has none.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    chosen = [n for n in names() if wanted is None or n in wanted]
    if cmyk_profile() is None:
        chosen = [n for n in chosen if n not in NEEDS_CMYK_PROFILE]
    with tempfile.TemporaryDirectory() as tmp:
        work = pathlib.Path(tmp)
        _assets(work)
        for source in HERE.glob("*.tex"):
            shutil.copy(source, work)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            built = dict(zip(chosen, pool.map(lambda n: _compile(n, work), chosen), strict=True))
        return {name: pathlib.Path(shutil.copy(pdf, out_dir / pdf.name)) for name, pdf in built.items()}


if __name__ == "__main__":
    if not available() or cmyk_profile() is None:
        sys.exit("building the samples needs lualatex and Ghostscript (for its CMYK profile) on PATH")
    wanted = sys.argv[1:] or None
    built = build(PDFS, wanted)
    recorded = json.loads((PDFS / "SOURCES.json").read_text()) if (PDFS / "SOURCES.json").is_file() else {}
    current = digests()
    (PDFS / "SOURCES.json").write_text(
        json.dumps({n: current[n] if n in built else recorded.get(n, "") for n in current}, indent=2) + "\n"
    )
    for sample, path in built.items():
        print(f"{sample}: {path}")
