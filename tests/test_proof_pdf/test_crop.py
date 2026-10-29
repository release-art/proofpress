"""Cutting a rectangle of a page out as a one-page PDF."""

import pathlib

import pikepdf
import pytest
import typer.testing

from proof_pdf import cli, crop, extract

PDFS = pathlib.Path(__file__).parents[1] / "fixtures" / "pdfs"
IMAGE_PDF = PDFS / "rgb-image.pdf"  # a 100 x 100 pt image placed at (28.5, 28.5)


@pytest.fixture
def runner():
    return typer.testing.CliRunner()


def boxes(path: pathlib.Path) -> tuple[list[float], list[float]]:
    with pikepdf.open(path) as pdf:
        page = pdf.pages[0]
        return [round(float(v), 2) for v in page.mediabox], [round(float(v), 2) for v in page.cropbox]


class TestCropPage:
    def test_the_box_becomes_the_whole_of_a_one_page_pdf(self, tmp_path):
        out = crop.crop_page(IMAGE_PDF, 1, (28.5, 28.5, 128.5, 128.5), tmp_path / "piece.pdf")
        window = [28.5, 28.5, 128.5, 128.5]
        assert boxes(out) == (window, window)
        with pikepdf.open(out) as pdf:
            assert len(pdf.pages) == 1 and "/TrimBox" not in pdf.pages[0].obj

    def test_the_content_is_the_pages_own_so_a_placed_image_survives(self, tmp_path):
        out = crop.crop_page(IMAGE_PDF, 1, (28.5, 28.5, 128.5, 128.5), tmp_path / "piece.pdf")
        result = extract.extract_pdf(out, tmp_path / "again", kinds=("images",))
        assert [a.details["pixels"] for a in result.assets if a.kind == "image"] == [[500, 500]]

    def test_corners_in_either_order_and_padding_stay_within_the_page(self, tmp_path):
        out = crop.crop_page(IMAGE_PDF, 1, (128.5, 128.5, 28.5, 28.5), tmp_path / "a.pdf", pad_pt=1000)
        with pikepdf.open(IMAGE_PDF) as pdf:
            media = [round(float(v), 2) for v in pdf.pages[0].mediabox]
        assert boxes(out)[0] == media  # padded far past the edge, kept to the page

    def test_text_in_the_window_stays_text(self, tmp_path):
        out = crop.crop_page(PDFS / "cyan-text.pdf", 1, (0, 0, 400, 400), tmp_path / "t.pdf")
        assert [run.text for run in extract.extract_pdf(out, tmp_path / "x", kinds=()).text] == ["cyan"]

    def test_links_are_not_carried_into_the_piece(self, tmp_path):
        pdf = pikepdf.new()
        page = pdf.add_blank_page(page_size=(200, 200))
        page.Annots = pikepdf.Array(
            [
                pdf.make_indirect(
                    pikepdf.Dictionary(Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Link, Rect=[10, 10, 50, 30])
                )
            ]
        )
        source = tmp_path / "linked.pdf"
        pdf.save(source)
        out = crop.crop_page(source, 1, (0, 0, 100, 100), tmp_path / "piece.pdf")
        with pikepdf.open(out) as piece:
            assert "/Annots" not in piece.pages[0].obj

    @pytest.mark.parametrize("page", [0, 2])
    def test_a_page_that_is_not_there_is_a_value_error(self, tmp_path, page):
        with pytest.raises(ValueError, match="no page"):
            crop.crop_page(IMAGE_PDF, page, (0, 0, 10, 10), tmp_path / "x.pdf")

    def test_a_box_off_the_page_is_a_value_error_that_says_where_the_page_is(self, tmp_path):
        with pytest.raises(ValueError, match="outside page 1"):
            crop.crop_page(IMAGE_PDF, 1, (5000, 5000, 6000, 6000), tmp_path / "x.pdf")


class TestCli:
    def test_crop_writes_the_piece_beside_the_file_by_default_name(self, runner, tmp_path):
        source = tmp_path / "doc.pdf"
        source.write_bytes(IMAGE_PDF.read_bytes())
        result = runner.invoke(cli.pdf_app, ["crop", str(source), "--page", "1", "--box", "28.5,28.5,128.5,128.5"])
        assert result.exit_code == 0, result.output
        assert (tmp_path / "doc-p1-28-28.pdf").is_file() and "wrote" in result.output

    def test_a_malformed_box_is_a_usage_error(self, runner, tmp_path):
        result = runner.invoke(
            cli.pdf_app, ["crop", str(IMAGE_PDF), "--page", "1", "--box", "1,2,3", "--out", str(tmp_path / "o.pdf")]
        )
        assert result.exit_code == 2

    def test_a_missing_page_fails_the_run(self, runner, tmp_path):
        args = ["crop", str(IMAGE_PDF), "--page", "9", "--box", "0,0,10,10", "--out", str(tmp_path / "o.pdf")]
        result = runner.invoke(cli.pdf_app, args)
        assert result.exit_code == 1 and not (tmp_path / "o.pdf").exists()
