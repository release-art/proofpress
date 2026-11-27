"""The PDF/X-4 structural layer and the finished-card geometry check."""

import dataclasses

import pikepdf
import pytest

from proof_pdf.models import PageGeometry, Profile, Severity

from .conftest import A6, BLEED, MEDIA_H, MEDIA_W, TRIM_H, TRIM_W, core_findings


def _checks(findings) -> set[str]:
    return {f.check for f in findings}


class TestGeometry:
    """The check that catches a valid PDF/X-4 file trimmed to the wrong size."""

    def test_correct_card_is_clean(self, make_pdf):
        with pikepdf.open(make_pdf()) as pdf:
            assert core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(A6)) == []

    def test_landscape_matches_without_a_flag(self, make_pdf):
        """Orientation-agnostic: the same two numbers the other way round."""
        boxes = {
            "MediaBox": [0, 0, MEDIA_H, MEDIA_W],
            "TrimBox": [BLEED, BLEED, BLEED + TRIM_H, BLEED + TRIM_W],
            "BleedBox": [0, 0, MEDIA_H, MEDIA_W],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            assert core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(A6)) == []

    def test_card_package_output_passes(self, make_pdf):
        """Measured on a real LaTeX card-package build: TrimBox inset
        19.84bp (the 7 mm crop-mark margin) on every side, BleedBox 3 mm outside it."""
        boxes = {
            "MediaBox": [0, 0, 337.32, 459.21],
            "BleedBox": [11.34, 11.34, 325.98, 447.87],
            "TrimBox": [19.84, 19.84, 317.48, 439.37],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            assert core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(A6)) == []

    def test_real_defect_from_a_build(self, make_pdf):
        """The measured output of the retired `subjectFlashcard.cls`: 25pt horizontal, 20pt vertical inset."""
        boxes = {
            "MediaBox": [0, 0, 337.32198, 459.21146],
            "TrimBox": [25, 20, 312.32198, 439.21146],
            "BleedBox": [0, 0, 337.32198, 459.21146],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            findings = core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(A6))
        assert "geometry" in _checks(findings)
        assert "101.3" in next(f.message for f in findings if f.check == "geometry")
        # Its bleed is the 7mm crop-mark margin, not the 1.5mm it should declare.
        assert "bleed" in _checks(findings)
        # It is wrongly sized but still symmetric, so centring must NOT fire.
        assert "centring" not in _checks(findings)

    def test_off_centre_trim_is_caught(self, make_pdf):
        boxes = {
            "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
            "TrimBox": [0, BLEED, TRIM_W, BLEED + TRIM_H],
            "BleedBox": [0, 0, MEDIA_W, MEDIA_H],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            assert "centring" in _checks(
                core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(A6))
            )

    @pytest.mark.parametrize(("tolerance_mm", "expect_finding"), [(0.25, True), (2.0, False)], ids=["tight", "loose"])
    def test_tolerance_is_honoured(self, make_pdf, tolerance_mm, expect_finding):
        one_mm = 72.0 / 25.4
        boxes = {
            "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
            "TrimBox": [BLEED, BLEED, BLEED + TRIM_W - one_mm, BLEED + TRIM_H],
            "BleedBox": [0, 0, MEDIA_W, MEDIA_H],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            findings = core_findings(
                pdf,
                {"geometry", "bleed", "centring"},
                geometry=dataclasses.asdict(dataclasses.replace(A6, tolerance_mm=tolerance_mm)),
            )
        assert ("geometry" in _checks(findings)) is expect_finding

    def test_other_stock_passes_with_its_own_numbers(self, make_pdf):
        """An 85x55 business card with 3mm bleed -- the cross-validation shape."""
        mm = 72.0 / 25.4
        trim_w, trim_h, bleed = 85 * mm, 55 * mm, 3 * mm
        boxes = {
            "MediaBox": [0, 0, trim_w + 2 * bleed, trim_h + 2 * bleed],
            "TrimBox": [bleed, bleed, bleed + trim_w, bleed + trim_h],
            "BleedBox": [0, 0, trim_w + 2 * bleed, trim_h + 2 * bleed],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            geometry = PageGeometry(trim_w_mm=85, trim_h_mm=55, bleed_mm=3)
            assert core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(geometry)) == []

    def test_missing_trim_is_left_to_check_page_boxes(self, make_pdf):
        with pikepdf.open(make_pdf(boxes={"MediaBox": [0, 0, MEDIA_W, MEDIA_H]})) as pdf:
            assert core_findings(pdf, {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(A6)) == []


def _with_intent(identifier: str | None, *, description_profile: bool = False) -> pikepdf.Pdf:
    pdf = pikepdf.new()
    pdf.add_blank_page(page_size=(100, 100))
    intent = pikepdf.Dictionary(Type=pikepdf.Name.OutputIntent, S=pikepdf.Name.GTS_PDFX)
    if identifier is not None:
        intent["/OutputConditionIdentifier"] = pikepdf.String(identifier)
    if description_profile:
        from PIL import ImageCms

        profile = pdf.make_stream(ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes())
        profile["/N"] = 3
        intent["/DestOutputProfile"] = pdf.make_indirect(profile)
    pdf.Root["/OutputIntents"] = pikepdf.Array([pdf.make_indirect(intent)])
    return pdf


class TestOutputCondition:
    def test_a_matching_identifier_passes_case_insensitively(self):
        assert (
            core_findings(_with_intent("FOGRA51 (PSO Coated v3)"), {"output-condition"}, output_condition="fogra51")
            == []
        )

    def test_a_different_condition_is_an_error_naming_both(self):
        [finding] = core_findings(_with_intent("sRGB IEC61966-2.1"), {"output-condition"}, output_condition="FOGRA51")
        assert finding.severity is Severity.ERROR and finding.check == "output-condition"
        assert "sRGB IEC61966-2.1" in finding.message and "FOGRA51" in finding.message

    def test_the_icc_profile_description_counts_too(self):
        pdf = _with_intent(None, description_profile=True)
        assert core_findings(pdf, {"output-condition"}, output_condition="sRGB") == []

    def test_no_output_intent_is_an_error(self):
        pdf = pikepdf.new()
        pdf.add_blank_page()
        [finding] = core_findings(pdf, {"output-condition"}, output_condition="FOGRA51")
        assert "no output intent" in finding.message


def _pages(*sizes_mm: tuple[float, float], rotate: dict[int, int] | None = None) -> pikepdf.Pdf:
    pdf = pikepdf.new()
    for i, (w, h) in enumerate(sizes_mm):
        pdf.add_blank_page(page_size=(w / 25.4 * 72, h / 25.4 * 72))
        if rotate and i in rotate:
            pdf.pages[i].obj["/Rotate"] = rotate[i]
    return pdf


class TestPageSet:
    def test_a_count_that_is_not_a_multiple_is_an_error(self):
        [finding] = core_findings(
            _pages(*[(105, 148)] * 10),
            {"page-count", "page-size", "page-rotate"},
            uniform_pages=False,
            pages_multiple_of=4 or None,
        )
        assert finding.check == "page-count" and "10 pages" in finding.message

    def test_a_multiple_passes(self):
        assert (
            core_findings(
                _pages(*[(105, 148)] * 8),
                {"page-count", "page-size", "page-rotate"},
                uniform_pages=False,
                pages_multiple_of=4 or None,
            )
            == []
        )

    def test_a_page_of_another_size_is_named(self):
        [finding] = core_findings(
            _pages((105, 148), (105, 148), (148, 210)),
            {"page-count", "page-size", "page-rotate"},
            uniform_pages=True,
            pages_multiple_of=None or None,
        )
        assert finding.check == "page-size" and finding.page == 3
        assert "148 x 210" in finding.message and "105 x 148" in finding.message

    def test_sizes_within_a_tenth_of_a_millimetre_are_the_same(self):
        assert (
            core_findings(
                _pages((105, 148), (105.05, 148.05)),
                {"page-count", "page-size", "page-rotate"},
                uniform_pages=True,
                pages_multiple_of=None or None,
            )
            == []
        )

    def test_a_rotated_page_is_a_warning(self):
        [finding] = core_findings(
            _pages((105, 148), (105, 148), rotate={1: 90}),
            {"page-count", "page-size", "page-rotate"},
            uniform_pages=True,
            pages_multiple_of=None or None,
        )
        assert finding.check == "page-rotate" and finding.severity is Severity.WARN and finding.page == 2

    def test_nothing_is_checked_unless_asked(self):
        assert (
            core_findings(
                _pages((105, 148), (148, 210)),
                {"page-count", "page-size", "page-rotate"},
                uniform_pages=False,
                pages_multiple_of=None or None,
            )
            == []
        )

    def test_the_trim_box_is_the_size_when_there_is_one(self):
        pdf = _pages((111, 154), (111, 154))
        for page in pdf.pages:
            page.obj["/TrimBox"] = [8.5, 8.5, 8.5 + 105 / 25.4 * 72, 8.5 + 148 / 25.4 * 72]
        assert (
            core_findings(
                pdf, {"page-count", "page-size", "page-rotate"}, uniform_pages=True, pages_multiple_of=None or None
            )
            == []
        )


class TestBindingBleed:
    """Book interiors: no bleed on the binding edge, which alternates from page to page."""

    BOOK = PageGeometry(trim_w_mm=105, trim_h_mm=148, bleed_mm=3, binding_bleed_mm=0)
    MM = 72 / 25.4

    def _pdf(self, *margins_mm: tuple[float, float]):
        """Pages of an A6 interior; each is given its (left, right) margin in mm, with 3 mm above and below the trim."""
        pdf = pikepdf.new()
        w, h = 105 * self.MM, 148 * self.MM
        for left, right in margins_mm:
            lm, bm = left * self.MM, 3 * self.MM
            media = [0, 0, lm + w + right * self.MM, bm + h + bm]
            boxes = {"MediaBox": media, "TrimBox": [lm, bm, lm + w, bm + h], "BleedBox": media}
            page = pikepdf.Dictionary(Type=pikepdf.Name.Page, Contents=pdf.make_stream(b""), **boxes)
            pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        return pdf

    def test_the_binding_edge_alternates_with_the_page(self):
        assert self.BOOK.bleeds_mm(1) == (0, 3, 3, 3) and self.BOOK.bleeds_mm(2) == (3, 3, 0, 3)
        right = PageGeometry(105, 148, 3, binding_bleed_mm=0, binding="right")
        assert right.bleeds_mm(1) == (3, 3, 0, 3) and right.bleeds_mm(2) == (0, 3, 3, 3)

    def test_a_correct_interior_is_clean(self):
        assert (
            core_findings(
                self._pdf((0, 3), (3, 0), (0, 3)),
                {"geometry", "bleed", "centring"},
                geometry=dataclasses.asdict(self.BOOK),
            )
            == []
        )

    def test_bleed_on_the_wrong_side_is_flagged_on_that_page_only(self):
        findings = core_findings(
            self._pdf((0, 3), (0, 3), (0, 3)), {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(self.BOOK)
        )  # page 2 repeats page 1
        assert {(f.check, f.page) for f in findings} == {("bleed", 2), ("centring", 2)}
        [bleed] = [f for f in findings if f.check == "bleed"]
        assert "expected 3, 3, 0, 3 mm" in bleed.message

    def test_bleed_on_the_binding_edge_is_flagged(self):
        assert "bleed" in {
            f.check
            for f in core_findings(
                self._pdf((3, 3)), {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(self.BOOK)
            )
        }

    def test_centring_follows_the_expected_asymmetry(self):
        [finding] = [
            f
            for f in core_findings(
                self._pdf((1.5, 3)), {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(self.BOOK)
            )
            if f.check == "centring"
        ]
        assert "expected left minus right -3.00 mm" in finding.message

    def test_without_a_binding_bleed_an_asymmetric_page_is_off_centre(self):
        [finding] = [
            f
            for f in core_findings(
                self._pdf((0, 3)),
                {"geometry", "bleed", "centring"},
                geometry=dataclasses.asdict(PageGeometry(105, 148, 3)),
            )
            if f.check == "centring"
        ]
        assert "expected left minus right" not in finding.message  # today's message

    def test_equal_bleed_is_exactly_the_old_behaviour(self):
        for geometry in (PageGeometry(105, 148, 3), PageGeometry(105, 148, 3, binding_bleed_mm=3, binding="right")):
            assert (
                core_findings(
                    self._pdf((3, 3), (3, 3)), {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(geometry)
                )
                == []
            )
            off = core_findings(
                self._pdf((3, 8)), {"geometry", "bleed", "centring"}, geometry=dataclasses.asdict(geometry)
            )
            assert {f.check for f in off} == {"bleed", "centring"}
            assert all("on every side" in f.message for f in off if f.check == "bleed")
            assert all("expected left minus right" not in f.message for f in off if f.check == "centring")

    def test_the_geometry_reaches_the_command(self):
        from proof_pdf import core

        geometry = PageGeometry(trim_w_mm=105, trim_h_mm=148, bleed_mm=3, binding_bleed_mm=0.0, binding="right")
        args = core.arguments(Profile(geometry=geometry))
        assert args[args.index("--binding") + 1] == "right"
        assert args[args.index("--binding-bleed-mm") + 1] == "0.0"
        assert "--binding-bleed-mm" not in core.arguments(Profile(geometry=PageGeometry(105, 148, 3)))
