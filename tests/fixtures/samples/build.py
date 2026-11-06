"""Build the colour sample PDFs from their LaTeX sources, for tests that run the checker over real LuaLaTeX output.

    pdm run python tests/fixtures/samples/build.py [name ...]    # writes ../pdfs (needs lualatex and a CMYK profile)

Each ``NAME.tex`` here becomes ``../pdfs/NAME.pdf``, beside the other fixture PDFs. The sources are the specification:
``expected.toml`` says what ``proof-pdf verify`` must report for each, in each colour mode. Both the sources and the
PDFs are committed, and the tests run on the PDFs, so CI needs no LaTeX. The PDFs depend on the TeX Live that built
them and are not reproduced byte for byte; instead ``SOURCES.json`` records a digest of each source, and a test fails
when a ``.tex`` changed without its PDF being rebuilt.

The images and ICC profiles the sources refer to are made here: Pillow writes the rasters and the sRGB profile, a
second RGB profile and a second CMYK profile are the first ones under another description (so they are another profile
by identity), and the CMYK profile is FOGRA39L from TeX Live's ``colorprofiles`` (else Ghostscript's own).
"""

import concurrent.futures
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

from PIL import Image, ImageCms

HERE = pathlib.Path(__file__).parent
PDFS = HERE.parent / "pdfs"  # every committed fixture PDF lives in tests/fixtures/pdfs
MANIFEST = HERE / "SOURCES.json"
# Samples that need a CMYK ICC profile, which only Ghostscript's install can supply.
NEEDS_CMYK_PROFILE = frozenset({"cmyk-image-profile", "cmyk-profile-mismatch"})


def names() -> list[str]:
    return sorted(p.stem for p in HERE.glob("*.tex") if p.stem != "pdfraw")


def digests() -> dict[str, str]:
    """A digest of each sample's source and the macro file every sample may use."""
    shared = (HERE / "pdfraw.tex").read_bytes()

    def source(n: str) -> bytes:
        xmp = HERE / f"{n}.xmpdata"
        return (HERE / f"{n}.tex").read_bytes() + (xmp.read_bytes() if xmp.is_file() else b"")

    return {n: hashlib.sha256(source(n) + shared).hexdigest() for n in names()}


def available() -> bool:
    return shutil.which("lualatex") is not None


def tex_profile(name: str) -> pathlib.Path | None:
    """An ICC profile shipped in TeX Live's ``colorprofiles`` package (sRGB.icc, FOGRA39L_coated.icc)."""
    found = subprocess.run(["kpsewhich", name], capture_output=True, text=True, check=False).stdout.strip()
    return pathlib.Path(found) if found else None


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


def sample_cmyk_profile() -> pathlib.Path | None:
    """The CMYK profile the samples embed: FOGRA39L from TeX Live (a real press condition), else Ghostscript's."""
    return tex_profile("FOGRA39L_coated.icc") or cmyk_profile()


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
    alpha = Image.linear_gradient("L").resize((64, 64))
    Image.merge("RGBA", (*Image.new("RGB", (64, 64), (30, 120, 200)).split(), alpha)).save(directory / "rgba.png")
    srgb = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    (directory / "srgb.icc").write_bytes(srgb)
    (directory / "rgb-other.icc").write_bytes(_renamed(srgb))
    if cmyk := sample_cmyk_profile():
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


def _needs(name: str) -> list[str]:
    """The samples a source includes as figures, from its ``% needs: a b`` comment lines."""
    text = (HERE / f"{name}.tex").read_text()
    return [n for m in re.finditer(r"^% needs:\s*(.*)$", text, re.MULTILINE) for n in m.group(1).split()]


def _waves(chosen: list[str]) -> list[list[str]]:
    """*chosen* in groups that can compile together: each sample after the ones it needs."""
    waves: list[list[str]] = []
    done: set[str] = set()
    pending = list(chosen)
    while pending:
        ready = [n for n in pending if all(d in done or d not in chosen for d in _needs(n))]
        if not ready:
            raise ValueError(f"a cycle among the samples' needs: {pending}")
        waves.append(ready)
        done.update(ready)
        pending = [n for n in pending if n not in ready]
    return waves


def build(out_dir: pathlib.Path, wanted: list[str] | None = None) -> dict[str, pathlib.Path]:
    """Compile the samples (all, or *wanted*) into *out_dir*; returns name -> PDF.

    Samples that need a CMYK profile are left out when the machine has none.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    chosen = [n for n in names() if wanted is None or n in wanted]
    if sample_cmyk_profile() is None:
        chosen = [n for n in chosen if n not in NEEDS_CMYK_PROFILE]
    with tempfile.TemporaryDirectory() as tmp:
        work = pathlib.Path(tmp)
        _assets(work)
        for source in (*HERE.glob("*.tex"), *HERE.glob("*.xmpdata")):  # pdfx reads NAME.xmpdata
            shutil.copy(source, work)
        built: dict[str, pathlib.Path] = {}
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            for wave in _waves(chosen):
                for name, pdf in zip(wave, pool.map(lambda n: _compile(n, work), wave), strict=True):
                    built[name] = pdf  # in work/, where a later sample may \includegraphics it
        return {name: pathlib.Path(shutil.copy(pdf, out_dir / pdf.name)) for name, pdf in built.items()}


if __name__ == "__main__":
    if not available() or sample_cmyk_profile() is None:
        sys.exit("building the samples needs lualatex, and TeX Live's colorprofiles or Ghostscript for a CMYK profile")
    wanted = sys.argv[1:] or None
    built = build(PDFS, wanted)
    recorded = json.loads(MANIFEST.read_text()) if MANIFEST.is_file() else {}
    current = digests()
    MANIFEST.write_text(
        json.dumps({n: current[n] if n in built else recorded.get(n, "") for n in current}, indent=2) + "\n"
    )
    for sample, path in built.items():
        print(f"{sample}: {path}")
