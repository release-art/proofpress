"""Colour sample PDFs, one situation each, run through the checker in both colour modes.

The LaTeX sources and the PDFs built from them are both committed in ``tests/fixtures/samples``, and these tests use
the PDFs, so they need no LaTeX. ``expected.toml`` says what each must report. After editing a ``.tex``, rebuild with
``pdm run python tests/fixtures/samples/build.py`` (needs lualatex and Ghostscript); the freshness test fails until
you do.
"""

import json
import pathlib
import shutil
import tomllib

import pytest

from proof_pdf import core
from proof_pdf.models import ColourMode, Profile

from ._samples import build

EXPECTED = tomllib.loads((build.HERE / "expected.toml").read_text())


def test_every_source_has_expectations_and_a_pdf():
    assert set(build.names()) == set(EXPECTED) == {p.stem for p in build.PDFS.glob("*.pdf")}


def test_the_pdfs_were_built_from_the_current_sources():
    recorded = json.loads((build.PDFS / "SOURCES.json").read_text())
    stale = sorted(name for name, digest in build.digests().items() if recorded.get(name) != digest)
    assert stale == [], "rebuild with: pdm run python tests/fixtures/samples/build.py"


@pytest.mark.container
@pytest.mark.skipif(shutil.which("gs") is None, reason="needs Ghostscript on PATH (total ink, in CMYK mode)")
@pytest.mark.parametrize("mode", list(ColourMode))
@pytest.mark.parametrize("name", sorted(EXPECTED))
async def test_the_findings_are_the_expected_ones(name, mode):
    spots = EXPECTED[name].get("options", {}).get("allow_spots")  # the one Profile field a sample sets
    # min_dpi is below any sample's: they are 1 to 64 pixel images, and resolution is not what they are about.
    profile = Profile(
        pdfa=False,
        pdfx=False,
        min_dpi=0.1,
        colour_mode=mode,
        allow_spots=None if spots is None else tuple(spots),
    )
    report = await core.verify_file(pathlib.Path(build.PDFS / f"{name}.pdf"), profile)
    assert {f"{f.check}/{f.severity.value}" for f in report.findings} == set(EXPECTED[name][mode.value])
