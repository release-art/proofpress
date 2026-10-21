"""The PDF/X-4 structural layer and the finished-card geometry check."""

import dataclasses

import pikepdf
import pytest

from proof_pdf import structure
from proof_pdf.models import PageGeometry, Severity

from .conftest import A6, BLEED, GOOD_BOXES, MEDIA_H, MEDIA_W, TRIM_H, TRIM_W, xmp_bytes


def _checks(findings) -> set[str]:
    return {f.check for f in findings}


class TestPageBoxes:
    def test_correct_boxes_are_clean(self, make_pdf):
        with pikepdf.open(make_pdf()) as pdf:
            assert structure.check_page_boxes(pdf) == []

    @pytest.mark.parametrize(
        ("boxes", "expected_check", "expected_severity"),
        [
            pytest.param(
                {"MediaBox": [0, 0, MEDIA_W, MEDIA_H]},
                "trimbox",
                Severity.ERROR,
                id="no-trim-or-art",
            ),
            pytest.param(
                {**GOOD_BOXES, "ArtBox": GOOD_BOXES["TrimBox"]},
                "trimbox",
                Severity.ERROR,
                id="both-trim-and-art",
            ),
            pytest.param(
                {
                    "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
                    "TrimBox": [-10, -10, MEDIA_W + 10, MEDIA_H + 10],
                    "BleedBox": [0, 0, MEDIA_W, MEDIA_H],
                },
                "trimbox",
                Severity.ERROR,
                id="trim-beyond-media",
            ),
            pytest.param(
                {
                    "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
                    "TrimBox": [BLEED, BLEED, BLEED + TRIM_W, BLEED + TRIM_H],
                },
                "bleedbox",
                Severity.WARN,
                id="missing-bleed-is-advisory",
            ),
        ],
    )
    def test_violations(self, make_pdf, boxes, expected_check, expected_severity):
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            findings = structure.check_page_boxes(pdf)
        assert expected_check in _checks(findings)
        assert any(f.severity is expected_severity for f in findings if f.check == expected_check)

    def test_bleed_smaller_than_trim_fails(self, make_pdf):
        boxes = {
            "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
            "TrimBox": [0, 0, MEDIA_W, MEDIA_H],
            "BleedBox": [BLEED, BLEED, BLEED + TRIM_W, BLEED + TRIM_H],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            findings = structure.check_page_boxes(pdf)
        assert any("does not contain" in f.message for f in findings)

    def test_no_media_box(self, make_pdf):
        """Checked in memory: pikepdf re-supplies a default MediaBox on every save."""
        with pikepdf.open(make_pdf()) as pdf:
            del pdf.pages[0].obj["/MediaBox"]
            findings = structure.check_page_boxes(pdf)
        assert [f.check for f in findings] == ["boxes"]

    def test_media_box_inherited_from_page_tree(self, make_pdf):
        """MediaBox is the one inheritable box; a page without it walks to /Parent."""
        path = make_pdf(boxes={"TrimBox": GOOD_BOXES["TrimBox"], "BleedBox": GOOD_BOXES["BleedBox"]})
        with pikepdf.open(path, allow_overwriting_input=True) as pdf:
            pdf.Root.Pages["/MediaBox"] = [0, 0, MEDIA_W, MEDIA_H]
            pdf.save(path)
        with pikepdf.open(path) as pdf:
            assert structure.check_page_boxes(pdf) == []

    def test_malformed_rect_is_ignored(self, make_pdf):
        with pikepdf.open(make_pdf(boxes={**GOOD_BOXES, "TrimBox": [1, 2, 3]})) as pdf:
            findings = structure.check_page_boxes(pdf)
        assert "trimbox" in _checks(findings)


class TestTrapped:
    @pytest.mark.parametrize(
        ("value", "expect_finding"),
        [(pikepdf.Name("/True"), False), (pikepdf.Name("/False"), False), (pikepdf.Name("/Unknown"), True)],
    )
    def test_values(self, make_pdf, value, expect_finding):
        path = make_pdf()
        with pikepdf.open(path, allow_overwriting_input=True) as pdf:
            pdf.docinfo["/Trapped"] = value
            pdf.save(path)
        with pikepdf.open(path) as pdf:
            assert bool(structure.check_trapped(pdf)) is expect_finding

    def test_missing(self, make_pdf):
        with pikepdf.open(make_pdf()) as pdf:
            findings = structure.check_trapped(pdf)
        assert findings and "missing" in findings[0].message


class TestPdfxIdentity:
    def _build(self, path, *, xmp: bool, intent: bool):
        with pikepdf.open(path, allow_overwriting_input=True) as pdf:
            if xmp:
                meta = pdf.make_stream(xmp_bytes())
                meta["/Type"] = pikepdf.Name.Metadata
                pdf.Root["/Metadata"] = pdf.make_indirect(meta)
            if intent:
                pdf.Root["/OutputIntents"] = [
                    pdf.make_indirect(pikepdf.Dictionary(S=pikepdf.Name("/GTS_PDFX"), OutputCondition="FOGRA51"))
                ]
            pdf.save(path)

    def test_complete_identity_is_clean(self, make_pdf):
        path = make_pdf()
        self._build(path, xmp=True, intent=True)
        with pikepdf.open(path) as pdf:
            assert structure.check_pdfx_identity(pdf) == []

    def test_missing_both(self, make_pdf):
        with pikepdf.open(make_pdf()) as pdf:
            findings = structure.check_pdfx_identity(pdf)
        assert _checks(findings) == {"pdfx-xmp", "pdfx-intent"}

    def test_wrong_intent_subtype(self, make_pdf):
        path = make_pdf()
        with pikepdf.open(path, allow_overwriting_input=True) as pdf:
            pdf.Root["/OutputIntents"] = [
                pdf.make_indirect(pikepdf.Dictionary(S=pikepdf.Name("/GTS_PDFA1"), OutputCondition="sRGB"))
            ]
            pdf.save(path)
        with pikepdf.open(path) as pdf:
            assert "pdfx-intent" in _checks(structure.check_pdfx_identity(pdf))


class TestGeometry:
    """The check that catches a valid PDF/X-4 file trimmed to the wrong size."""

    def test_correct_card_is_clean(self, make_pdf):
        with pikepdf.open(make_pdf()) as pdf:
            assert structure.check_geometry(pdf, A6) == []

    def test_landscape_matches_without_a_flag(self, make_pdf):
        """Orientation-agnostic: the same two numbers the other way round."""
        boxes = {
            "MediaBox": [0, 0, MEDIA_H, MEDIA_W],
            "TrimBox": [BLEED, BLEED, BLEED + TRIM_H, BLEED + TRIM_W],
            "BleedBox": [0, 0, MEDIA_H, MEDIA_W],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            assert structure.check_geometry(pdf, A6) == []

    def test_card_package_output_passes(self, make_pdf):
        """Measured on a real LaTeX card-package build: TrimBox inset
        19.84bp (the 7 mm crop-mark margin) on every side, BleedBox 3 mm outside it."""
        boxes = {
            "MediaBox": [0, 0, 337.32, 459.21],
            "BleedBox": [11.34, 11.34, 325.98, 447.87],
            "TrimBox": [19.84, 19.84, 317.48, 439.37],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            assert structure.check_geometry(pdf, A6) == []

    def test_real_defect_from_a_build(self, make_pdf):
        """The measured output of the retired `subjectFlashcard.cls`: 25pt horizontal, 20pt vertical inset."""
        boxes = {
            "MediaBox": [0, 0, 337.32198, 459.21146],
            "TrimBox": [25, 20, 312.32198, 439.21146],
            "BleedBox": [0, 0, 337.32198, 459.21146],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            findings = structure.check_geometry(pdf, A6)
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
            assert "centring" in _checks(structure.check_geometry(pdf, A6))

    @pytest.mark.parametrize(("tolerance_mm", "expect_finding"), [(0.25, True), (2.0, False)], ids=["tight", "loose"])
    def test_tolerance_is_honoured(self, make_pdf, tolerance_mm, expect_finding):
        one_mm = 72.0 / 25.4
        boxes = {
            "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
            "TrimBox": [BLEED, BLEED, BLEED + TRIM_W - one_mm, BLEED + TRIM_H],
            "BleedBox": [0, 0, MEDIA_W, MEDIA_H],
        }
        with pikepdf.open(make_pdf(boxes=boxes)) as pdf:
            findings = structure.check_geometry(pdf, dataclasses.replace(A6, tolerance_mm=tolerance_mm))
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
            assert structure.check_geometry(pdf, geometry) == []

    def test_missing_trim_is_left_to_check_page_boxes(self, make_pdf):
        with pikepdf.open(make_pdf(boxes={"MediaBox": [0, 0, MEDIA_W, MEDIA_H]})) as pdf:
            assert structure.check_geometry(pdf, A6) == []


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
        assert structure.check_output_condition(_with_intent("FOGRA51 (PSO Coated v3)"), "fogra51") == []

    def test_a_different_condition_is_an_error_naming_both(self):
        [finding] = structure.check_output_condition(_with_intent("sRGB IEC61966-2.1"), "FOGRA51")
        assert finding.severity is Severity.ERROR and finding.check == "output-condition"
        assert "sRGB IEC61966-2.1" in finding.message and "FOGRA51" in finding.message

    def test_the_icc_profile_description_counts_too(self):
        pdf = _with_intent(None, description_profile=True)
        assert structure.check_output_condition(pdf, "sRGB") == []

    def test_no_output_intent_is_an_error(self):
        pdf = pikepdf.new()
        pdf.add_blank_page()
        [finding] = structure.check_output_condition(pdf, "FOGRA51")
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
        [finding] = structure.check_pages(_pages(*[(105, 148)] * 10), uniform=False, multiple_of=4)
        assert finding.check == "page-count" and "10 pages" in finding.message

    def test_a_multiple_passes(self):
        assert structure.check_pages(_pages(*[(105, 148)] * 8), uniform=False, multiple_of=4) == []

    def test_a_page_of_another_size_is_named(self):
        [finding] = structure.check_pages(_pages((105, 148), (105, 148), (148, 210)), uniform=True, multiple_of=None)
        assert finding.check == "page-size" and finding.page == 3
        assert "148 x 210" in finding.message and "105 x 148" in finding.message

    def test_sizes_within_a_tenth_of_a_millimetre_are_the_same(self):
        assert structure.check_pages(_pages((105, 148), (105.05, 148.05)), uniform=True, multiple_of=None) == []

    def test_a_rotated_page_is_a_warning(self):
        [finding] = structure.check_pages(
            _pages((105, 148), (105, 148), rotate={1: 90}), uniform=True, multiple_of=None
        )
        assert finding.check == "page-rotate" and finding.severity is Severity.WARN and finding.page == 2

    def test_nothing_is_checked_unless_asked(self):
        assert structure.check_pages(_pages((105, 148), (148, 210)), uniform=False, multiple_of=None) == []

    def test_the_trim_box_is_the_size_when_there_is_one(self):
        pdf = _pages((111, 154), (111, 154))
        for page in pdf.pages:
            page.obj["/TrimBox"] = [8.5, 8.5, 8.5 + 105 / 25.4 * 72, 8.5 + 148 / 25.4 * 72]
        assert structure.check_pages(pdf, uniform=True, multiple_of=None) == []
