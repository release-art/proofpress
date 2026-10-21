"""Missing glyphs, size floors, hairlines and the safe area."""

import pathlib

import pikepdf
import pytest

from proof_pdf import text
from proof_pdf.models import Severity

# A6 trim at (8.5, 8.5)-(306, 428) inside a 3 mm bleed, like the conftest cards.
MM = 72.0 / 25.4
BOXES = {
    "MediaBox": [0, 0, 105 * MM + 6 * MM, 148 * MM + 6 * MM],
    "TrimBox": [3 * MM, 3 * MM, 3 * MM + 105 * MM, 3 * MM + 148 * MM],
}


def _page(tmp_path: pathlib.Path, stream: bytes, *, fonts: dict | None = None) -> pikepdf.Pdf:
    pdf = pikepdf.new()
    resources = pikepdf.Dictionary()
    if fonts:
        resources["/Font"] = pikepdf.Dictionary({f"/{k}": pdf.make_indirect(v) for k, v in fonts.items()})
    page = pikepdf.Dictionary(Type=pikepdf.Name.Page, Contents=pdf.make_stream(stream), Resources=resources, **BOXES)
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
    path = tmp_path / "t.pdf"
    pdf.save(path)
    return pikepdf.open(path)


def _simple(**entries) -> pikepdf.Dictionary:
    return pikepdf.Dictionary(
        Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name("/Face"), **entries
    )


def _identity(descendant: pikepdf.Dictionary | None = None) -> pikepdf.Dictionary:
    cid = descendant or pikepdf.Dictionary(Subtype=pikepdf.Name.CIDFontType2, FontDescriptor=pikepdf.Dictionary())
    return pikepdf.Dictionary(
        Type=pikepdf.Name.Font,
        Subtype=pikepdf.Name.Type0,
        BaseFont=pikepdf.Name("/ABCDEF+Nunito"),
        Encoding=pikepdf.Name("/Identity-H"),
        DescendantFonts=[cid],
    )


def _wide() -> pikepdf.Dictionary:
    return _simple(FirstChar=65, Widths=[1000, 1000, 1000])  # A, B, C: 1 em each


class TestMissingGlyphs:
    def test_notdef_in_an_identity_font_is_an_error_naming_the_font(self, tmp_path):
        with _page(tmp_path, b"BT /F1 12 Tf 50 50 Td <000000410000> Tj ET", fonts={"F1": _identity()}) as pdf:
            [finding] = text.check_missing_glyphs(pdf)
        assert finding.severity is Severity.ERROR and finding.check == "missing-glyph"
        assert "2 character(s)" in finding.message and "Nunito" in finding.message and finding.page == 1

    def test_ordinary_text_is_clean(self, tmp_path):
        with _page(tmp_path, b"BT /F1 12 Tf 50 50 Td <00410042> Tj ET", fonts={"F1": _identity()}) as pdf:
            assert text.check_missing_glyphs(pdf) == []

    def test_counts_are_per_font_across_runs(self, tmp_path):
        stream = b"BT /F1 12 Tf <0000> Tj /F2 12 Tf <0000> Tj <0000> Tj ET"
        fonts = {"F1": _identity(), "F2": _identity()}
        with _page(tmp_path, stream, fonts=fonts) as pdf:
            [finding] = text.check_missing_glyphs(pdf)
        assert "3 character(s)" in finding.message


class TestMinTextSize:
    def test_text_below_the_floor_is_flagged_with_its_effective_size(self, tmp_path):
        with _page(tmp_path, b"BT /F1 12 Tf 0.5 0 0 0.5 50 50 Tm (A) Tj ET", fonts={"F1": _wide()}) as pdf:
            [finding] = text.check_min_text_size(pdf, 7.0)
        assert "6.0pt" in finding.message and "7pt minimum" in finding.message

    def test_text_at_or_above_the_floor_passes(self, tmp_path):
        with _page(tmp_path, b"BT /F1 7 Tf 50 50 Td (A) Tj ET", fonts={"F1": _wide()}) as pdf:
            assert text.check_min_text_size(pdf, 7.0) == []


class TestHairlines:
    @pytest.mark.parametrize(
        ("stream", "expect"),
        [
            pytest.param(b"0 w 0 0 m 10 0 l S", "hairline", id="zero-width"),
            pytest.param(b"0.1 w 0 0 m 10 0 l S", "0.10pt", id="thin"),
        ],
    )
    def test_thin_lines_are_flagged(self, tmp_path, stream, expect):
        with _page(tmp_path, stream) as pdf:
            [finding] = text.check_min_stroke(pdf, 0.25)
        assert expect in finding.message

    def test_a_proper_line_and_a_fill_are_not(self, tmp_path):
        with _page(tmp_path, b"1 w 0 0 m 10 0 l S 0 w 0 0 5 5 re f") as pdf:
            assert text.check_min_stroke(pdf, 0.25) == []


class TestSafeArea:
    def test_text_on_the_trim_edge_is_outside_a_safe_margin(self, tmp_path):
        stream = b"BT /F1 12 Tf %f %f Td (A) Tj ET" % (3 * MM + 1, 100)
        with _page(tmp_path, stream, fonts={"F1": _wide()}) as pdf:
            [finding] = text.check_safe_area(pdf, 5.0)
        assert finding.severity is Severity.ERROR and "outside the 5mm safe area" in finding.message

    def test_text_well_inside_passes(self, tmp_path):
        stream = b"BT /F1 12 Tf 100 200 Td (A) Tj ET"
        with _page(tmp_path, stream, fonts={"F1": _wide()}) as pdf:
            assert text.check_safe_area(pdf, 5.0) == []

    def test_the_margin_is_measured_from_the_trim_not_the_media_box(self, tmp_path):
        """Text in the bleed is outside the trim whatever the margin."""
        with _page(tmp_path, b"BT /F1 12 Tf 2 200 Td (A) Tj ET", fonts={"F1": _wide()}) as pdf:
            assert [f.check for f in text.check_safe_area(pdf, 0.1)] == ["safe-area"]

    def test_the_far_edge_counts_too(self, tmp_path):
        # A is 12pt wide; start so that it ends just past the right safe line.
        right = 3 * MM + 105 * MM - 5 * MM
        with _page(tmp_path, b"BT /F1 12 Tf %f 200 Td (A) Tj ET" % (right - 6), fonts={"F1": _wide()}) as pdf:
            assert [f.check for f in text.check_safe_area(pdf, 5.0)] == ["safe-area"]

    def test_a_font_without_widths_gives_an_estimated_warning(self, tmp_path):
        stream = b"BT /F1 12 Tf %f 100 Td (A) Tj ET" % (3 * MM + 1)
        with _page(tmp_path, stream, fonts={"F1": _simple()}) as pdf:
            [finding] = text.check_safe_area(pdf, 5.0)
        assert finding.severity is Severity.WARN and "estimated" in finding.message

    def test_a_page_without_any_box_is_skipped_not_crashed(self, tmp_path):
        pdf = pikepdf.new()
        pdf.add_blank_page()
        assert text.check_safe_area(pdf, 5.0) == []


class TestFonts:
    def _embedded(self, pdf: pikepdf.Pdf | None = None) -> pikepdf.Dictionary:
        descriptor = pikepdf.Dictionary(FontFile2=(pdf or pikepdf.new()).make_stream(b"x"))
        return _simple(FontDescriptor=descriptor, FirstChar=65, Widths=[600])

    def test_an_unembedded_font_is_an_error_naming_it(self, tmp_path):
        with _page(tmp_path, b"BT /F1 12 Tf (A) Tj ET", fonts={"F1": _simple(FirstChar=65, Widths=[600])}) as pdf:
            [finding] = text.check_fonts(pdf)
        assert finding.check == "font-embedded" and finding.severity is Severity.ERROR
        assert "Face" in finding.message and "not embedded" in finding.message

    def test_an_embedded_font_is_clean(self, tmp_path):
        pdf = pikepdf.new()
        font = pdf.make_indirect(self._embedded(pdf))
        page = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            Contents=pdf.make_stream(b"BT /F1 12 Tf (A) Tj ET"),
            Resources=pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font)),
            **BOXES,
        )
        pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        assert text.check_fonts(pdf) == []

    def test_a_type3_font_is_a_warning(self, tmp_path):
        type3 = pikepdf.Dictionary(
            Type=pikepdf.Name.Font,
            Subtype=pikepdf.Name.Type3,
            Name=pikepdf.Name.F1,
            FontMatrix=[0.001, 0, 0, 0.001, 0, 0],
            FirstChar=65,
            Widths=[600],
        )
        with _page(tmp_path, b"BT /F1 12 Tf (A) Tj ET", fonts={"F1": type3}) as pdf:
            [finding] = text.check_fonts(pdf)
        assert finding.check == "font-type3" and finding.severity is Severity.WARN

    def test_a_font_no_text_is_shown_in_is_not_reported(self, tmp_path):
        with _page(tmp_path, b"0 g 0 0 5 5 re f", fonts={"F1": _simple()}) as pdf:
            assert text.check_fonts(pdf) == []

    def test_each_font_is_reported_once_per_page_with_its_run_count(self, tmp_path):
        with _page(
            tmp_path, b"BT /F1 12 Tf (A) Tj (A) Tj ET", fonts={"F1": _simple(FirstChar=65, Widths=[600])}
        ) as pdf:
            [finding] = text.check_fonts(pdf)
        assert "2 run(s)" in finding.message
