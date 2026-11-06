"""Effective placed resolution of image XObjects."""

import pikepdf
import pytest

from proof_pdf._internal import resolution
from proof_pdf.models import Severity


def _one_page(pdf: pikepdf.Pdf, stream: bytes, resources: pikepdf.Dictionary) -> None:
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page,
        MediaBox=[0, 0, 1000, 1000],
        Contents=pdf.make_stream(stream),
        Resources=resources,
    )
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))


@pytest.fixture
def placed_image(tmp_path, make_image):
    """A PDF placing one image of *pixels* px across *points* pt of page."""

    def _make(*, pixels: int = 300, points: float = 72.0, stream: bytes | None = None, name: str = "dpi.pdf"):
        path = tmp_path / name
        pdf = pikepdf.new()
        image = pdf.make_indirect(make_image(pdf, width=pixels, height=pixels))
        content = stream if stream is not None else b"q %f 0 0 %f 0 0 cm /Im1 Do Q" % (points, points)
        _one_page(pdf, content, pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=image)))
        pdf.save(path)
        return path

    return _make


class TestEffectiveDpi:
    def test_float_rounding_at_the_minimum_is_not_a_shortfall(self, placed_image):
        """74 px over 17.76 pt measures 299.99999999999994 dpi in floats; it was prepared for exactly 300."""
        with pikepdf.open(placed_image(pixels=74, points=17.76)) as pdf:
            assert resolution.check_image_dpi(pdf, 300.0) == []

    def test_a_real_shortfall_still_counts(self, placed_image):
        with pikepdf.open(placed_image(pixels=424, points=102.0)) as pdf:  # 299.6 dpi
            assert resolution.check_image_dpi(pdf, 300.0)

    @pytest.mark.parametrize(
        ("pixels", "points", "expected_dpi"),
        [
            pytest.param(300, 72.0, 300.0, id="exactly-300"),
            pytest.param(600, 72.0, 600.0, id="600-in-one-inch"),
            pytest.param(300, 144.0, 150.0, id="stretched-to-two-inches"),
            pytest.param(150, 36.0, 300.0, id="half-inch-at-300"),
        ],
    )
    def test_dpi_is_pixels_over_placed_inches(self, placed_image, pixels, points, expected_dpi):
        with pikepdf.open(placed_image(pixels=pixels, points=points)) as pdf:
            findings = resolution.check_image_dpi(pdf, expected_dpi + 1)
        assert findings, "expected a finding at a threshold just above the measured dpi"
        assert f"{expected_dpi:.0f} dpi" in findings[0].message

    def test_at_or_above_the_minimum_is_clean(self, placed_image):
        with pikepdf.open(placed_image(pixels=600, points=72.0)) as pdf:
            assert resolution.check_image_dpi(pdf, 300.0) == []

    def test_below_the_minimum_is_an_error(self, placed_image):
        with pikepdf.open(placed_image(pixels=150, points=72.0)) as pdf:
            findings = resolution.check_image_dpi(pdf, 300.0)
        assert [f.severity for f in findings] == [Severity.ERROR]
        assert "150 dpi" in findings[0].message

    def test_worst_axis_wins(self, placed_image):
        """A square image stretched on one axis only is judged by the soft axis."""
        stream = b"q 72 0 0 288 0 0 cm /Im1 Do Q"  # 1in wide, 4in tall
        with pikepdf.open(placed_image(pixels=300, stream=stream)) as pdf:
            findings = resolution.check_image_dpi(pdf, 300.0)
        assert findings and "75 dpi" in findings[0].message

    def test_worst_placement_of_a_repeated_image_is_reported(self, placed_image):
        stream = b"q 72 0 0 72 0 0 cm /Im1 Do Q q 288 0 0 288 0 0 cm /Im1 Do Q"
        with pikepdf.open(placed_image(pixels=300, stream=stream)) as pdf:
            findings = resolution.check_image_dpi(pdf, 300.0)
        assert len(findings) == 1
        assert "75 dpi" in findings[0].message

    def test_image_inside_a_form_is_measured_through_the_form_matrix(self, tmp_path, make_image, make_form):
        """A 300dpi image placed inside a form scaled 4x is really 75dpi."""
        path = tmp_path / "nested.pdf"
        pdf = pikepdf.new()
        image = pdf.make_indirect(make_image(pdf, width=300, height=300))
        form = pdf.make_indirect(
            make_form(
                pdf,
                b"q 72 0 0 72 0 0 cm /Im1 Do Q",
                matrix=[4, 0, 0, 4, 0, 0],
                resources=pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=image)),
            )
        )
        _one_page(pdf, b"/Fm1 Do", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            findings = resolution.check_image_dpi(opened, 300.0)
        assert findings and "75 dpi" in findings[0].message

    def test_forms_are_not_measured_as_images(self, tmp_path, make_form):
        path = tmp_path / "form-only.pdf"
        pdf = pikepdf.new()
        form = pdf.make_indirect(make_form(pdf, b""))
        _one_page(pdf, b"q 1 0 0 1 0 0 cm /Fm1 Do Q", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert resolution.check_image_dpi(opened, 300.0) == []

    def test_hairline_placement_is_not_measured(self, placed_image):
        """A sub-point placement would divide by ~0 and report a nonsense dpi."""
        with pikepdf.open(placed_image(pixels=10, stream=b"q 0.4 0 0 0.4 0 0 cm /Im1 Do Q")) as pdf:
            assert resolution.check_image_dpi(pdf, 300.0) == []

    def test_missing_dimensions_are_skipped(self, tmp_path, make_image):
        path = tmp_path / "nodims.pdf"
        pdf = pikepdf.new()
        image = make_image(pdf, width=10, height=10)
        del image["/Width"]
        _one_page(
            pdf,
            b"q 72 0 0 72 0 0 cm /Im1 Do Q",
            pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=pdf.make_indirect(image))),
        )
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert resolution.check_image_dpi(opened, 300.0) == []

    def test_unresolvable_name_is_skipped(self, tmp_path):
        path = tmp_path / "dangling.pdf"
        pdf = pikepdf.new()
        _one_page(pdf, b"q 72 0 0 72 0 0 cm /Missing Do Q", pikepdf.Dictionary())
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert resolution.check_image_dpi(opened, 300.0) == []


def test_inline_images_are_measured_too(tmp_path):
    pdf = pikepdf.new()
    stream = b"q 72 0 0 72 0 0 cm BI /W 100 /H 100 /CS /G /BPC 8 ID " + b"\x00" * 10000 + b" EI Q"
    _one_page(pdf, stream, pikepdf.Dictionary())
    [finding] = resolution.check_image_dpi(pdf, 300)
    assert finding.severity is Severity.ERROR and "(inline)" in finding.message and "100 dpi" in finding.message


class TestLimits:
    """The opt-in limits: warn tier, ceiling, stretch, and where the worst placement is."""

    def _check(self, placed_image, stream=None, *, pixels=300, min_dpi=200.0, **limits):
        with pikepdf.open(placed_image(pixels=pixels, stream=stream)) as pdf:
            return resolution.check_image_dpi(pdf, min_dpi, **limits)

    def test_everything_is_off_until_given(self, placed_image):
        assert self._check(placed_image, pixels=3000) == []

    def test_the_warn_tier_sits_between_the_minimum_and_its_level(self, placed_image):
        [finding] = self._check(placed_image, pixels=250, warn_dpi=300.0)  # 250 dpi: above 200, below 300
        assert (finding.check, finding.severity) == ("image-dpi", Severity.WARN)
        assert "250 dpi" in finding.message and "advisory" in finding.message
        assert self._check(placed_image, pixels=300, warn_dpi=300.0) == []  # at the level is fine
        [error] = self._check(placed_image, pixels=100, warn_dpi=300.0)  # below the minimum stays the one error
        assert error.severity is Severity.ERROR

    def test_the_ceiling_warns_above_it_only(self, placed_image):
        [finding] = self._check(placed_image, pixels=900, max_dpi=600.0)
        assert (finding.check, finding.severity) == ("image-dpi-max", Severity.WARN)
        assert "900 dpi" in finding.message and "file size" in finding.message
        assert self._check(placed_image, pixels=600, max_dpi=600.0) == []

    def test_stretch_is_the_larger_axis_over_the_smaller(self, placed_image):
        stream = b"q 72 0 0 144 0 0 cm /Im1 Do Q"  # 300 dpi across, 150 up: a 100% stretch
        assert self._check(placed_image, stream, min_dpi=100.0, max_stretch_pct=99.0)
        assert self._check(placed_image, stream, min_dpi=100.0, max_stretch_pct=101.0) == []
        [finding] = self._check(placed_image, stream, min_dpi=100.0, max_stretch_pct=50.0)
        assert (finding.check, finding.severity) == ("image-stretch", Severity.WARN)
        assert "100%" in finding.message and "300 x 150 dpi" in finding.message

    def test_a_uniform_scale_is_not_a_stretch(self, placed_image):
        assert self._check(placed_image, max_stretch_pct=0.0) == []

    def test_the_message_says_where_the_image_sits_and_both_axes_when_they_differ(self, placed_image):
        stream = b"q 72 0 0 288 28.3465 56.693 cm /Im1 Do Q"  # 300 x 75 dpi at (10, 20) mm
        [finding] = self._check(placed_image, stream)
        assert "75 dpi (300 x 75 dpi) at 10.0, 20.0 mm from the page's bottom-left" in finding.message
        [uniform] = self._check(placed_image, b"q 144 0 0 144 28.3465 56.693 cm /Im1 Do Q")
        assert (
            "150 dpi at 10.0, 20.0 mm" in uniform.message and "x" not in uniform.message.split("at")[1].split("mm")[0]
        )

    def test_the_worst_placement_is_named_and_the_first_wins_a_tie(self, placed_image):
        stream = (
            b"q 72 0 0 72 0 0 cm /Im1 Do Q q 144 0 0 144 28.3465 0 cm /Im1 Do Q q 144 0 0 144 56.693 0 cm /Im1 Do Q"
        )
        [finding] = self._check(placed_image, stream)
        assert "150 dpi at 10.0, 0.0 mm" in finding.message

    def test_the_ceiling_names_the_sharpest_placement(self, placed_image):
        stream = b"q 72 0 0 72 0 0 cm /Im1 Do Q q 36 0 0 36 72 0 cm /Im1 Do Q"
        [finding] = self._check(placed_image, stream, max_dpi=500.0)
        assert "600 dpi" in finding.message

    def test_inline_images_are_measured_too(self, tmp_path):
        pdf = pikepdf.new()
        _one_page(
            pdf,
            b"q 36 0 0 36 0 0 cm BI /W 100 /H 100 /CS /G /BPC 8 ID " + bytes(10000) + b" EI Q",
            pikepdf.Dictionary(),
        )
        [finding] = resolution.check_image_dpi(pdf, 200.0, max_dpi=100.0)
        assert "(inline)" in finding.message and "200 dpi" in finding.message
