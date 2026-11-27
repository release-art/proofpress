"""The contrast check's findings: L* contrast measured through ICC profiles (the model and the background rules are
tested in the core, ``checks/contrast.rs``)."""

import pathlib

import pikepdf
import pytest

from proof_pdf.models import Severity

from ._samples import build
from .conftest import ported_findings


def _page(tmp_path: pathlib.Path, stream: bytes, *, xobjects=None) -> pikepdf.Pdf:
    pdf = pikepdf.new()
    font = pdf.make_indirect(
        pikepdf.Dictionary(
            Type=pikepdf.Name.Font,
            Subtype=pikepdf.Name.Type1,
            BaseFont=pikepdf.Name("/Face"),
            FirstChar=65,
            Widths=[1000],
        )
    )
    resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font))
    if xobjects:
        resources["/XObject"] = pikepdf.Dictionary({f"/{k}": v(pdf) for k, v in xobjects.items()})
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page, MediaBox=[0, 0, 300, 300], Contents=pdf.make_stream(stream), Resources=resources
    )
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
    path = tmp_path / "t.pdf"
    pdf.save(path)
    return pikepdf.open(path)


def _text(colour: bytes, at: bytes = b"100 100 Td") -> bytes:
    return b"BT /F1 20 Tf " + colour + b" " + at + b" (A) Tj ET"


CMYK_PROFILE = build.cmyk_profile()
needs_cmyk_profile = pytest.mark.skipif(CMYK_PROFILE is None, reason="no CMYK ICC profile on this machine")


class TestFindings:
    def _findings(self, tmp_path, stream, minimum=30.0, **kw):
        with _page(tmp_path, stream, **kw) as pdf:
            return ported_findings(pdf, {"contrast"}, min_delta_l=minimum)

    def test_low_contrast_text_is_an_error_with_both_colours_named(self, tmp_path):
        [finding] = self._findings(tmp_path, b"0.9 g 50 50 150 150 re f " + _text(b"0.8 g"))
        assert finding.severity is Severity.ERROR and finding.check == "contrast" and finding.page == 1
        assert "gray 80%" in finding.message and "gray 90%" in finding.message and "minimum 30" in finding.message

    def test_good_contrast_passes(self, tmp_path):
        assert self._findings(tmp_path, _text(b"0 g")) == []

    def test_the_threshold_is_the_callers(self, tmp_path):
        stream = b"0.9 g 50 50 150 150 re f " + _text(b"0.6 g")
        assert self._findings(tmp_path, stream, minimum=10) == []
        assert len(self._findings(tmp_path, stream, minimum=60)) == 1

    def test_unmeasurable_text_is_a_warning_once_per_page(self, tmp_path):
        stream = b"BT /F1 20 Tf /Pattern cs /P1 scn 100 100 Td (A) Tj ET"
        [finding] = self._findings(tmp_path, stream)
        assert finding.severity is Severity.WARN and "not measured" in finding.message

    def test_cmyk_text_without_a_cmyk_profile_is_an_error(self, tmp_path):
        [finding] = self._findings(tmp_path, _text(b"0 0 0 1 k"))
        assert finding.severity is Severity.ERROR and "no CMYK profile" in finding.message and finding.page is None

    @needs_cmyk_profile
    def test_cmyk_text_is_measured_through_a_given_profile(self, tmp_path):
        with _page(tmp_path, b"0 0 0 0.1 k 50 50 150 150 re f " + _text(b"0 0 0 0.2 k")) as pdf:
            [finding] = ported_findings(pdf, {"contrast"}, min_delta_l=30.0, proof_profile=str(CMYK_PROFILE))
        assert "C0 M0 Y0 K20" in finding.message and "C0 M0 Y0 K10" in finding.message
