"""L* contrast measured through ICC profiles: the model, the background rules and the findings."""

import io
import pathlib
import shutil
import subprocess

import pikepdf
import pytest
from PIL import ImageCms

from proof_pdf._internal import contrast
from proof_pdf._internal.paint import Fill
from proof_pdf.models import Severity


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


def _model() -> contrast.ColourModel:
    return contrast.ColourModel(cmyk=None)


def _cmyk_profile() -> pathlib.Path | None:
    candidates = [
        pathlib.Path("/usr/share/color/icc/ghostscript/default_cmyk.icc"),
        pathlib.Path("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"),
    ]
    if gs := shutil.which("gs"):
        root = pathlib.Path(gs).resolve().parent.parent
        candidates += list(root.glob("share/ghostscript/*/iccprofiles/default_cmyk.icc"))
        try:  # Homebrew keeps the install elsewhere than the symlink
            prefix = subprocess.run([gs, "-h"], capture_output=True, text=True).stdout
        except OSError:
            prefix = ""
        candidates += [pathlib.Path(p) / "default_cmyk.icc" for p in prefix.split() if p.endswith("iccprofiles")]
    return next((c for c in candidates if c.is_file()), None)


CMYK_PROFILE = _cmyk_profile()
needs_cmyk_profile = pytest.mark.skipif(CMYK_PROFILE is None, reason="no CMYK ICC profile on this machine")


class TestColourModel:
    def test_srgb_black_and_white_are_the_ends_of_the_scale(self):
        model = _model()
        black, white = model.lab(Fill("rgb", (0.0, 0.0, 0.0))), model.lab(Fill("rgb", (1.0, 1.0, 1.0)))
        assert black is not None and black[0] == pytest.approx(0, abs=0.6)
        assert white is not None and white[0] == pytest.approx(100, abs=0.6)

    def test_mid_gray_is_where_srgb_puts_it(self):
        lab = _model().lab(Fill("gray", (0.5,)))
        assert lab is not None and lab[0] == pytest.approx(53.4, abs=1.0)  # sRGB 50% gray: L* ~ 53.4

    def test_without_a_cmyk_profile_the_paper_is_white_rgb(self):
        assert _model().paper == Fill("rgb", (1.0, 1.0, 1.0))

    def test_cmyk_ink_needs_a_cmyk_profile(self):
        with pytest.raises(contrast.NoProfileError):
            _model().lab(Fill("cmyk", (0.0, 0.0, 0.0, 1.0)))

    def test_spots_and_patterns_have_no_lab(self):
        assert _model().lab(Fill("other")) is None

    @needs_cmyk_profile
    def test_a_cmyk_profile_measures_ink_on_paper(self):
        model = contrast.ColourModel.for_pdf(pikepdf.new(), CMYK_PROFILE)
        paper, black, mid = (model.lab(Fill("cmyk", v)) for v in [(0, 0, 0, 0), (0, 0, 0, 1.0), (0, 0, 0, 0.5)])
        assert paper and black and mid
        assert paper[0] > 90 and black[0] < 25
        assert black[0] < mid[0] < paper[0]
        assert model.paper == Fill("cmyk", (0.0, 0.0, 0.0, 0.0))

    @needs_cmyk_profile
    def test_gray_text_is_the_k_plate_when_there_is_a_cmyk_profile(self):
        model = contrast.ColourModel.for_pdf(pikepdf.new(), CMYK_PROFILE)
        gray, k = model.lab(Fill("gray", (0.0,))), model.lab(Fill("cmyk", (0, 0, 0, 1.0)))
        assert gray == k

    def test_the_output_intent_profile_is_used_when_there_is_one(self):
        pdf = pikepdf.new()
        srgb = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB"))
        buffer = io.BytesIO(srgb.tobytes())
        stream = pdf.make_stream(buffer.getvalue())
        stream["/N"] = 3
        intent = pikepdf.Dictionary(S=pikepdf.Name.GTS_PDFX, DestOutputProfile=pdf.make_indirect(stream))
        pdf.Root["/OutputIntents"] = pikepdf.Array([intent])
        assert contrast.ColourModel.for_pdf(pdf).lab(Fill("rgb", (1.0, 1.0, 1.0))) is not None


class TestBackgrounds:
    def _pairs(self, tmp_path, stream, **kw):
        with _page(tmp_path, stream, **kw) as pdf:
            return contrast.measure(pdf, _model())

    def test_text_on_bare_paper_is_measured_against_the_paper(self, tmp_path):
        [pair] = self._pairs(tmp_path, _text(b"0 g"))
        assert pair.on_paper and pair.delta_l == pytest.approx(100, abs=1.0) and pair.runs == 1

    def test_a_filled_rectangle_under_the_text_is_its_background(self, tmp_path):
        stream = b"0.9 g 50 50 150 150 re f " + _text(b"0.2 g")
        [pair] = self._pairs(tmp_path, stream)
        assert not pair.on_paper and pair.ground == Fill("gray", (0.9,))
        assert pair.delta_l is not None and 60 < pair.delta_l < 80

    def test_the_last_painted_shape_is_on_top(self, tmp_path):
        stream = b"0.9 g 50 50 150 150 re f 0.5 g 90 90 60 60 re f " + _text(b"0 g")
        [pair] = self._pairs(tmp_path, stream)
        assert pair.ground == Fill("gray", (0.5,))

    def test_a_shape_the_text_is_not_over_does_not_count(self, tmp_path):
        stream = b"0.5 g 200 200 50 50 re f " + _text(b"0 g")
        [pair] = self._pairs(tmp_path, stream)
        assert pair.on_paper

    def test_a_shape_painted_after_the_text_does_not_count(self, tmp_path):
        [pair] = self._pairs(tmp_path, _text(b"0 g") + b" 0.5 g 50 50 150 150 re f")
        assert pair.on_paper

    def test_text_over_an_image_is_unmeasured_not_passed(self, tmp_path):
        def image(pdf):
            stream = pdf.make_stream(b"\x80")
            stream["/Type"], stream["/Subtype"] = pikepdf.Name.XObject, pikepdf.Name.Image
            stream["/Width"], stream["/Height"], stream["/BitsPerComponent"] = 1, 1, 8
            stream["/ColorSpace"] = pikepdf.Name.DeviceGray
            return pdf.make_indirect(stream)

        stream = b"q 200 0 0 200 0 0 cm /Im1 Do Q " + _text(b"0 g")
        [pair] = self._pairs(tmp_path, stream, xobjects={"Im1": image})
        assert pair.ground is None and pair.delta_l is None

    def test_pairs_group_by_colour_pair_and_count_runs(self, tmp_path):
        stream = _text(b"0 g") + _text(b"0 g", b"100 150 Td") + _text(b"0.5 g", b"100 200 Td")
        pairs = self._pairs(tmp_path, stream)
        assert sorted(p.runs for p in pairs) == [1, 2]

    def test_ciede2000_is_reported_beside_l_star(self, tmp_path):
        [pair] = self._pairs(tmp_path, _text(b"0 g"))
        assert pair.delta_e is not None and pair.delta_e > 50


class TestFindings:
    def _findings(self, tmp_path, stream, minimum=30.0, **kw):
        with _page(tmp_path, stream, **kw) as pdf:
            return contrast.check_contrast(pdf, minimum)

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
            [finding] = contrast.check_contrast(pdf, 30.0, CMYK_PROFILE)
        assert "C0 M0 Y0 K20" in finding.message and "C0 M0 Y0 K10" in finding.message
