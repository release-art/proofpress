"""The ``tools pdf`` command surface: exit codes, layers, and readable output."""

import importlib.metadata
import json
import pathlib

import pytest
import typer.testing

from proof_pdf import cli, core, preview
from proof_pdf.models import FileReport, Finding, PageGeometry, Severity


@pytest.fixture
def runner():
    return typer.testing.CliRunner()


@pytest.fixture
def stub_verify(mocker):
    """Replace the verification with canned findings, keyed by file name."""

    def _install(findings_by_name: dict[str, list[Finding]]):
        seen: list = []

        async def _verify(path, profile, **kwargs):
            seen.append((path, profile, kwargs))
            return FileReport(path=path, findings=list(findings_by_name.get(path.name, [])))

        mocker.patch.object(cli.core, "verify_file", side_effect=_verify)
        return seen

    return _install


def _pdf(tmp_path: pathlib.Path, name: str) -> pathlib.Path:
    path = tmp_path / name
    path.write_bytes(b"%PDF-1.7\n")
    return path


class TestVerifyCommand:
    def test_clean_file_exits_zero(self, runner, tmp_path, stub_verify):
        stub_verify({})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert result.exit_code == 0
        assert "OK" in result.output

    def test_error_exits_one_and_lists_the_finding(self, runner, tmp_path, stub_verify):
        stub_verify({"a.pdf": [Finding("geometry", Severity.ERROR, "wrong size", 1)]})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert result.exit_code == 1
        assert "wrong size" in result.output
        assert "FAIL" in result.output

    def test_warning_alone_still_passes(self, runner, tmp_path, stub_verify):
        stub_verify({"a.pdf": [Finding("overprint", Severity.WARN, "advisory", 1)]})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert result.exit_code == 0
        assert "advisory warning" in result.output

    def test_quiet_prints_nothing(self, runner, tmp_path, stub_verify):
        stub_verify({"a.pdf": [Finding("geometry", Severity.ERROR, "wrong size", 1)]})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "--quiet"])
        assert result.exit_code == 1
        assert result.output.strip() == ""

    def test_no_targets_is_an_error(self, runner):
        result = runner.invoke(cli.pdf_app, ["verify", "--quiet"])
        assert result.exit_code == 1

    def test_directory_argument_expands(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        for name in ("a.pdf", "b.pdf"):
            _pdf(tmp_path, name)
        result = runner.invoke(cli.pdf_app, ["verify", str(tmp_path)])
        assert result.exit_code == 0
        assert [p.name for p, _, _ in seen] == ["a.pdf", "b.pdf"]

    def test_json_output_is_machine_readable(self, runner, tmp_path, stub_verify):
        stub_verify({"a.pdf": [Finding("tac", Severity.ERROR, "too much ink", 2)]})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "--json"])
        assert result.exit_code == 1
        payload = json.loads(result.output)
        assert payload[0]["ok"] is False
        assert payload[0]["findings"][0] == {
            "check": "tac",
            "severity": "error",
            "message": "too much ink",
            "page": 2,
        }


class TestProfileWiring:
    def test_flags_reach_the_profile(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        runner.invoke(
            cli.pdf_app,
            [
                "verify",
                str(_pdf(tmp_path, "a.pdf")),
                "--no-pdfa",
                "--no-pdfx",
                "--no-colour",
                "--min-dpi",
                "450",
                "--strict-cmyk",
                "--max-tac",
                "280",
            ],
        )
        _, profile, _ = seen[0]
        assert not profile.pdfa and not profile.pdfx and not profile.colour
        assert profile.min_dpi == 450 and profile.strict_cmyk and profile.max_tac == 280

    def test_geometry_is_off_without_trim(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert seen[0][1].geometry is None

    def test_trim_and_bleed_options_build_the_geometry(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        runner.invoke(
            cli.pdf_app,
            ["verify", str(_pdf(tmp_path, "a.pdf")), "--trim-mm", "85x55", "--bleed-mm", "3"],
        )
        assert seen[0][1].geometry == PageGeometry(trim_w_mm=85.0, trim_h_mm=55.0, bleed_mm=3.0)

    def test_jobs_option_reaches_the_runner(self, runner, tmp_path, mocker):
        verify_files = mocker.patch.object(cli.core, "verify_files", return_value=[])
        runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "-j", "7"])
        assert verify_files.call_args.kwargs["jobs"] == 7

    @pytest.mark.parametrize("value", ["105", "105x148x2", "axb", "0x148", "-5x10"])
    def test_malformed_trim_is_rejected(self, runner, tmp_path, stub_verify, value):
        stub_verify({})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "--trim-mm", value])
        assert result.exit_code != 0

    def test_trim_accepts_an_asterisk_separator(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "--trim-mm", "85*55"])
        assert seen[0][1].geometry.trim_w_mm == 85.0


class TestExemptAndKOnly:
    def test_exempt_files_are_listed_with_their_reason_and_pass(self, runner, tmp_path, stub_verify):
        stub_verify({})
        result = runner.invoke(
            cli.pdf_app,
            [
                "verify",
                str(_pdf(tmp_path, "a.pdf")),
                str(_pdf(tmp_path, "manual.pdf")),
                "--exempt",
                "manual.pdf=internal docs, never printed",
            ],
        )
        assert result.exit_code == 0
        assert "EXEMPT" in result.output and "internal docs, never printed" in result.output
        assert "all 1 file(s) passed" in result.output and "1 exempt" in result.output

    def test_exempt_files_are_in_the_json(self, runner, tmp_path, stub_verify):
        stub_verify({})
        result = runner.invoke(
            cli.pdf_app,
            ["verify", str(_pdf(tmp_path, "m.pdf")), "--exempt", "m.pdf=why", "--json"],
        )
        record = json.loads(result.output)[0]
        assert record["exempt"] == "why" and record["ok"] is True

    @pytest.mark.parametrize("spec", ["manual.pdf", "=reason", "manual.pdf=", " = "])
    def test_exempt_needs_a_glob_and_a_reason(self, runner, tmp_path, stub_verify, spec):
        stub_verify({})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "--exempt", spec])
        assert result.exit_code != 0

    def test_k_only_text_flag_reaches_the_profile(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf")), "--k-only-text"])
        assert seen[0][1].k_only_text is True

    def test_k_only_text_is_off_by_default(self, runner, tmp_path, stub_verify):
        seen = stub_verify({})
        runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert seen[0][1].k_only_text is False


class TestCollapsedOutput:
    def test_identical_findings_across_pages_become_one_line(self, runner, tmp_path, stub_verify):
        """A 50-card deck must not print the same sentence fifty times."""
        findings = [Finding("geometry", Severity.ERROR, "wrong size", page) for page in range(1, 10)]
        stub_verify({"a.pdf": findings})
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert result.output.count("wrong size") == 1
        assert "pages 1-9" in result.output

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

    def test_different_messages_stay_separate(self, runner, tmp_path, stub_verify):
        stub_verify(
            {
                "a.pdf": [
                    Finding("geometry", Severity.ERROR, "wrong size", 1),
                    Finding("bleed", Severity.ERROR, "wrong bleed", 1),
                ]
            }
        )
        result = runner.invoke(cli.pdf_app, ["verify", str(_pdf(tmp_path, "a.pdf"))])
        assert "wrong size" in result.output and "wrong bleed" in result.output


class TestPreviewCommand:
    def test_writes_pages_and_reports_them(self, runner, tmp_path, mocker):
        mocker.patch.object(cli.preview, "gs_available", return_value=True)
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
        mocker.patch.object(cli.preview, "gs_available", return_value=True)
        seen: list[pathlib.Path] = []

        async def _preview_file(target, dst, **kwargs):
            seen.append(dst)
            return []

        mocker.patch.object(cli.preview, "preview_file", side_effect=_preview_file)
        runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf"))])
        assert seen == [tmp_path / "preview"]

    def test_missing_ghostscript_fails_fast(self, runner, tmp_path, mocker):
        mocker.patch.object(cli.preview, "gs_available", return_value=False)
        result = runner.invoke(cli.pdf_app, ["preview", str(_pdf(tmp_path, "a.pdf"))])
        assert result.exit_code == 1
        assert "Ghostscript not found" in result.output

    def test_render_failure_exits_one(self, runner, tmp_path, mocker):
        mocker.patch.object(cli.preview, "gs_available", return_value=True)

        async def _boom(*a, **k):
            raise preview.GhostscriptError("bad page")

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


def test_verapdf_error_is_reexported():
    assert cli.VeraPdfError is core.verapdf.VeraPdfError


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
