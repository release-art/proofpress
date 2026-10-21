"""The shared walk: text boxes, advances, fonts, strokes, path and image extents, spot inks."""

import pathlib
import typing

import pikepdf
import pytest

from proof_pdf._internal import fonts, paint

Pt = pytest.approx


def _font(pdf: pikepdf.Pdf, **entries) -> pikepdf.Object:
    return pdf.make_indirect(
        pikepdf.Dictionary(
            Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name("/Test"), **entries
        )
    )


def _ab_font(pdf: pikepdf.Pdf) -> pikepdf.Object:
    """A=600, B=700 (per 1000 em)."""
    return _font(pdf, FirstChar=65, Widths=[600, 700])


def _events(tmp_path: pathlib.Path, stream: bytes, *, font=None, **resources: typing.Any) -> list[paint.Paint]:
    pdf = pikepdf.new()
    res = pikepdf.Dictionary({f"/{k}": (v(pdf) if callable(v) else v) for k, v in resources.items()})
    if font is not None:
        res["/Font"] = pikepdf.Dictionary(F1=font(pdf) if callable(font) else font)
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page, MediaBox=[0, 0, 500, 500], Contents=pdf.make_stream(stream), Resources=res
    )
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
    path = tmp_path / "t.pdf"
    pdf.save(path)
    with pikepdf.open(path) as opened:
        return list(paint.paint_events(opened.pages[0]))


def _text(tmp_path, stream: bytes, **kw) -> list[paint.Paint]:
    return [p for p in _events(tmp_path, stream, font=kw.pop("font", _ab_font), **kw) if p.kind == "text"]


class TestTextBoxes:
    def test_box_follows_the_declared_widths(self, tmp_path):
        [run] = _text(tmp_path, b"BT /F1 10 Tf 100 200 Td (AB) Tj ET")
        # (600 + 700) / 1000 * 10 = 13 wide; descent -200 and ascent 800 default to -2 and +8.
        assert run.bbox == (Pt(100), Pt(198), Pt(113), Pt(208))
        assert run.glyphs == 2 and run.size == Pt(10)

    def test_a_second_show_continues_where_the_first_stopped(self, tmp_path):
        first, second = _text(tmp_path, b"BT /F1 10 Tf 100 200 Td (AB) Tj (A) Tj ET")
        assert second.bbox is not None and second.bbox[0] == Pt(113)

    def test_tj_adjustments_move_the_pen(self, tmp_path):
        [run] = _text(tmp_path, b"BT /F1 10 Tf 0 0 Td [(A) -500 (B)] TJ ET")
        assert run.bbox is not None and run.bbox[2] == Pt(6 + 5 + 7)  # A, a +5pt gap, B

    def test_char_and_word_spacing_and_horizontal_scale(self, tmp_path):
        [spaced] = _text(tmp_path, b"BT /F1 10 Tf 2 Tc 0 0 Td (AB) Tj ET")
        [scaled] = _text(tmp_path, b"BT /F1 10 Tf 50 Tz 0 0 Td (AB) Tj ET")
        assert spaced.bbox is not None and spaced.bbox[2] == Pt(13 + 4)
        assert scaled.bbox is not None and scaled.bbox[2] == Pt(13 / 2)

    def test_td_is_relative_to_the_start_of_the_line_not_the_pen(self, tmp_path):
        _, second = _text(tmp_path, b"BT /F1 10 Tf 100 100 Td (AB) Tj 0 -12 Td (A) Tj ET")
        assert second.bbox is not None and second.bbox[0] == Pt(100) and second.bbox[1] == Pt(100 - 12 - 2)

    def test_leading_and_t_star(self, tmp_path):
        _, second = _text(tmp_path, b"BT /F1 10 Tf 12 TL 100 100 Td (A) Tj T* (B) Tj ET")
        assert second.bbox is not None and second.bbox[0] == Pt(100) and second.bbox[1] == Pt(86)

    def test_text_matrix_and_ctm_scale_the_box(self, tmp_path):
        [run] = _text(tmp_path, b"q 1 0 0 1 10 20 cm BT /F1 10 Tf 2 0 0 2 50 60 Tm (AB) Tj ET Q")
        assert run.bbox is not None
        assert run.bbox[0] == Pt(60) and run.bbox[2] == Pt(60 + 26)  # 13 wide, doubled
        assert run.size == Pt(20)

    def test_text_state_is_restored_by_q(self, tmp_path):
        runs = _text(tmp_path, b"BT /F1 10 Tf q 5 Tc /F1 20 Tf Q 0 0 Td (AB) Tj ET")
        assert runs[0].size == Pt(10) and runs[0].bbox is not None and runs[0].bbox[2] == Pt(13)


class TestFonts:
    def test_a_composite_font_reads_cid_widths(self, tmp_path):
        def type0(pdf):
            cid = pikepdf.Dictionary(
                Subtype=pikepdf.Name.CIDFontType2, DW=1000, W=[1, [500], 5, 6, 250], FontDescriptor=pikepdf.Dictionary()
            )
            return pdf.make_indirect(
                pikepdf.Dictionary(
                    Type=pikepdf.Name.Font,
                    Subtype=pikepdf.Name.Type0,
                    BaseFont=pikepdf.Name("/ABCDEF+Face"),
                    Encoding=pikepdf.Name("/Identity-H"),
                    DescendantFonts=[pdf.make_indirect(cid)],
                )
            )

        [run] = _text(tmp_path, b"BT /F1 10 Tf <0001000200050006> Tj ET", font=type0)
        # cid 1 = 500, cid 2 = default 1000, cid 5 and 6 = 250 each.
        assert run.bbox is not None and run.bbox[2] == Pt((500 + 1000 + 250 + 250) / 100)
        assert run.font is not None and run.font.name == "Face" and run.font.code_bytes == 2

    def test_cid_zero_is_notdef_in_an_identity_font(self, tmp_path):
        def type0(pdf):
            cid = pikepdf.Dictionary(Subtype=pikepdf.Name.CIDFontType2, FontDescriptor=pikepdf.Dictionary())
            return pdf.make_indirect(
                pikepdf.Dictionary(
                    Type=pikepdf.Name.Font,
                    Subtype=pikepdf.Name.Type0,
                    BaseFont=pikepdf.Name("/F"),
                    Encoding=pikepdf.Name("/Identity-H"),
                    DescendantFonts=[pdf.make_indirect(cid)],
                )
            )

        [run] = _text(tmp_path, b"BT /F1 10 Tf <00000001> Tj ET", font=type0)
        assert (run.glyphs, run.notdef) == (2, 1)

    def test_a_non_identity_composite_encoding_is_not_guessed_at(self, tmp_path):
        def type0(pdf):
            cid = pikepdf.Dictionary(Subtype=pikepdf.Name.CIDFontType2, FontDescriptor=pikepdf.Dictionary())
            return pdf.make_indirect(
                pikepdf.Dictionary(
                    Type=pikepdf.Name.Font,
                    Subtype=pikepdf.Name.Type0,
                    BaseFont=pikepdf.Name("/F"),
                    Encoding=pikepdf.Name("/UniJIS-UCS2-H"),
                    DescendantFonts=[pdf.make_indirect(cid)],
                )
            )

        [run] = _text(tmp_path, b"BT /F1 10 Tf <0000> Tj ET", font=type0)
        assert run.notdef == 0

    def test_differences_mapping_to_notdef_is_found(self, tmp_path):
        def simple(pdf):
            encoding = pikepdf.Dictionary(
                Type=pikepdf.Name.Encoding, Differences=[65, pikepdf.Name("/.notdef"), pikepdf.Name("/B")]
            )
            return _font(pdf, FirstChar=65, Widths=[600, 700], Encoding=encoding)

        [run] = _text(tmp_path, b"BT /F1 10 Tf (AB) Tj ET", font=simple)
        assert run.notdef == 1

    def test_embedding_and_tounicode_and_subset_prefix(self):
        pdf = pikepdf.new()
        descriptor = pikepdf.Dictionary(FontFile2=pdf.make_stream(b"x"))
        font = pikepdf.Dictionary(
            Subtype=pikepdf.Name.TrueType,
            BaseFont=pikepdf.Name("/ABCDEF+Nunito"),
            FontDescriptor=descriptor,
            ToUnicode=pdf.make_stream(b"x"),
        )
        loaded = fonts.load(font)
        assert (loaded.name, loaded.embedded, loaded.has_to_unicode) == ("Nunito", True, True)

    def test_an_unembedded_standard_font_is_reported_as_such(self):
        font = pikepdf.Dictionary(Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name("/Helvetica"))
        loaded = fonts.load(font)
        assert (loaded.embedded, loaded.has_to_unicode) == (False, False)
        assert loaded.width(65) == fonts._FALLBACK_WIDTH

    def test_a_malformed_font_degrades_instead_of_raising(self):
        assert fonts.load(pikepdf.Dictionary(Subtype=pikepdf.Name.Type0)) is fonts.UNKNOWN
        assert fonts.load(None) is fonts.UNKNOWN


class TestPaths:
    def test_a_filled_rectangle_has_its_page_space_extent_and_colour(self, tmp_path):
        [rect] = _events(tmp_path, b"0.5 g 10 20 30 40 re f")
        assert rect.kind == "path" and rect.filled and rect.stroke_width is None
        assert rect.bbox == (Pt(10), Pt(20), Pt(40), Pt(60))
        assert rect.fill == paint.Fill("gray", (0.5,))

    def test_line_width_is_reported_in_page_points(self, tmp_path):
        plain, scaled, hairline = (
            _events(tmp_path, s)[0].stroke_width
            for s in (b"2 w 0 0 m 10 0 l S", b"0.5 0 0 0.5 0 0 cm 2 w 0 0 m 10 0 l S", b"0 w 0 0 m 10 0 l S")
        )
        assert (plain, scaled, hairline) == (Pt(2.0), Pt(1.0), Pt(0.0))

    def test_line_width_is_restored_by_q_and_set_by_an_extgstate(self, tmp_path):
        gs = pikepdf.Dictionary(GS1=pikepdf.Dictionary(LW=0.25))
        a, b = _events(tmp_path, b"q 5 w Q 0 0 m 1 1 l S /GS1 gs 0 0 m 1 1 l S", ExtGState=gs)
        assert (a.stroke_width, b.stroke_width) == (Pt(1.0), Pt(0.25))

    def test_a_curve_extends_the_box(self, tmp_path):
        [path] = _events(tmp_path, b"0 0 m 10 0 20 30 30 30 c f")
        assert path.bbox == (Pt(0), Pt(0), Pt(30), Pt(30))

    def test_n_discards_the_path(self, tmp_path):
        [path] = _events(tmp_path, b"0 0 100 100 re n 5 5 10 10 re f")
        assert path.bbox == (Pt(5), Pt(5), Pt(15), Pt(15))


class TestImagesAndInks:
    def test_an_image_has_the_footprint_the_ctm_gives_it(self, tmp_path):
        def xobjects(pdf):
            image = pdf.make_stream(b"\x00")
            image["/Type"], image["/Subtype"] = pikepdf.Name.XObject, pikepdf.Name.Image
            image["/Width"], image["/Height"], image["/BitsPerComponent"] = 1, 1, 8
            image["/ColorSpace"] = pikepdf.Name.DeviceGray
            return pikepdf.Dictionary(Im1=pdf.make_indirect(image))

        [event] = _events(tmp_path, b"q 50 0 0 40 10 20 cm /Im1 Do Q", XObject=xobjects)
        assert event.kind == "image" and event.bbox == (Pt(10), Pt(20), Pt(60), Pt(60)) and event.name == "/Im1"

    def test_spot_and_devicen_inks_are_collected(self, tmp_path):
        spaces = pikepdf.Dictionary(
            CS1=pikepdf.Array([pikepdf.Name.Separation, pikepdf.Name("/PANTONE 185 C"), pikepdf.Name.DeviceCMYK]),
            CS2=pikepdf.Array(
                [
                    pikepdf.Name.DeviceN,
                    pikepdf.Array([pikepdf.Name("/Gold"), pikepdf.Name("/Varnish")]),
                    pikepdf.Name.DeviceCMYK,
                ]
            ),
        )
        pdf = pikepdf.new()
        res = pikepdf.Dictionary(ColorSpace=spaces)
        page = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            MediaBox=[0, 0, 100, 100],
            Contents=pdf.make_stream(b"/CS1 cs 1 scn 0 0 5 5 re f /CS2 cs 1 1 scn 0 0 5 5 re f"),
            Resources=res,
        )
        pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        walker = paint.Walker()
        list(walker.walk(pdf.pages[0]))
        assert walker.spots == {"PANTONE 185 C", "Gold", "Varnish"}
