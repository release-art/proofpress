"""Colour sample PDFs, one situation each, run through the checker in both colour modes.

The LaTeX sources are in ``tests/fixtures/samples`` and the PDFs built from them in ``tests/fixtures/pdfs`` with the
other fixtures; both are committed, and these tests use the PDFs, so they need no LaTeX. ``expected.toml`` says what
each must report. After editing a ``.tex``, rebuild with ``pdm run python tests/fixtures/samples/build.py`` (needs
lualatex and a CMYK profile); the freshness test fails until you do.
"""

import json
import pathlib
import shutil
import tomllib
import typing

import pytest

from proof_pdf import core
from proof_pdf.models import ColourMode, PageGeometry, Profile

from ._samples import build

EXPECTED = tomllib.loads((build.HERE / "expected.toml").read_text())


def test_every_source_has_expectations_and_a_pdf():
    assert set(build.names()) == set(EXPECTED)
    assert [n for n in build.names() if not (build.PDFS / f"{n}.pdf").is_file()] == []


def test_the_pdfs_were_built_from_the_current_sources():
    recorded = json.loads(build.MANIFEST.read_text())
    stale = sorted(name for name, digest in build.digests().items() if recorded.get(name) != digest)
    assert stale == [], "rebuild with: pdm run python tests/fixtures/samples/build.py"


def _profile(name: str, mode: ColourMode) -> Profile:
    extra: dict[str, typing.Any] = dict(EXPECTED[name].get("options", {}))
    if "allow_spots" in extra:
        extra["allow_spots"] = tuple(extra["allow_spots"])
    if "geometry" in extra:
        extra["geometry"] = PageGeometry(*extra["geometry"])
    # The standards layers are off unless a sample is built to pass them, and min_dpi is below any sample's: most are
    # 1 to 64 pixel images, and resolution is not what they are about.
    base: dict[str, typing.Any] = {"pdfa": False, "pdfx": False, "min_dpi": 0.1, "colour_mode": mode}
    return Profile(**(base | extra))


@pytest.mark.container
@pytest.mark.skipif(shutil.which("gs") is None, reason="needs Ghostscript on PATH (total ink, in CMYK mode)")
@pytest.mark.parametrize("mode", list(ColourMode))
@pytest.mark.parametrize("name", sorted(EXPECTED))
async def test_the_findings_are_the_expected_ones(name, mode):
    profile = _profile(name, mode)
    if profile.pdfa and shutil.which("verapdf") is None:
        pytest.skip("needs veraPDF on PATH (PDF/A layer)")
    report = await core.verify_file(pathlib.Path(build.PDFS / f"{name}.pdf"), profile)
    assert {f"{f.check}/{f.severity.value}" for f in report.findings} == set(EXPECTED[name][mode.value])
