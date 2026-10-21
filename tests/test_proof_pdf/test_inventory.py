"""The facts ``inspect`` reports, read from the committed fixture PDFs (no external tools needed)."""

import json
import pathlib

import pikepdf
import pytest
import typer.testing

from proof_pdf import cli, inventory

PDFS = pathlib.Path(__file__).parents[1] / "fixtures" / "pdfs"


@pytest.fixture
def runner():
    return typer.testing.CliRunner()


class TestInventory:
    def test_a_graphics_only_card_reports_its_page_and_nothing_else(self):
        facts = inventory.inspect_pdf(PDFS / "good.pdf")
        [page] = facts.pages
        assert page.trim_mm == (105.0, 148.0) and page.media_mm == (111.0, 154.0)
        assert page.text_runs == 0 and page.smallest_text_pt is None and page.thinnest_stroke_pt is None
        assert (facts.fonts, facts.text_colours, facts.spot_inks, facts.images, facts.contrast) == ([], [], [], [], [])

    def test_fonts_are_listed_with_embedding_and_glyph_counts(self):
        [font] = inventory.inspect_pdf(PDFS / "cyan-text.pdf").fonts
        assert (font.name, font.subtype, font.embedded, font.has_to_unicode) == ("Helvetica", "Type1", False, False)
        assert font.glyphs == 4 and font.notdef == 0 and font.pages == (1,)

    def test_a_notdef_glyph_is_counted_against_its_font(self):
        [font] = inventory.inspect_pdf(PDFS / "notdef-text.pdf").fonts
        assert (font.name, font.subtype, font.glyphs, font.notdef) == ("Nunito", "Type0", 2, 1)

    def test_text_colours_and_the_smallest_size(self):
        [colour] = inventory.inspect_pdf(PDFS / "small-text.pdf").text_colours
        assert colour.colour == "gray 0%" and colour.runs == 1 and colour.smallest_pt == pytest.approx(4.0)

    def test_images_report_colour_space_pixels_and_effective_dpi(self):
        [image] = inventory.inspect_pdf(PDFS / "rgb-image.pdf").images
        assert (image.colour_space, image.pixels, image.name) == ("rgb", (500, 500), "/Im1")
        assert image.dpi == pytest.approx(360.0, abs=0.5)  # 500 px over 100 pt

    def test_the_thinnest_stroke_is_reported(self):
        [page] = inventory.inspect_pdf(PDFS / "hairline.pdf").pages
        assert page.thinnest_stroke_pt == 0.0

    def test_contrast_pairs_carry_l_star_and_ciede2000(self):
        [low] = [c for c in inventory.inspect_pdf(PDFS / "low-contrast.pdf").contrast if c.text == "gray 80%"]
        assert low.background == "gray 90%" and low.delta_l is not None and 5 < low.delta_l < 15
        assert low.delta_e is not None and low.delta_e > 0
        [high] = inventory.inspect_pdf(PDFS / "high-contrast.pdf").contrast
        assert high.text == "gray 0%" and high.delta_l is not None and high.delta_l > 70

    def test_cmyk_text_without_a_cmyk_profile_says_why_instead_of_failing(self):
        facts = inventory.inspect_pdf(PDFS / "cyan-text.pdf")
        assert facts.contrast == [] and facts.contrast_error and "no CMYK profile" in facts.contrast_error

    def test_contrast_can_be_skipped(self):
        facts = inventory.inspect_pdf(PDFS / "high-contrast.pdf", with_contrast=False)
        assert facts.contrast == [] and facts.contrast_error is None

    def test_spot_inks_are_listed(self, tmp_path):
        pdf = pikepdf.new()
        spaces = pikepdf.Dictionary(
            CS1=pikepdf.Array([pikepdf.Name.Separation, pikepdf.Name("/PANTONE 185 C"), pikepdf.Name.DeviceCMYK])
        )
        page = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            MediaBox=[0, 0, 100, 100],
            Contents=pdf.make_stream(b"/CS1 cs 1 scn 0 0 5 5 re f"),
            Resources=pikepdf.Dictionary(ColorSpace=spaces),
        )
        pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        pdf.save(tmp_path / "spot.pdf")
        assert inventory.inspect_pdf(tmp_path / "spot.pdf").spot_inks == ["PANTONE 185 C"]

    def test_a_file_that_will_not_open_raises(self):
        with pytest.raises(pikepdf.PdfError):
            inventory.inspect_pdf(PDFS / "corrupt.pdf")

    async def test_the_async_wrapper_matches(self):
        assert await inventory.inspect_file(PDFS / "good.pdf") == inventory.inspect_pdf(PDFS / "good.pdf")

    def test_to_dict_is_plain_json(self):
        json.dumps(inventory.inspect_pdf(PDFS / "low-contrast.pdf").to_dict())


class TestInspectCommand:
    def test_json_is_the_inventory(self, runner):
        result = runner.invoke(cli.pdf_app, ["inspect", str(PDFS / "rgb-image.pdf"), "--json"])
        [record] = json.loads(result.stdout)
        assert result.exit_code == 0 and record["images"][0]["colour_space"] == "rgb"

    def test_the_report_reads_as_a_summary(self, runner):
        result = runner.invoke(cli.pdf_app, ["inspect", str(PDFS / "low-contrast.pdf")])
        assert result.exit_code == 0
        assert "trim 105x148mm" in result.stdout and "font Helvetica (Type1, NOT embedded" in result.stdout
        assert "contrast gray 80% on gray 90%: L* diff" in result.stdout

    def test_no_contrast_skips_the_measurement(self, runner):
        result = runner.invoke(cli.pdf_app, ["inspect", str(PDFS / "low-contrast.pdf"), "--no-contrast"])
        assert "L* diff" not in result.stdout and "not measured" not in result.stdout

    def test_an_unreadable_file_fails_the_run_but_not_the_others(self, runner):
        result = runner.invoke(cli.pdf_app, ["inspect", str(PDFS / "corrupt.pdf"), str(PDFS / "good.pdf"), "--json"])
        assert result.exit_code == 1
        assert [pathlib.Path(r["path"]).name for r in json.loads(result.stdout)] == ["good.pdf"]

    def test_no_targets_is_an_error(self, runner):
        assert runner.invoke(cli.pdf_app, ["inspect"]).exit_code == 1


class TestTexlogCommand:
    LOG = (
        "Missing character: There is no ♥ (U+2665) in font Nunito!\n"
        "Overfull \\hbox (12.5pt too wide) in paragraph at lines 4--5\n"
    )

    def test_errors_fail_the_run(self, runner, tmp_path):
        log = tmp_path / "b.log"
        log.write_text(self.LOG)
        result = runner.invoke(cli.pdf_app, ["texlog", str(log)])
        assert result.exit_code == 1 and "missing-character" in result.stdout and "1 of 1 log(s)" in result.stdout

    def test_warnings_alone_pass(self, runner, tmp_path):
        log = tmp_path / "b.log"
        log.write_text("Overfull \\hbox (12.5pt too wide) in paragraph at lines 4--5\n")
        assert runner.invoke(cli.pdf_app, ["texlog", str(log)]).exit_code == 0

    def test_an_overfull_limit_promotes_it(self, runner, tmp_path):
        log = tmp_path / "b.log"
        log.write_text("Overfull \\hbox (12.5pt too wide) in paragraph at lines 4--5\n")
        assert runner.invoke(cli.pdf_app, ["texlog", str(log), "--max-overfull-pt", "10"]).exit_code == 1

    def test_json_output(self, runner, tmp_path):
        log = tmp_path / "b.log"
        log.write_text(self.LOG)
        [record] = json.loads(runner.invoke(cli.pdf_app, ["texlog", str(log), "--json"]).stdout)
        assert record["ok"] is False and {f["check"] for f in record["findings"]} == {"missing-character", "overfull"}

    def test_no_logs_is_an_error(self, runner):
        assert runner.invoke(cli.pdf_app, ["texlog"]).exit_code == 1
