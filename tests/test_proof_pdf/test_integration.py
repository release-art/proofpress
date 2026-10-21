"""The real thing: the CLI and library against real veraPDF and Ghostscript on real (small) PDFs.

``tests/fixtures/build_pdfs.py`` builds one passing PDF and a set of PDFs that each break exactly one thing. These tests
assert which checks fire on which file, end to end: argument parsing, the subprocesses, the in-process PDF checks
and the report. They skip when veraPDF or Ghostscript is missing; ``bin/docker/test.sh`` runs them in the image.
"""

import json
import pathlib
import shutil
import subprocess
import typing

import pytest
import typer.testing
from PIL import Image

from proof_pdf import cli, core, preview
from proof_pdf.models import PageGeometry, Profile

from .test_contrast import CMYK_PROFILE

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


PROOFING = ["--no-pdfa", "--no-pdfx", "--no-colour"]  # isolate the proofing checks from the standards layers


class TestProofingChecks:
    """Each fixture breaks exactly one proofing rule; the rule fires only when its threshold is supplied."""

    def test_missing_glyphs_are_checked_by_default(self, runner):
        code, [record] = _verify(runner, PDFS / "notdef-text.pdf", *PROOFING)
        assert code == 1 and _checks(record) == {"missing-glyph"}
        assert "Nunito" in _messages(record, "missing-glyph")[0]

    def test_and_can_be_turned_off(self, runner):
        code, [record] = _verify(runner, PDFS / "notdef-text.pdf", *PROOFING, "--no-missing-glyphs")
        assert (code, record["findings"]) == (0, [])

    @pytest.mark.parametrize(
        ("fixture", "flags", "check"),
        [
            ("small-text", ["--min-text-pt", "7"], "text-size"),
            ("hairline", ["--min-stroke-pt", "0.25"], "hairline"),
            ("edge-text", ["--safe-mm", "5"], "safe-area"),
            ("low-contrast", ["--min-delta-l", "30"], "contrast"),
        ],
    )
    def test_each_rule_fires_when_asked(self, runner, fixture, flags, check):
        code, [record] = _verify(runner, PDFS / f"{fixture}.pdf", *PROOFING, *flags)
        assert code == 1 and _checks(record) == {check}

    @pytest.mark.parametrize("fixture", ["small-text", "hairline", "edge-text", "low-contrast"])
    def test_and_stays_silent_without_a_threshold(self, runner, fixture):
        code, [record] = _verify(runner, PDFS / f"{fixture}.pdf", *PROOFING)
        assert (code, record["findings"]) == (0, [])

    def test_a_looser_threshold_lets_the_same_file_through(self, runner):
        assert _verify(runner, PDFS / "low-contrast.pdf", *PROOFING, "--min-delta-l", "5")[0] == 0
        assert _verify(runner, PDFS / "edge-text.pdf", *PROOFING, "--safe-mm", "0.5")[0] == 0

    def test_high_contrast_text_and_a_clean_card_pass_every_rule(self, runner):
        every = ["--min-text-pt", "7", "--min-stroke-pt", "0.25", "--safe-mm", "5", "--min-delta-l", "30"]
        for name in ("high-contrast", "good"):
            code, [record] = _verify(runner, PDFS / f"{name}.pdf", *PROOFING, *every)
            assert (code, record["findings"]) == (0, []), name

    def test_cmyk_text_needs_a_cmyk_profile_to_be_measured(self, runner):
        code, [record] = _verify(runner, PDFS / "black-text.pdf", *PROOFING, "--min-delta-l", "30")
        assert code == 1 and "no CMYK profile" in _messages(record, "contrast")[0]

    @pytest.mark.skipif(CMYK_PROFILE is None, reason="no CMYK ICC profile on this machine")
    def test_and_is_measured_through_a_supplied_one(self, runner):
        args = [*PROOFING, "--min-delta-l", "30", "--proof-profile", str(CMYK_PROFILE or "")]
        code, [record] = _verify(runner, PDFS / "black-text.pdf", *args)
        assert (code, record["findings"]) == (0, [])


class TestInspectOnRealFiles:
    def test_inspect_agrees_with_verify_about_the_same_file(self, runner):
        inspected = json.loads(runner.invoke(cli.pdf_app, ["inspect", str(PDFS / "rgb-image.pdf"), "--json"]).stdout)
        _, [verified] = _verify(runner, PDFS / "rgb-image.pdf", *A6)
        assert inspected[0]["images"][0]["colour_space"] == "rgb" and "colour-space" in _checks(verified)


LUALATEX = shutil.which("lualatex")
TEX = r"""\documentclass{article}
\usepackage{fontspec}
\begin{document}
Hello \symbol{"2665} world \ref{nowhere}.
\end{document}
"""


@pytest.fixture(scope="module")
def lualatex_build(tmp_path_factory):
    directory = tmp_path_factory.mktemp("lualatex")
    (directory / "doc.tex").write_text(TEX)
    subprocess.run(
        [LUALATEX or "lualatex", "-interaction=nonstopmode", "doc.tex"],
        cwd=directory,
        capture_output=True,
        check=True,
        timeout=200,
    )
    return directory


@pytest.mark.skipif(LUALATEX is None, reason="needs lualatex")
@pytest.mark.timeout(240)
class TestRealLuaLatexBuild:
    """A document with a character Latin Modern lacks and a dangling reference, built by the real engine."""

    def test_the_log_names_the_dropped_character_and_the_reference(self, runner, lualatex_build):
        result = runner.invoke(cli.pdf_app, ["texlog", str(lualatex_build / "doc.log"), "--json"])
        [record] = json.loads(result.stdout)
        assert result.exit_code == 1
        assert {"missing-character", "undefined-reference"} <= _checks(record)
        assert "U+2665" in _messages(record, "missing-character")[0]

    def test_the_pdf_shows_the_notdef_glyph_the_engine_typeset(self, runner, lualatex_build):
        code, [record] = _verify(runner, lualatex_build / "doc.pdf", *PROOFING)
        assert code == 1 and "LMRoman10-Regular" in _messages(record, "missing-glyph")[0]

    def test_inspect_lists_the_embedded_fonts_and_the_notdef(self, runner, lualatex_build):
        [record] = json.loads(runner.invoke(cli.pdf_app, ["inspect", str(lualatex_build / "doc.pdf"), "--json"]).stdout)
        regular = next(f for f in record["fonts"] if f["name"] == "LMRoman10-Regular")
        assert regular["embedded"] and regular["has_to_unicode"] and regular["notdef"] == 1
        assert record["contrast"][0]["background"] == "paper" and record["contrast"][0]["delta_l"] > 95
