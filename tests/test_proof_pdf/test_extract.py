"""What ``extract`` recovers, from the committed fixture PDFs and small ones built here (no external tools needed)."""

import json
import pathlib
import shutil

import pikepdf
import pytest
import typer.testing

from proof_pdf import cli, extract
from proof_pdf._internal import fonts, xmp

PDFS = pathlib.Path(__file__).parents[1] / "fixtures" / "pdfs"

# Two identical triangles, far apart on the page (one at an offset), and a plain rectangle that touches both.
ARTWORK = b"""
0 0 0 rg 10 10 280 20 re f
q 1 0 0 rg 20 100 m 40 100 l 30 120 l h f Q
q 1 0 0 1 100 0 cm 1 0 0 rg 20 100 m 40 100 l 30 120 l h f Q
"""


@pytest.fixture
def runner():
    return typer.testing.CliRunner()


@pytest.fixture
def artwork_pdf(tmp_path):
    pdf = pikepdf.new()
    page = pdf.add_blank_page(page_size=(300, 200))
    page.Contents = pdf.make_stream(ARTWORK)
    path = tmp_path / "art.pdf"
    pdf.save(path)
    return path


class TestImages:
    def test_an_image_is_written_with_its_placement(self, tmp_path):
        result = extract.extract_pdf(PDFS / "rgb-image.pdf", tmp_path)
        [image] = [a for a in result.assets if a.kind == "image"]
        assert (tmp_path / image.file).is_file() and image.details["pixels"] == [500, 500]
        [placement] = image.placements
        assert (placement.page, placement.width, placement.height) == (1, pytest.approx(100), pytest.approx(100))

    def test_kinds_limits_what_is_recovered_but_the_document_is_always_written(self, tmp_path):
        result = extract.extract_pdf(PDFS / "rgb-image.pdf", tmp_path, kinds=("fonts",))
        assert [a.kind for a in result.assets] == [] and (tmp_path / "document.json").is_file()


class TestVector:
    def test_artwork_is_one_file_per_piece_and_the_same_piece_is_placed_twice(self, tmp_path, artwork_pdf):
        result = extract.extract_pdf(artwork_pdf, tmp_path / "out", kinds=("vector",))
        [triangle] = [a for a in result.assets if a.kind == "vector"]
        assert sorted(round(p.x) for p in triangle.placements) == [20, 120]
        with pikepdf.open(tmp_path / "out" / triangle.file) as pdf:
            assert [round(float(v)) for v in pdf.pages[0].mediabox] == [0, 0, 20, 20]

    def test_a_plain_rectangle_is_a_box_and_does_not_glue_the_artwork(self, tmp_path, artwork_pdf):
        result = extract.extract_pdf(artwork_pdf, tmp_path / "out", kinds=("vector",))
        [box] = result.boxes
        assert (box.kind, box.fill, box.width, box.height) == ("rectangle", "rg 0 0 0", 280, 20)

    def test_a_shape_bigger_than_the_page_fraction_is_left_out(self, tmp_path, artwork_pdf):
        result = extract.extract_pdf(artwork_pdf, tmp_path / "out", kinds=("vector",), max_fraction=0.0001)
        assert [a for a in result.assets if a.kind == "vector"] == []


class TestText:
    def test_a_run_comes_back_with_its_font_size_colour_and_position(self, tmp_path):
        [run] = extract.extract_pdf(PDFS / "cyan-text.pdf", tmp_path).text
        assert (run.text, run.font, run.page) == ("cyan", "Helvetica", 1)
        assert run.size_pt > 0 and run.colour and run.width > 0

    def test_to_unicode_maps_chars_and_ranges(self):
        pdf = pikepdf.new()
        cmap = pdf.make_stream(
            b"1 beginbfchar <0003> <0020> endbfchar "
            b"2 beginbfrange <0010> <0012> <0041> <0020> <0021> [<0061> <0062>] endbfrange",
        )
        assert fonts._to_unicode(cmap) == {3: " ", 0x10: "A", 0x11: "B", 0x12: "C", 0x20: "a", 0x21: "b"}


class TestDocument:
    def test_document_json_holds_pages_assets_text_and_inventory(self, tmp_path):
        extract.extract_pdf(PDFS / "cyan-text.pdf", tmp_path)
        document = json.loads((tmp_path / "document.json").read_text(encoding="utf-8"))
        assert {"pages", "metadata", "output_intents", "assets", "text", "boxes", "inventory", "failures"} <= set(
            document
        )
        assert document["pages"][0]["height_pt"] > 0 and document["inventory"]["fonts"][0]["name"] == "Helvetica"

    def test_an_embedded_icc_profile_is_written_with_its_description(self, tmp_path):
        result = extract.extract_pdf(PDFS / "rgb-image.pdf", tmp_path)
        [profile] = [a for a in result.assets if a.kind == "icc"]
        assert (tmp_path / profile.file).is_file() and profile.details["description"]


class TestCli:
    def test_extract_writes_a_folder_named_for_the_pdf(self, runner, tmp_path):
        result = runner.invoke(cli.pdf_app, ["extract", str(PDFS / "rgb-image.pdf"), "--out", str(tmp_path)])
        assert result.exit_code == 0, result.output
        assert (tmp_path / "rgb-image" / "document.json").is_file() and "1 image" in result.output

    def test_an_unknown_kind_is_refused(self, runner, tmp_path):
        result = runner.invoke(
            cli.pdf_app, ["extract", str(PDFS / "good.pdf"), "--kind", "nope", "--out", str(tmp_path)]
        )
        assert result.exit_code != 0

    def test_a_file_that_will_not_open_fails_the_run(self, runner, tmp_path):
        result = runner.invoke(cli.pdf_app, ["extract", str(PDFS / "corrupt.pdf"), "--out", str(tmp_path)])
        assert result.exit_code == 1


def test_artwork_smaller_than_the_minimum_page_size_is_still_written(tmp_path):
    pdf = pikepdf.new()
    page = pdf.add_blank_page(page_size=(300, 200))
    page.Contents = pdf.make_stream(b"0 g 50 50 m 51 50 l 50 51 l h f")
    path = tmp_path / "dot.pdf"
    pdf.save(path)
    result = extract.extract_pdf(path, tmp_path / "out", kinds=("vector",))
    [dot] = [a for a in result.assets if a.kind == "vector"]
    assert result.failures == [] and (tmp_path / "out" / dot.file).is_file()


def _page_pdf(tmp_path, content: bytes, *, font: bool = False) -> pathlib.Path:
    pdf = pikepdf.new()
    page = pdf.add_blank_page(page_size=(300, 200))
    page.Contents = pdf.make_stream(content)
    if font:
        helvetica = pdf.make_indirect(
            pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name.Helvetica)
        )
        page.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=helvetica))
    path = tmp_path / "page.pdf"
    pdf.save(path)
    return path


class TestTidying:
    def test_runs_on_one_baseline_become_a_line_with_a_space_at_a_word_gap(self, tmp_path):
        # Unembedded Helvetica is measured at 6 pt a character: "Hello" ends at 40, so 45 is a 5 pt gap, 50 is 10 pt.
        path = _page_pdf(
            tmp_path, b"BT /F1 12 Tf 10 100 Td (Hello) Tj 35 0 Td (World) Tj 40 0 Td (Far) Tj ET", font=True
        )
        result = extract.extract_pdf(path, tmp_path / "out", kinds=())
        assert [line.text for line in result.lines] == ["Hello World", "Far"] and len(result.text) == 3

    def test_touching_rules_of_one_style_are_one_rule_and_a_line_keeps_its_ends(self, tmp_path):
        path = _page_pdf(tmp_path, b"0 g 10 10 50 0.5 re f 60 10 50 0.5 re f 10 50 m 100 90 l S")
        result = extract.extract_pdf(path, tmp_path / "out", kinds=("vector",))
        line, rule = sorted(result.boxes, key=lambda b: b.kind)
        assert (line.kind, line.points) == ("line", [[10.0, 50.0], [100.0, 90.0]])
        assert (rule.kind, rule.width) == ("rectangle", pytest.approx(100.0))

    def test_vector_files_are_named_by_page_and_order(self, tmp_path, artwork_pdf):
        result = extract.extract_pdf(artwork_pdf, tmp_path / "out", kinds=("vector",))
        assert [a.file for a in result.assets] == ["vector/p001-v01.pdf"]


class TestUnreadableText:
    def test_a_font_without_to_unicode_gives_codes_and_a_note_instead_of_guesses(self, tmp_path):
        pdf = pikepdf.new()
        page = pdf.add_blank_page(page_size=(300, 200))
        cid = pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.CIDFontType2, BaseFont=pikepdf.Name.Foo)
        font = pikepdf.Dictionary(
            Type=pikepdf.Name.Font,
            Subtype=pikepdf.Name.Type0,
            BaseFont=pikepdf.Name("/ABCDEF+Foo"),
            Encoding=pikepdf.Name("/Identity-H"),
            DescendantFonts=[pdf.make_indirect(cid)],
        )
        page.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=pdf.make_indirect(font)))
        page.Contents = pdf.make_stream(b"BT /F1 10 Tf 10 100 Td <00410042> Tj ET")
        path = tmp_path / "cid.pdf"
        pdf.save(path)
        result = extract.extract_pdf(path, tmp_path / "out", kinds=())
        [run] = result.text
        assert (run.text, run.codes) == (None, [0x41, 0x42])
        assert [(line.text, line.font) for line in result.lines] == [(None, "Foo")]  # geometry still counts
        assert len(result.notes) == 1 and "Foo (1)" in result.notes[0]


@pytest.mark.container
@pytest.mark.skipif(shutil.which("gs") is None, reason="needs Ghostscript on PATH")
class TestPreviews:
    def test_the_review_page_boxes_each_asset_on_its_page_and_lists_it_below(self, tmp_path):
        result = extract.extract_pdf(PDFS / "rgb-image.pdf", tmp_path, previews=True)
        assert result.previews == "previews/index.html" and result.failures == []
        page = (tmp_path / result.previews).read_text(encoding="utf-8")
        assert "class='ov img'" in page and "href='../images/p001-im10.png'" in page
        assert (tmp_path / "previews" / "image" / "p001-im10.png").is_file()
        assert (tmp_path / "previews" / "rgb-image-p1.png").is_file()

    def test_vector_pieces_get_a_png_thumbnail(self, tmp_path, artwork_pdf):
        result = extract.extract_pdf(artwork_pdf, tmp_path / "out", previews=True)
        assert result.previews and (tmp_path / "out" / "previews" / "vector" / "p001-v01.png").is_file()


class TestPreviewsCli:
    def test_previews_without_ghostscript_fails_before_extracting(self, runner, tmp_path):
        args = ["extract", str(PDFS / "good.pdf"), "--out", str(tmp_path), "--previews", "--gs", "no-such-gs"]
        result = runner.invoke(cli.pdf_app, args)
        assert result.exit_code == 1 and not (tmp_path / "good").exists()


PACKET = b"""<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:xmpTPg="http://ns.adobe.com/xap/1.0/t/pg/" xmlns:stFnt="http://ns.adobe.com/xap/1.0/sType/Font#"
  xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreatorTool="InDesign">
 <dc:title><rdf:Alt><rdf:li xml:lang="x-default">Brochure</rdf:li></rdf:Alt></dc:title>
 <dc:subject><rdf:Bag><rdf:li>box</rdf:li><rdf:li>carton</rdf:li></rdf:Bag></dc:subject>
 <xmpTPg:Fonts><rdf:Bag><rdf:li rdf:parseType="Resource">
  <stFnt:fontName>Calibri-Bold</stFnt:fontName></rdf:li></rdf:Bag></xmpTPg:Fonts>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>"""


class TestXmp:
    def test_properties_lists_alternatives_and_structures_are_decoded_with_their_prefixes(self):
        assert xmp.parse(PACKET) == {
            "xmp:CreatorTool": "InDesign",
            "dc:title": "Brochure",
            "dc:subject": ["box", "carton"],
            "xmpTPg:Fonts": [{"stFnt:fontName": "Calibri-Bold"}],
        }

    def test_text_that_is_not_xml_is_a_value_error(self):
        with pytest.raises(ValueError, match="not XML"):
            xmp.parse(b"<<nope")

    def test_the_packet_is_saved_and_decoded_in_the_document(self, tmp_path):
        pdf = pikepdf.new()
        pdf.add_blank_page()
        with pdf.open_metadata() as meta:
            meta["dc:title"] = "Hello"
        path = tmp_path / "meta.pdf"
        pdf.save(path)
        result = extract.extract_pdf(path, tmp_path / "out", kinds=())
        assert result.metadata["xmp"]["dc:title"] == "Hello" and (tmp_path / "out" / "xmp.xml").is_file()


def _text_pdf(tmp_path, pages: list[list[tuple[int, int, float, str]]]) -> pathlib.Path:
    """Pages of (x, y, size, text) lines in Helvetica, 400 x 300 pt."""
    pdf = pikepdf.new()
    helvetica = pdf.make_indirect(
        pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name.Helvetica)
    )
    for lines in pages:
        page = pdf.add_blank_page(page_size=(400, 300))
        page.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=helvetica))
        page.Contents = pdf.make_stream(
            b" ".join(f"BT /F1 {size} Tf 1 0 0 1 {x} {y} Tm ({text}) Tj ET".encode() for x, y, size, text in lines)
        )
    path = tmp_path / "text.pdf"
    pdf.save(path)
    return path


class TestLayout:
    def body(self, x: int, top: int, word: str) -> list[tuple[int, int, float, str]]:
        return [(x, top - 12 * i, 10.0, f"{word} {word} {word} {word}") for i in range(5)]

    def test_columns_styles_and_leading_are_read_from_where_the_text_sits(self, tmp_path):
        lines = [(50, 250, 24.0, "Title"), *self.body(50, 200, "left"), *self.body(250, 200, "right")]
        result = extract.extract_pdf(_text_pdf(tmp_path, [lines]), tmp_path / "out", kinds=())
        layout = result.layout
        assert [(s.role, s.size_pt) for s in layout.styles] == [("heading-1", 24.0), ("body", 10.0)]
        [page] = layout.pages
        assert [[round(lo), round(hi)] for lo, hi in page.columns] == [[50, 145], [250, 365]]
        assert sorted((b.role, b.lines, b.column) for b in page.blocks) == [
            ("body", 5, 0),
            ("body", 5, 1),
            ("heading-1", 1, 0),
        ]
        assert (layout.summary.columns, layout.summary.leading_pt, layout.summary.body_size_pt) == (2, 12.0, 10.0)
        assert layout.summary.margins_mm is not None and layout.summary.measure_pt == pytest.approx(
            105, abs=1
        )  # the median of 95 and 115

    def test_text_repeated_at_one_place_on_every_page_is_running_and_a_folio_when_digits_differ(self, tmp_path):
        pages = [[*self.body(50, 200, word), (50, 20, 8.0, f"Report {n}")] for n, word in ((1, "one"), (2, "two"))]
        result = extract.extract_pdf(_text_pdf(tmp_path, pages), tmp_path / "out", kinds=())
        [running] = result.layout.running
        assert (running.text, running.position, running.folio, running.pages) == ("Report #", "footer", True, [1, 2])
        assert all(b.running for p in result.layout.pages for b in p.blocks if b.lines == 1)
        margins = result.layout.pages[0].margins_pt
        assert margins is not None and margins["bottom"] > 100  # the folio is not content


class TestArtworkGrouping:
    DIAGRAM = (
        b"0 G 10 100 40 30 re S 100 100 40 30 re S 50 115 m 100 115 l S "  # two boxes joined by a wire
        b"0 g 94 111 m 100 115 l 94 119 l f "  # its arrowhead
        b"BT /F1 10 Tf 1 0 0 1 14 112 Tm (A) Tj ET"  # a label inside the first box
    )

    def pieces(self, result):
        return [a for a in result.assets if a.kind == "vector"]

    def test_wires_and_boxes_joined_by_lines_are_one_piece_with_its_label(self, tmp_path):
        result = extract.extract_pdf(_page_pdf(tmp_path, self.DIAGRAM, font=True), tmp_path / "out", kinds=("vector",))
        [piece] = self.pieces(result)
        assert (piece.details["shapes"], piece.details["text"]) == (4, ["A"])
        with pikepdf.open(tmp_path / "out" / piece.file) as pdf:
            assert "/Font" in pdf.pages[0].Resources  # the label is drawn into the piece, with its font
        assert [line.text for line in result.lines] == ["A"]  # and is still recovered as text

    def test_a_label_touching_two_pieces_joins_them(self, tmp_path):
        content = (
            b"0 g 10 100 m 20 100 l 15 110 l f 60 100 m 70 100 l 65 110 l f "
            b"BT /F1 10 Tf 1 0 0 1 15 98 Tm (label label) Tj ET"
        )
        path = _page_pdf(tmp_path, content, font=True)
        joined = extract.extract_pdf(path, tmp_path / "a", kinds=("vector",))
        assert [p.details["shapes"] for p in self.pieces(joined)] == [2]
        apart = extract.extract_pdf(path, tmp_path / "b", kinds=("vector",), vector_text=False)
        assert [p.details["shapes"] for p in self.pieces(apart)] == [1, 1]

    def test_an_icon_touching_a_grid_of_rules_stays_apart_from_the_grid(self, tmp_path):
        grid = b"".join(f"10 {y} m 70 {y} l S ".encode() for y in (10, 22, 34, 46))
        grid += b"".join(f"{x} 10 m {x} 46 l S ".encode() for x in (10, 40, 70))
        path = _page_pdf(tmp_path, b"0 G " + grid + b"0 g 40 22 m 48 22 l 44 30 l f")
        result = extract.extract_pdf(path, tmp_path / "out", kinds=("vector",))
        assert [p.details["shapes"] for p in self.pieces(result)] == [1] and len(result.boxes) == 7


class TestMoreFacts:
    def test_an_adobe_cjk_font_without_to_unicode_reads_its_latin_cids(self, tmp_path):
        pdf = pikepdf.new()
        page = pdf.add_blank_page(page_size=(300, 200))
        info = pikepdf.Dictionary(Registry=pikepdf.String("Adobe"), Ordering=pikepdf.String("GB1"), Supplement=5)
        cid = pikepdf.Dictionary(
            Type=pikepdf.Name.Font, Subtype=pikepdf.Name.CIDFontType0, BaseFont=pikepdf.Name.Foo, CIDSystemInfo=info
        )
        font = pikepdf.Dictionary(
            Type=pikepdf.Name.Font,
            Subtype=pikepdf.Name.Type0,
            BaseFont=pikepdf.Name.Foo,
            Encoding=pikepdf.Name("/Identity-H"),
            DescendantFonts=[pdf.make_indirect(cid)],
        )
        page.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=pdf.make_indirect(font)))
        page.Contents = pdf.make_stream(b"BT /F1 10 Tf 10 100 Td <00220023> Tj ET")  # CID 34, 35 are 'A', 'B'
        path = tmp_path / "gb1.pdf"
        pdf.save(path)
        result = extract.extract_pdf(path, tmp_path / "out", kinds=())
        assert [(run.text, run.codes) for run in result.text] == [("AB", None)] and result.notes == []

    def test_rotated_scaled_and_invisible_text_say_so(self, tmp_path):
        content = (
            b"BT /F1 10 Tf 0 1 -1 0 50 50 Tm 50 Tz 2 Tc (up) Tj ET BT /F1 10 Tf 3 Tr 1 0 0 1 100 100 Tm (ocr) Tj ET"
        )
        result = extract.extract_pdf(_page_pdf(tmp_path, content, font=True), tmp_path / "out", kinds=())
        up, ocr = sorted(result.text, key=lambda run: run.text or "", reverse=True)
        assert (up.rotation_deg, up.h_scale, up.letter_spacing_em, up.invisible) == (90.0, 0.5, 0.2, False)
        assert (ocr.rotation_deg, ocr.invisible) == (0.0, True)

    def test_an_image_placement_keeps_its_matrix(self, tmp_path):
        result = extract.extract_pdf(PDFS / "rgb-image.pdf", tmp_path / "out", kinds=("images",))
        [image] = [a for a in result.assets if a.kind == "image"]
        assert image.placements[0].matrix is not None and image.placements[0].matrix[0] == pytest.approx(100.0)

    def test_links_form_fields_outline_and_page_labels_are_collected(self, tmp_path):
        pdf = pikepdf.new()
        pdf.add_blank_page(page_size=(300, 200))
        pdf.add_blank_page(page_size=(300, 200))
        link = pdf.make_indirect(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Link,
                Rect=[10, 10, 60, 30],
                A=pikepdf.Dictionary(S=pikepdf.Name.URI, URI=pikepdf.String("https://example.com")),
            )
        )
        field = pdf.make_indirect(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                FT=pikepdf.Name.Tx,
                T=pikepdf.String("name"),
                V=pikepdf.String("Ann"),
                Rect=[70, 10, 160, 30],
                DA=pikepdf.String("/Helv 9 Tf 0 g"),
            )
        )
        pdf.pages[0].Annots = pikepdf.Array([link, field])
        pdf.Root.AcroForm = pikepdf.Dictionary(Fields=pikepdf.Array([field]))
        pdf.Root.PageLabels = pikepdf.Dictionary(Nums=[0, pikepdf.Dictionary(S=pikepdf.Name.r)])
        with pdf.open_outline() as outline:
            outline.root.append(pikepdf.OutlineItem("Second page", 1))
        path = tmp_path / "forms.pdf"
        pdf.save(path)
        found = extract.extract_pdf(path, tmp_path / "out", kinds=()).interactive
        assert found["links"][0]["uri"] == "https://example.com" and found["links"][0]["page"] == 1
        [name] = found["fields"]
        assert (name["name"], name["type"], name["value"], name["font"], name["font_size_pt"]) == (
            "name",
            "text",
            "Ann",
            "Helv",
            9.0,
        )
        assert [(o["title"], o["to"]["page"]) for o in found["outline"]] == [("Second page", 2)]
        assert found["page_labels"] == [{"from_page": 1, "style": "r", "prefix": "", "start": 1}]


def _fn(**entries) -> pikepdf.Dictionary:
    return pikepdf.Dictionary(Domain=[0, 1], **entries)


class TestSpotColours:
    def test_functions_give_the_alternate_of_a_tint(self):
        from proof_pdf._internal import functions

        pdf = pikepdf.new()
        exponential = _fn(FunctionType=2, C0=[0, 0, 0, 0], C1=[0, 0.2, 0.8, 0.1], N=1)
        calculator = pdf.make_stream(b"{ dup 0.4 mul exch 0.2 mul 0 0 }")
        calculator.update(_fn(FunctionType=4, Range=[0, 1] * 4))
        sampled = pdf.make_stream(bytes([0, 0, 255, 128]))
        sampled.update(_fn(FunctionType=0, Size=[2], BitsPerSample=8, Range=[0, 1, 0, 1]))
        assert functions.evaluate(exponential, [1.0]) == pytest.approx([0, 0.2, 0.8, 0.1])
        assert functions.evaluate(exponential, [0.5]) == pytest.approx([0, 0.1, 0.4, 0.05])
        assert functions.evaluate(calculator, [1.0]) == pytest.approx([0.4, 0.2, 0, 0])
        assert functions.evaluate(sampled, [1.0]) == pytest.approx([1.0, 128 / 255])
        assert functions.evaluate(_fn(FunctionType=9), [1.0]) is None

    def test_a_separation_is_named_with_its_alternate_and_shows_in_the_palette(self, tmp_path):
        pdf = pikepdf.new()
        page = pdf.add_blank_page(page_size=(300, 200))
        tint = pdf.make_indirect(_fn(FunctionType=2, C0=[0, 0, 0, 0], C1=[0, 0.2, 0.8, 0.1], N=1))
        space = pikepdf.Array([pikepdf.Name.Separation, pikepdf.Name("/Gold"), pikepdf.Name.DeviceCMYK, tint])
        page.Resources = pikepdf.Dictionary(ColorSpace=pikepdf.Dictionary(CS0=space))
        page.Contents = pdf.make_stream(b"/CS0 cs 0.5 scn 10 10 50 50 re f")
        path = tmp_path / "spot.pdf"
        pdf.save(path)
        result = extract.extract_pdf(path, tmp_path / "out", kinds=("vector",))
        [spot] = result.spot_colours
        assert (spot["names"], spot["alternate_space"], spot["pages"]) == (["Gold"], "DeviceCMYK", [1])
        assert spot["alternate"] == pytest.approx([0, 0.2, 0.8, 0.1])
        [entry] = [e for e in result.palette if e["colour"] == "spot Gold 50%"]
        assert entry["panel_area_pt2"] == pytest.approx(2500) and entry["spot"]["names"] == ["Gold"]


class TestTables:
    def grid(self, tmp_path, *, merged_top: bool):
        # Three rows by two columns, 100 x 20 pt each row; with merged_top the middle rule is missing in the top row.
        rules = b"".join(f"10 {y} m 110 {y} l S ".encode() for y in (100, 80, 60, 40))
        rules += b"10 40 m 10 100 l S 110 40 m 110 100 l S "
        rules += b"60 40 m 60 80 l S " if merged_top else b"60 40 m 60 100 l S "
        cells = [(15, 85, "H" if merged_top else "A1"), (65, 85, "B1"), (15, 65, "A2"), (65, 65, "B2")]
        cells += [(15, 45, "A3"), (65, 45, "B3")]
        words = b" ".join(f"BT /F1 8 Tf 1 0 0 1 {x} {y} Tm ({t}) Tj ET".encode() for x, y, t in cells)
        return _page_pdf(tmp_path, b"0 G " + rules + words, font=True)

    def test_a_ruled_grid_is_a_table_with_its_cells(self, tmp_path):
        result = extract.extract_pdf(self.grid(tmp_path, merged_top=False), tmp_path / "out", kinds=("vector",))
        [table] = [t for p in result.layout.pages for t in p.tables]
        assert (table.mode, table.rows, table.cols) == ("grid", 3, 2)
        assert [c.text for c in table.cells] == ["A1", "B1", "A2", "B2", "A3", "B3"]

    def test_a_missing_rule_merges_cells_into_a_span(self, tmp_path):
        result = extract.extract_pdf(self.grid(tmp_path, merged_top=True), tmp_path / "out", kinds=("vector",))
        [table] = [t for p in result.layout.pages for t in p.tables]
        [head] = [c for c in table.cells if c.row == 0]
        assert (head.col, head.colspan, head.text) == (0, 2, "H B1") and len(table.cells) == 5

    def test_text_in_a_table_is_not_counted_as_body_text(self, tmp_path):
        result = extract.extract_pdf(self.grid(tmp_path, merged_top=False), tmp_path / "out", kinds=("vector",))
        [page] = result.layout.pages
        assert page.blocks and all(b.table == page.tables[0].id for b in page.blocks)

    def test_a_frame_with_no_text_is_not_a_table(self, tmp_path):
        rules = b"0 G 10 10 m 110 10 l S 10 30 m 110 30 l S 10 10 m 10 30 l S 110 10 m 110 30 l S 60 10 m 60 30 l S"
        result = extract.extract_pdf(_page_pdf(tmp_path, rules), tmp_path / "out", kinds=("vector",))
        assert [t for p in result.layout.pages for t in p.tables] == []


class TestTaggedStructure:
    @pytest.fixture
    def tagged_pdf(self, tmp_path):
        pdf = pikepdf.new()
        page = pdf.add_blank_page(page_size=(300, 200))
        helvetica = pdf.make_indirect(
            pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name.Helvetica)
        )
        page.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=helvetica))
        page.Contents = pdf.make_stream(
            b"/H1 <</MCID 0>> BDC BT /F1 20 Tf 10 150 Td (Title) Tj ET EMC "
            b"/Custom <</MCID 1>> BDC BT /F1 10 Tf 10 120 Td (Body one) Tj ET EMC "
            b"/Figure <</MCID 2>> BDC 0 g 10 10 m 30 10 l 20 30 l f EMC "
            b"/Artifact <</Type /Pagination /Subtype /Footer>> BDC BT /F1 8 Tf 10 5 Td (Page 1) Tj ET EMC"
        )

        def element(tag: str, kids, **more) -> pikepdf.Object:
            return pdf.make_indirect(
                pikepdf.Dictionary(Type=pikepdf.Name.StructElem, S=pikepdf.Name(f"/{tag}"), Pg=page.obj, K=kids, **more)
            )

        document = element(
            "Document",
            pikepdf.Array(
                [
                    element("H1", 0),
                    element(
                        "Custom",
                        1,
                        A=pikepdf.Dictionary(O=pikepdf.Name.Layout, SpaceAfter=6, TextAlign=pikepdf.Name.Justify),
                    ),
                    element("Figure", 2, Alt=pikepdf.String("A triangle")),
                ]
            ),
        )
        pdf.Root.StructTreeRoot = pdf.make_indirect(
            pikepdf.Dictionary(
                Type=pikepdf.Name.StructTreeRoot, K=document, RoleMap=pikepdf.Dictionary(Custom=pikepdf.Name.P)
            )
        )
        pdf.Root.MarkInfo = pikepdf.Dictionary(Marked=True)
        path = tmp_path / "tagged.pdf"
        pdf.save(path)
        return path

    def test_the_tree_is_read_with_roles_through_the_rolemap_and_the_producers_attributes(self, tmp_path, tagged_pdf):
        result = extract.extract_pdf(tagged_pdf, tmp_path / "out", kinds=())
        elements = result.structure["elements"]
        assert result.structure["tagged"] and [e["role"] for e in elements] == ["Document", "H1", "P", "Figure"]
        body = elements[2]
        assert (body["tag"], body["text"], body["attributes"]) == (
            "Custom",
            "Body one",
            {"SpaceAfter": 6.0, "TextAlign": "Justify"},
        )
        assert elements[1]["text"] == "Title" and elements[1]["box"][1] == pytest.approx(150, abs=5)

    def test_text_carries_its_tag_and_page_furniture_says_so(self, tmp_path, tagged_pdf):
        result = extract.extract_pdf(tagged_pdf, tmp_path / "out", kinds=())
        by_text = {run.text: run for run in result.text}
        assert (by_text["Title"].tag, by_text["Body one"].tag) == ("H1", "P")
        assert (by_text["Page 1"].tag, by_text["Page 1"].artifact) == (None, "Footer")
        footer = next(b for p in result.layout.pages for b in p.blocks if b.text == "Page 1")
        assert footer.running  # the PDF marked it as a footer

    def test_a_heading_tag_sets_the_role_and_a_figure_gives_its_alt_text_to_the_piece(self, tmp_path, tagged_pdf):
        result = extract.extract_pdf(tagged_pdf, tmp_path / "out", kinds=("vector",))
        title = next(s for s in result.layout.styles if s.size_pt == 20.0)
        assert title.role == "heading-1" and title.tags == {"H1": 1}
        [piece] = [a for a in result.assets if a.kind == "vector"]
        assert (piece.details["alt"], piece.details["tag"]) == ("A triangle", "Figure")

    def test_an_untagged_file_says_so(self, tmp_path):
        result = extract.extract_pdf(PDFS / "good.pdf", tmp_path, kinds=())
        assert result.structure["tagged"] is False and result.structure["elements"] == []


def test_font_descriptor_facts_read_flags_weight_and_slant():
    descriptor = pikepdf.Dictionary(
        Flags=(1 << 18) | 2, ItalicAngle=-12, FontWeight=400, FontFamily=pikepdf.String("Acme")
    )
    facts = extract._descriptor_facts(descriptor, "Acme-Regular")
    assert (facts["bold"], facts["italic"], facts["family"], facts["flags"]) == (
        True,
        True,
        "Acme",
        ["serif", "force-bold"],
    )
    plain = extract._descriptor_facts(pikepdf.Dictionary(Flags=32, FontWeight=700), "Acme-Black")
    assert (plain["bold"], plain["italic"], plain["flags"]) == (True, False, ["nonsymbolic"])


def test_a_schematic_of_mostly_wires_is_one_piece_because_the_wires_use_the_same_pen(tmp_path):
    # An outlined triangle and four wires, all stroked 1 pt black: one drawing, though only 1 of 5 shapes is not a line.
    content = (
        b"0 G 1 w 20 100 m 20 130 l 60 115 l h S 5 115 m 20 115 l S 60 115 m 100 115 l S "
        b"100 115 m 100 150 l S 100 150 m 40 150 l S"
    )
    result = extract.extract_pdf(_page_pdf(tmp_path, content), tmp_path / "out", kinds=("vector",))
    assert [a.details["shapes"] for a in result.assets if a.kind == "vector"] == [5] and result.boxes == []


class TestLabelsNearADrawing:
    DRAWING = (  # a package outline: a body, an outlined pin-1 marker and three legs, all one pen
        b"0 G 1 w 100 50 40 60 re S 100 110 m 106 110 l 103 116 l h S "
        b"94 60 m 100 60 l S 94 75 m 100 75 l S 94 90 m 100 90 l S "
    )

    def pieces(self, tmp_path, extra: bytes):
        result = extract.extract_pdf(
            _page_pdf(tmp_path, self.DRAWING + extra, font=True), tmp_path / "out", kinds=("vector",)
        )
        return [a for a in result.assets if a.kind == "vector"]

    def test_short_text_a_line_or_so_from_a_drawing_is_its_label(self, tmp_path):
        # "OFFSET NULL" ends 10 pt short of the legs: too far to touch, near enough to be a pin name.
        text = b"BT /F1 8 Tf 1 0 0 1 40 58 Tm (OFFSET NULL) Tj ET"
        [piece] = self.pieces(tmp_path, text)
        assert piece.details["text"] == ["OFFSET NULL"]
        assert piece.placements[0].x < 45  # the piece reaches out to the label

    def test_a_paragraph_beside_the_drawing_stays_text(self, tmp_path):
        paragraph = b"".join(
            f"BT /F1 8 Tf 1 0 0 1 150 {y} Tm (alpha alpha alpha) Tj ET ".encode() for y in (90, 80, 70)
        )
        [piece] = self.pieces(tmp_path, paragraph)
        assert piece.details["text"] == []

    def test_a_long_line_is_not_a_label(self, tmp_path):
        long_line = b"BT /F1 8 Tf 1 0 0 1 150 60 Tm (this line is far too long to be a pin name) Tj ET"
        [piece] = self.pieces(tmp_path, long_line)
        assert piece.details["text"] == []


def test_a_compound_hairline_path_too_big_for_a_background_still_joins_its_drawing(tmp_path):
    # Two outlined triangles joined by a wire, and three hairlines drawn as ONE path whose box is bigger than the cap.
    content = (
        b"0 G 1 w 20 20 m 25 20 l 22 25 l h S 60 20 m 65 20 l 62 25 l h S 25 22 m 60 22 l S "
        b"0.3 w 20 21 m 70 21 l 20 23 m 70 23 l 20 24 m 70 24 l S"
    )
    path = _page_pdf(tmp_path, content)
    result = extract.extract_pdf(path, tmp_path / "out", kinds=("vector",), max_fraction=0.001)
    assert [a.details["shapes"] for a in result.assets if a.kind == "vector"] == [4]


def test_a_state_set_again_keeps_the_latest_value_last():
    from proof_pdf._internal import vector

    old = vector._StateOp("w", (1,))
    line = vector._put({"a": old, "w": old}, "a", vector._StateOp("w", (2,)))
    assert list(line) == ["w", "a"]
