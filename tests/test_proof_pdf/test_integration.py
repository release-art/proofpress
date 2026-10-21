"""The real thing: the CLI and library against real veraPDF and Ghostscript on real (small) PDFs.

``tests/fixtures/build_pdfs.py`` builds one passing PDF and a set of PDFs that each break exactly one thing. These tests
assert which checks fire on which file, end to end: argument parsing, the subprocesses, the in-process PDF checks
and the report. They skip when veraPDF or Ghostscript is missing; ``bin/docker/test.sh`` runs them in the image.
"""

import json
import pathlib
import shutil
import typing

import pytest
import typer.testing
from PIL import Image

from proof_pdf import cli, core, preview
from proof_pdf.models import PageGeometry, Profile

pytestmark = [
    pytest.mark.container,
    pytest.mark.skipif(
        shutil.which("verapdf") is None or shutil.which("gs") is None, reason="needs veraPDF and Ghostscript on PATH"
    ),
    pytest.mark.timeout(120),
]

PDFS = pathlib.Path(__file__).parents[1] / "fixtures" / "pdfs"
A6 = ["--trim-mm", "105x148", "--bleed-mm", "3"]


@pytest.fixture
def runner():
    return typer.testing.CliRunner()


def _verify(runner, *args: object) -> tuple[int, list[dict]]:
    """Run ``proof-pdf verify ... --json`` and return the exit code and the parsed records."""
    result = runner.invoke(cli.pdf_app, ["verify", *map(str, args), "--json"])
    return result.exit_code, json.loads(result.stdout)


def _checks(record: dict, *, severity: str | None = None) -> set[str]:
    return {f["check"] for f in record["findings"] if severity in (None, f["severity"])}


def _colours(png: pathlib.Path) -> set[typing.Any]:
    with Image.open(png) as image:
        return {pixel for _, pixel in image.convert("RGB").getcolors(maxcolors=1_000_000) or []}


def _messages(record: dict, check: str) -> list[str]:
    return [f["message"] for f in record["findings"] if f["check"] == check]


class TestPassingFile:
    def test_the_good_card_passes_every_layer(self, runner):
        code, [record] = _verify(runner, PDFS / "good.pdf", *A6)
        assert (code, record["ok"], record["findings"]) == (0, True, [])

    def test_and_the_k_only_rule(self, runner):
        code, [record] = _verify(runner, PDFS / "good.pdf", *A6, "--k-only-text")
        assert (code, record["findings"]) == (0, [])

    async def test_through_the_library(self):
        profile = Profile(geometry=PageGeometry(trim_w_mm=105, trim_h_mm=148, bleed_mm=3))
        report = await core.verify_file(PDFS / "good.pdf", profile)
        assert report.ok and report.findings == []


class TestOffendingFiles:
    def test_a_default_latex_style_file_fails_the_standards(self, runner):
        code, [record] = _verify(runner, PDFS / "plain.pdf", *A6)
        assert code == 1
        assert {"pdfa", "trimbox", "trapped", "pdfx-xmp", "pdfx-intent"} <= _checks(record)

    def test_the_wrong_trim_size_is_caught_by_geometry_alone(self, runner):
        code, [record] = _verify(runner, PDFS / "wrong-size.pdf", *A6)
        assert code == 1
        assert _checks(record) == {"geometry", "bleed"}  # veraPDF passes it: the standards are satisfied

    def test_geometry_is_skipped_without_trim(self, runner):
        code, [record] = _verify(runner, PDFS / "wrong-size.pdf")
        assert (code, record["ok"]) == (0, True)

    def test_an_rgb_image_is_a_colour_space_error(self, runner):
        code, [record] = _verify(runner, PDFS / "rgb-image.pdf", *A6)
        assert code == 1
        assert _checks(record) == {"colour-space"}
        assert "/Im1" in _messages(record, "colour-space")[0]

    def test_a_low_resolution_image_is_an_image_dpi_error(self, runner):
        code, [record] = _verify(runner, PDFS / "low-dpi-image.pdf", *A6)
        assert code == 1
        assert _checks(record) == {"image-dpi"}

    def test_min_dpi_is_adjustable(self, runner):
        code, [record] = _verify(runner, PDFS / "low-dpi-image.pdf", *A6, "--min-dpi", "1")
        assert (code, record["findings"]) == (0, [])

    def test_heavy_ink_exceeds_the_limit_in_a_real_ghostscript_raster(self, runner):
        code, [record] = _verify(runner, PDFS / "heavy-ink.pdf", *A6, "--no-pdfa")
        assert code == 1
        assert _checks(record) == {"tac"}
        assert "exceeds the 300% limit" in _messages(record, "tac")[0]

    def test_a_higher_ink_ceiling_lets_it_through(self, runner):
        code, [record] = _verify(runner, PDFS / "heavy-ink.pdf", *A6, "--no-pdfa", "--max-tac", "400")
        assert (code, record["findings"]) == (0, [])

    def test_a_truncated_file_fails_cleanly_with_a_readable_message(self, runner):
        code, [record] = _verify(runner, PDFS / "corrupt.pdf", *A6)
        assert code == 1
        assert "cannot open the PDF" in _messages(record, "structure")[0]
        [pdfa_message] = _messages(record, "pdfa")
        assert "valid PDF" in pdfa_message and "Final field" not in pdfa_message  # no JVM noise


class TestOutputIntentHint:
    def test_verapdf_failure_names_the_cause(self, runner):
        code, [record] = _verify(runner, PDFS / "no-output-intent.pdf", *A6)
        assert code == 1
        hinted = [m for m in _messages(record, "pdfa") if "6.2.4.3" in m]
        assert hinted and "hint: the file has no PDF/A output intent" in hinted[0]
        assert "pdfx-intent" in _checks(record)


class TestTextRules:
    """The text fixtures use a non-embedded font and DeviceCMYK, so veraPDF rightly fails them: run with --no-pdfa."""

    def test_cyan_text_under_overprint_fails_k_only_and_warns_overprint(self, runner):
        code, [record] = _verify(runner, PDFS / "cyan-text.pdf", *A6, "--no-pdfa", "--k-only-text")
        assert code == 1
        assert _checks(record, severity="error") == {"k-only-text"}
        assert _checks(record, severity="warn") == {"overprint"}
        assert "C100" in _messages(record, "k-only-text")[0]

    def test_k_only_is_opt_in(self, runner):
        code, [record] = _verify(runner, PDFS / "cyan-text.pdf", *A6, "--no-pdfa")
        assert _checks(record) == {"overprint"} and code == 0  # advisory only

    def test_overprinting_100_percent_k_text_is_clean(self, runner):
        code, [record] = _verify(runner, PDFS / "black-text.pdf", *A6, "--no-pdfa", "--k-only-text")
        assert (code, record["findings"]) == (0, [])

    def test_rich_black_small_text_is_a_warning_that_does_not_fail(self, runner):
        code, [record] = _verify(runner, PDFS / "rich-black-text.pdf", *A6, "--no-pdfa")
        assert code == 0
        assert _checks(record, severity="warn") == {"rich-black"} and _checks(record, severity="error") == set()


class TestDirectoriesAndExemptions:
    def test_exempt_files_are_listed_and_do_not_fail_the_run(self, runner, tmp_path):
        for name in ("good", "plain"):
            shutil.copy(PDFS / f"{name}.pdf", tmp_path / f"{name}.pdf")
        code, records = _verify(runner, tmp_path, *A6, "--exempt", "plain.pdf=known non-conformant sample")
        by_name = {pathlib.Path(r["path"]).name: r for r in records}
        assert code == 0
        assert by_name["plain.pdf"]["exempt"] == "known non-conformant sample" and by_name["plain.pdf"]["ok"]
        assert by_name["good.pdf"]["exempt"] is None and by_name["good.pdf"]["findings"] == []

    def test_the_same_file_without_the_exemption_fails(self, runner, tmp_path):
        shutil.copy(PDFS / "plain.pdf", tmp_path / "plain.pdf")
        code, _ = _verify(runner, tmp_path, *A6)
        assert code == 1

    def test_exclude_drops_files_without_listing_them(self, runner, tmp_path):
        for name in ("good", "plain"):
            shutil.copy(PDFS / f"{name}.pdf", tmp_path / f"{name}.pdf")
        code, records = _verify(runner, tmp_path, *A6, "--exclude", "plain.pdf")
        assert code == 0 and [pathlib.Path(r["path"]).name for r in records] == ["good.pdf"]

    def test_concurrency_does_not_change_any_result(self, runner):
        """Every fixture, checked one at a time and four at a time, gives identical reports in the same order."""
        files = sorted(PDFS.glob("*.pdf"))
        _, serial = _verify(runner, *files, "--no-pdfa", *A6, "-j", "1")
        _, parallel = _verify(runner, *files, "--no-pdfa", *A6, "-j", "4")
        assert serial == parallel and len(serial) == len(files)

    def test_the_human_report_summarises_a_mixed_run(self, runner, tmp_path):
        for name in ("good", "wrong-size"):
            shutil.copy(PDFS / f"{name}.pdf", tmp_path / f"{name}.pdf")
        result = runner.invoke(cli.pdf_app, ["verify", str(tmp_path), *A6])
        assert result.exit_code == 1
        assert "OK" in result.stdout and "FAIL" in result.stdout
        assert "1 of 2 file(s) failed" in result.stdout


class TestPreview:
    def test_renders_pages_with_the_guides_from_the_files_own_boxes(self, runner, tmp_path):
        result = runner.invoke(cli.pdf_app, ["preview", str(PDFS / "good.pdf"), "--out", str(tmp_path), "--dpi", "72"])
        assert result.exit_code == 0, result.stdout
        [png] = tmp_path.glob("*.png")
        assert preview._TRIM_RGB in _colours(png)

    def test_no_guides_leaves_the_raster_plain(self, runner, tmp_path):
        runner.invoke(
            cli.pdf_app, ["preview", str(PDFS / "good.pdf"), "--out", str(tmp_path), "--dpi", "72", "--no-guides"]
        )
        [png] = tmp_path.glob("*.png")
        assert preview._TRIM_RGB not in _colours(png)
