"""The ``proof-pdf`` command surface: exit codes, layers, and readable output.

``verify`` is the Rust core's command (``rust/src/cli``), so these tests run it for real on the built fixtures; the
option parsing itself is unit-tested in Rust. The other commands are still Python's.
"""

import importlib.metadata
import json
import pathlib
import shutil

import pytest
import typer.testing

from proof_pdf import cli, preview
from proof_pdf.models import Finding, Severity

PDFS = pathlib.Path(__file__).parents[1] / "fixtures" / "pdfs"
GOOD = str(PDFS / "x4-good.pdf")


@pytest.fixture
def runner():
    return typer.testing.CliRunner()


def _pdf(tmp_path: pathlib.Path, name: str) -> pathlib.Path:
    path = tmp_path / name
    path.write_bytes(b"%PDF-1.7\n")
    return path


def _verify(runner, *args: object):
    return runner.invoke(cli.pdf_app, ["verify", *map(str, args)])


class TestVerifyCommand:
    def test_a_conforming_file_exits_zero(self, runner):
        result = _verify(runner, GOOD)
        assert result.exit_code == 0
        assert "OK -- all 1 file(s) passed; checked: PDF/X as claimed" in result.stdout

    def test_an_error_exits_one_and_names_the_check(self, runner):
        result = _verify(runner, PDFS / "x4-limit-nesting.pdf")
        assert result.exit_code == 1
        assert "[x4.limits.graphics]" in result.stdout and "FAIL" in result.stdout

    def test_a_warning_alone_still_passes(self, runner):
        result = _verify(runner, PDFS / "x4-name-other-not-utf8.pdf")
        assert result.exit_code == 0
        assert "advisory warning" in result.stdout

    def test_quiet_prints_nothing(self, runner):
        result = _verify(runner, GOOD, "--quiet")
        assert (result.exit_code, result.stdout) == (0, "")

    def test_no_targets_is_a_usage_error(self, runner):
        result = _verify(runner)
        assert result.exit_code == 64
        assert "no PDFs to verify" in result.stderr

    def test_a_refused_file_exits_two(self, runner):
        assert _verify(runner, PDFS / "x4-no-header.pdf", "--quiet").exit_code == 2

    def test_a_directory_argument_expands(self, runner, tmp_path):
        shutil.copy(GOOD, tmp_path / "a.pdf")
        shutil.copy(GOOD, tmp_path / "b.pdf")
        result = _verify(runner, tmp_path, "--json")
        assert [pathlib.Path(r["path"]).name for r in json.loads(result.stdout)] == ["a.pdf", "b.pdf"]

    def test_json_output_is_machine_readable(self, runner):
        result = _verify(runner, PDFS / "x4-limit-nesting.pdf", "--json")
        [record] = json.loads(result.stdout)
        assert (result.exit_code, record["ok"], record["verdict"]) == (1, False, "fail")
        [finding] = record["findings"]
        assert (finding["check"], finding["severity"], finding["page"]) == ("x4.limits.graphics", "error", 1)

    def test_help_comes_from_the_command(self, runner):
        result = _verify(runner, "--help")
        assert result.exit_code == 0 and "--trim-mm" in result.stdout


class TestOptions:
    def test_the_trim_size_turns_the_geometry_checks_on(self, runner):
        assert "[geometry]" not in _verify(runner, GOOD).stdout
        assert "[geometry]" in _verify(runner, GOOD, "--trim-mm", "210x297").stdout
        assert _verify(runner, GOOD, "--trim-mm", "105*148", "--bleed-mm", "3").exit_code == 0

    @pytest.mark.parametrize("value", ["85", "85x", "axb", "0x5"])
    def test_a_malformed_trim_is_a_usage_error(self, runner, value):
        result = _verify(runner, GOOD, "--trim-mm", value)
        assert result.exit_code == 64 and "--trim-mm" in result.stderr

    def test_the_warn_levels_must_sit_inside_the_limits(self, runner):
        assert "must be below --max-tac" in _verify(runner, GOOD, "--warn-tac", "300").stderr
        assert "must be above --min-dpi" in _verify(runner, GOOD, "--warn-dpi", "200").stderr

    def test_a_house_rule_is_off_until_asked(self, runner):
        cyan = PDFS / "cyan-text.pdf"
        assert "[k-only-text]" not in _verify(runner, cyan, "--no-pdfx").stdout
        assert "[k-only-text]" in _verify(runner, cyan, "--no-pdfx", "--k-only-text").stdout

    def test_the_python_spellings_of_the_defaults_still_work(self, runner):
        assert _verify(runner, GOOD, "--no-pdfa", "--colour", "--fonts", "--missing-glyphs").exit_code == 0

    def test_the_summary_names_the_layers_that_are_on(self, runner):
        result = _verify(runner, GOOD, "--safe-mm", "4", "--min-delta-l", "30", "--no-pdfx")
        assert "missing glyphs" in result.stdout and "safe area" in result.stdout and "contrast" in result.stdout
        assert "PDF/X-4" not in result.stdout

    def test_preview_draws_no_safe_guide_unless_asked(self, runner, tmp_path, mocker):
        calls = mocker.patch.object(cli.preview, "preview_file", return_value=[])
        runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf"))])
        assert calls.call_args.kwargs["safe_mm"] is None
        runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "b.pdf")), "--safe", "4"])
        assert calls.call_args.kwargs["safe_mm"] == 4.0


class TestExemptions:
    def test_exempt_files_are_listed_with_their_reason_and_pass(self, runner, tmp_path):
        shutil.copy(PDFS / "x4-limit-nesting.pdf", tmp_path / "manual.pdf")
        result = _verify(runner, tmp_path / "manual.pdf", "--exempt", "manual.pdf=internal docs")
        assert result.exit_code == 0
        assert "EXEMPT" in result.stdout and "internal docs" in result.stdout

    def test_exempt_files_are_in_the_json(self, runner):
        result = _verify(runner, GOOD, "--exempt", "x4-good.pdf=a proof", "--json")
        [record] = json.loads(result.stdout)
        assert (record["exempt"], record["findings"], record["ok"]) == ("a proof", [], True)

    @pytest.mark.parametrize("spec", ["nothing", "=reason", "glob="])
    def test_an_exemption_needs_a_glob_and_a_reason(self, runner, spec):
        result = _verify(runner, GOOD, "--exempt", spec)
        assert result.exit_code == 64 and "GLOB=reason" in result.stderr


class TestConfigFile:
    def _config(self, directory: pathlib.Path, text: str) -> pathlib.Path:
        path = directory / "proof-pdf.toml"
        path.write_text(text)
        return path

    def test_config_values_become_the_defaults(self, runner, tmp_path):
        config = self._config(tmp_path, '[verify]\ntrim-mm = "210x297"\n')
        result = runner.invoke(cli.pdf_app, ["--config", str(config), "verify", GOOD])
        assert "[geometry]" in result.stdout

    def test_a_flag_on_the_command_line_wins(self, runner, tmp_path):
        config = self._config(tmp_path, '[verify]\ntrim-mm = "210x297"\n')
        result = runner.invoke(cli.pdf_app, ["--config", str(config), "verify", GOOD, "--trim-mm", "105x148"])
        assert "[geometry]" not in result.stdout

    def test_a_config_can_switch_a_check_off(self, runner, tmp_path):
        config = self._config(tmp_path, "[verify]\npdfx = false\n")
        result = runner.invoke(cli.pdf_app, ["--config", str(config), "verify", str(PDFS / "x4-limit-nesting.pdf")])
        assert result.exit_code == 0

    def test_a_profile_is_chosen_with_dash_dash_profile(self, runner, tmp_path):
        config = self._config(tmp_path, '[profiles.big.verify]\ntrim-mm = "297x420"\n')
        result = runner.invoke(cli.pdf_app, ["--config", str(config), "--profile", "big", "verify", GOOD])
        assert "expected 297 x 420 mm" in result.stdout

    def test_the_nearest_file_is_found_from_the_working_directory(self, runner, tmp_path, monkeypatch):
        self._config(tmp_path, '[verify]\ntrim-mm = "210x297"\n')
        (tmp_path / "job").mkdir()
        monkeypatch.chdir(tmp_path / "job")
        assert "[geometry]" in _verify(runner, GOOD).stdout

    def test_the_environment_can_name_the_file_and_profile(self, runner, tmp_path, monkeypatch):
        config = self._config(tmp_path, '[profiles.big.verify]\ntrim-mm = "297x420"\n')
        monkeypatch.setenv("PROOF_PDF_CONFIG", str(config))
        monkeypatch.setenv("PROOF_PDF_PROFILE", "big")
        assert "expected 297 x 420 mm" in _verify(runner, GOOD).stdout

    def test_other_commands_have_their_own_tables(self, runner, tmp_path):
        config = self._config(tmp_path, "[verify]\nmin-text-pt = 6\n[texlog]\nmax-overfull-pt = 5\n")
        result = runner.invoke(cli.pdf_app, ["--config", str(config), "texlog"])
        assert result.exit_code == 1 and "no logs" in result.output

    def test_a_typo_in_the_file_is_an_error_not_a_silently_disabled_check(self, runner, tmp_path):
        config = self._config(tmp_path, "[verify]\nmin-txt-pt = 6\n")
        result = runner.invoke(cli.pdf_app, ["--config", str(config), "verify", GOOD])
        assert result.exit_code == 64 and "min-txt-pt" in result.stderr

    def test_a_profile_without_any_config_is_an_error(self, runner, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        monkeypatch.delenv("PROOF_PDF_CONFIG", raising=False)
        result = runner.invoke(cli.pdf_app, ["--profile", "x", "verify", GOOD])
        assert result.exit_code != 0


class TestCollapsedOutput:
    def test_identical_findings_across_pages_become_one_line(self, runner):
        result = _verify(runner, PDFS / "three-pages.pdf", "--no-pdfx", "--trim-mm", "10x10")
        assert "(pages 1-3)" in result.stdout

    @pytest.mark.parametrize(
        ("pages", "expected"),
        [
            ([1], "(page 1)"),
            ([1, 2, 3], "(pages 1-3)"),
            ([2, 5, 8], "(pages 2, 5, 8)"),
            ([1, 2, 3, 7, 8], "(pages 1-3, 7-8)"),
        ],
    )
    def test_page_ranges(self, pages, expected):
        finding = Finding("x", Severity.ERROR, "msg", pages[0])
        assert expected in cli._labelled(finding, pages)

    def test_pageless_findings_keep_their_plain_label(self):
        finding = Finding("pdfa", Severity.ERROR, "no identification")
        assert cli._labelled(finding, []) == "[pdfa] no identification"


class TestPreviewCommand:
    def test_writes_pages_and_reports_them(self, runner, tmp_path, mocker):
        written = tmp_path / "out" / "a-p1.png"

        async def _preview_file(target, dst, **kwargs):
            dst.mkdir(parents=True, exist_ok=True)
            written.parent.mkdir(parents=True, exist_ok=True)
            written.write_bytes(b"png")
            return [written]

        mocker.patch.object(cli.preview, "preview_file", side_effect=_preview_file)
        result = runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf")), "--out", str(tmp_path / "out")])
        assert result.exit_code == 0
        assert "1 page(s)" in result.output

    def test_default_output_sits_beside_the_pdf(self, runner, tmp_path, mocker):
        seen: list[pathlib.Path] = []

        async def _preview_file(target, dst, **kwargs):
            seen.append(dst)
            return []

        mocker.patch.object(cli.preview, "preview_file", side_effect=_preview_file)
        runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf"))])
        assert seen == [tmp_path / "preview"]

    def test_the_old_ghostscript_options_are_still_accepted(self, runner, tmp_path, mocker):
        mocker.patch.object(cli.preview, "preview_file", return_value=[])
        result = runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf")), "--gs", "gs", "--timeout", "9"])
        assert result.exit_code == 0, result.output

    @pytest.mark.parametrize("error", [preview.UnreadablePdfError("bad page"), ValueError("bad page")])
    def test_render_failure_exits_one(self, runner, tmp_path, mocker, error):
        async def _boom(*a, **k):
            raise error

        mocker.patch.object(cli.preview, "preview_file", side_effect=_boom)
        result = runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf"))])
        assert result.exit_code == 1
        assert "bad page" in result.output

    def test_no_targets_is_an_error(self, runner):
        assert runner.invoke(cli.pdf_app, ["preview"]).exit_code == 1


def test_relative_path_display(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    assert cli._rel(tmp_path / "sub" / "a.pdf") == str(pathlib.Path("sub/a.pdf"))
    # Anything outside the working tree is clearest as an absolute path.
    assert cli._rel(pathlib.Path("/etc/hosts")) == "/etc/hosts"


class TestVersion:
    def test_prints_the_package_version(self, runner, monkeypatch):
        monkeypatch.delenv("PROOFPRESS_REVISION", raising=False)
        result = runner.invoke(cli.pdf_app, ["--version"])
        assert result.exit_code == 0
        assert result.output.strip() == f"proof-pdf {importlib.metadata.version('proof-pdf')}"

    def test_includes_the_commit_the_image_recorded(self, runner, monkeypatch):
        monkeypatch.setenv("PROOFPRESS_REVISION", "0123456789abcdef0123")
        assert "(commit 0123456789ab)" in runner.invoke(cli.pdf_app, ["--version"]).output

    def test_unknown_revision_is_not_shown(self, runner, monkeypatch):
        monkeypatch.setenv("PROOFPRESS_REVISION", "unknown")
        assert "commit" not in runner.invoke(cli.pdf_app, ["--version"]).output


class TestMissingFiles:
    @pytest.mark.parametrize("command", ["inspect", "texlog"])
    def test_a_missing_file_is_one_line_not_a_traceback(self, runner, tmp_path, command):
        missing = tmp_path / ("gone.pdf" if command == "inspect" else "gone.log")
        result = runner.invoke(cli.pdf_app, [command, str(missing)])
        assert result.exit_code == 1
        assert result.exception is None or isinstance(result.exception, SystemExit)
        assert "FAIL -- " in result.output and missing.name in result.output
        assert "Traceback" not in result.output
