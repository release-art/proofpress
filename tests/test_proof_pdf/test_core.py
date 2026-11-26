"""The Python API over the Rust command: which layers run, what a missing tool means, and how files are chosen."""

import asyncio
import os
import pathlib

import pytest

from proof_pdf import core
from proof_pdf.models import FileReport, Finding, PageGeometry, Profile, Severity

from .conftest import A6

# Every Profile here leaves PDF/X out unless a test is about it: the blank test pages are not PDF/X-4 files.
PRINT = Profile(pdfx=False)


def _checks(report) -> set[str]:
    return {f.check for f in report.findings}


def _script(tmp_path: pathlib.Path, name: str, body: str) -> str:
    """An executable stand-in for an external tool."""
    path = tmp_path / name
    path.write_text(f"#!/bin/sh\n{body}\n")
    path.chmod(0o755)
    return str(path)


class TestVerifyFile:
    async def test_a_correct_file_passes(self, make_pdf):
        report = await core.verify_file(make_pdf(), PRINT)
        assert report.ok, report.findings

    async def test_the_pdfx_level_runs_by_default(self, make_pdf):
        report = await core.verify_file(make_pdf(), Profile())
        assert any(check.startswith("x4.") for check in _checks(report)) and not report.ok

    async def test_missing_verapdf_fails_rather_than_skips(self, make_pdf):
        report = await core.verify_file(make_pdf(), Profile(pdfx=False, pdfa=True), verapdf_bin="/nonexistent/vera")
        assert [(f.check, f.message) for f in report.errors] == [
            ("pdfa", "veraPDF not found (/nonexistent/vera) -- cannot confirm PDF/A-2b")
        ]

    async def test_missing_ghostscript_fails_rather_than_skips(self, make_pdf):
        report = await core.verify_file(make_pdf(), Profile(pdfx=False, tac=True), gs_bin="/nonexistent/gs")
        assert [(f.check, f.message) for f in report.errors] == [
            ("tac", "Ghostscript not found (/nonexistent/gs) -- cannot measure ink coverage")
        ]

    @pytest.mark.skipif(os.name != "posix", reason="the stand-in tool is a shell script")
    async def test_verapdf_rules_become_findings(self, make_pdf, tmp_path):
        report_json = (
            '{"report": {"jobs": [{"validationResult": [{"compliant": false, "details": {"ruleSummaries": '
            '[{"clause": "6.1.2", "testNumber": 1, "description": "header"}]}}]}]}}'
        )
        vera = _script(tmp_path, "vera", f"echo '{report_json}'\nexit 1")
        report = await core.verify_file(make_pdf(), Profile(pdfx=False, pdfa=True), verapdf_bin=vera)
        assert [(f.check, f.message) for f in report.errors] == [("pdfa", "PDF/A-2b 6.1.2-1: header")]

    @pytest.mark.skipif(os.name != "posix", reason="the stand-in tool is a shell script")
    async def test_a_tool_that_runs_too_long_is_a_finding(self, make_pdf, tmp_path):
        vera = _script(tmp_path, "vera", "sleep 5")
        report = await core.verify_file(make_pdf(), Profile(pdfx=False, pdfa=True), verapdf_bin=vera, timeout=0.1)
        assert [f.check for f in report.errors] == ["pdfa"]
        assert "timed out" in report.errors[0].message

    async def test_corrupt_pdf_is_a_finding_not_a_crash(self, tmp_path):
        broken = tmp_path / "broken.pdf"
        broken.write_bytes(b"not a pdf at all")
        report = await core.verify_file(broken, PRINT)
        assert not report.ok
        assert all(f.message.startswith("could not be checked") or f.check.startswith("file.") for f in report.errors)

    async def test_a_file_that_cannot_be_read_fails(self, tmp_path):
        report = await core.verify_file(tmp_path / "missing.pdf", PRINT)
        assert [(f.check, f.message.split(":")[0]) for f in report.errors] == [("structure", "cannot read the file")]

    async def test_password_protected_pdf_is_a_finding_not_a_crash(self, password_pdf):
        report = await core.verify_file(password_pdf, PRINT)
        assert not report.ok

    async def test_tools_run_only_when_asked(self, make_pdf):
        report = await core.verify_file(make_pdf(), PRINT, verapdf_bin="/nonexistent", gs_bin="/nonexistent")
        assert not {"pdfa", "tac"} & _checks(report)

    async def test_geometry_uses_the_profile_dimensions(self, make_pdf):
        path = make_pdf()
        assert "geometry" not in _checks(await core.verify_file(path, Profile(pdfx=False, geometry=A6)))
        wrong = Profile(pdfx=False, geometry=PageGeometry(trim_w_mm=210, trim_h_mm=297, bleed_mm=3, binding="right"))
        assert "geometry" in _checks(await core.verify_file(path, wrong))

    async def test_every_option_reaches_the_command(self, make_pdf):
        profile = Profile(
            pdfx=False,
            colour=False,
            missing_glyphs=False,
            fonts=False,
            strict_cmyk=True,
            allow_k_only=True,
            require_bleed=True,
            no_marks=True,
            uniform_pages=True,
            pages_multiple_of=2,
            output_condition="FOGRA51",
            allow_spots=(),
            warn_tac=250.0,
            warn_dpi=400.0,
            max_dpi=900.0,
            max_stretch_pct=1.0,
            min_text_pt=6.0,
            min_stroke_pt=0.25,
            safe_mm=3.0,
            min_delta_l=40.0,
            proof_profile=pathlib.Path("p.icc"),
        )
        report = await core.verify_file(make_pdf(), profile)
        assert {"page-count", "output-condition"} <= _checks(report)

    async def test_a_profile_the_command_refuses_raises(self, make_pdf):
        with pytest.raises(ValueError, match="must be below --max-tac"):
            await core.verify_file(make_pdf(), Profile(pdfx=False, warn_tac=400.0))


class TestConcurrency:
    async def test_verify_files_keeps_input_order_and_bounds_concurrency(self, tmp_path, mocker):
        running = peak = 0

        async def _verify(path, profile, **kwargs):
            nonlocal running, peak
            running += 1
            peak = max(peak, running)
            await asyncio.sleep(0.05 if path.name == "a.pdf" else 0.01)  # the first finishes last
            running -= 1
            return FileReport(path=path)

        mocker.patch.object(core, "verify_file", side_effect=_verify)
        paths = [tmp_path / f"{name}.pdf" for name in "abcde"]
        reports = await core.verify_files(paths, PRINT, jobs=2)
        assert [r.path for r in reports] == paths
        assert peak == 2


class TestKOnlyLayer:
    async def test_off_by_default(self, make_content_pdf):
        path = make_content_pdf(b"BT /F1 8 Tf 1 0 0 0 k (x) Tj ET")
        profile = Profile(pdfa=False, pdfx=False, colour=False)
        assert "k-only-text" not in _checks(await core.verify_file(path, profile))

    async def test_opt_in_fails_non_k_text(self, make_content_pdf):
        path = make_content_pdf(b"BT /F1 8 Tf 1 0 0 0 k (x) Tj ET")
        profile = Profile(pdfa=False, pdfx=False, colour=False, k_only_text=True)
        report = await core.verify_file(path, profile)
        assert "k-only-text" in _checks(report) and not report.ok


class TestExemption:
    @pytest.mark.parametrize(
        ("glob", "path", "matches"),
        [
            pytest.param("*_preview.pdf", "job/deep/Deck_preview.pdf", True, id="name-glob-at-any-depth"),
            pytest.param("*_preview.pdf", "Deck_preview.pdf", True, id="name-glob-at-the-top"),
            pytest.param("res/**", "job/res/logo.pdf", True, id="dir-glob"),
            pytest.param("res/logo.pdf", "job/res/logo.pdf", True, id="exact-tail"),
            pytest.param("res/logo.pdf", "job/other/logo.pdf", False, id="tail-must-match-whole"),
            pytest.param("*_preview.pdf", "job/Deck_full.pdf", False, id="no-match"),
            pytest.param("/abs/x.pdf", "/abs/x.pdf", True, id="absolute"),
        ],
    )
    def test_glob_matching(self, glob, path, matches):
        reason = core.exemption(pathlib.PurePath(path), {glob: "because"})
        assert (reason == "because") is matches

    def test_first_matching_glob_wins(self):
        assert core.exemption(pathlib.PurePath("a.pdf"), {"*.pdf": "first", "a.pdf": "second"}) == "first"

    def test_no_exemptions_means_none(self):
        assert core.exemption(pathlib.PurePath("a.pdf"), {}) is None


class TestVerifyFilesExemptions:
    async def test_exempt_files_are_reported_not_checked(self, tmp_path, mocker):

        async def _verify(path, *a, **k):
            return FileReport(path=path)

        verify = mocker.patch.object(core, "verify_file", side_effect=_verify)
        paths = [tmp_path / "a.pdf", tmp_path / "manual.pdf"]
        reports = await core.verify_files(paths, Profile(), exemptions={"manual.pdf": "internal docs"})
        assert [r.exempt for r in reports] == [None, "internal docs"]
        assert all(r.ok for r in reports)
        assert [call.args[0].name for call in verify.call_args_list] == ["a.pdf"]


class TestExpandTargets:
    def test_files_pass_through(self, tmp_path):
        a, b = tmp_path / "a.pdf", tmp_path / "b.pdf"
        for p in (a, b):
            p.write_bytes(b"x")
        assert core.expand_targets([a, b]) == [a, b]

    def test_directory_expands_recursively(self, tmp_path):
        (tmp_path / "nested").mkdir()
        top, deep = tmp_path / "top.pdf", tmp_path / "nested" / "deep.pdf"
        for p in (top, deep):
            p.write_bytes(b"x")
        (tmp_path / "notes.txt").write_text("ignored")
        assert core.expand_targets([tmp_path]) == [deep, top]

    def test_duplicates_are_dropped(self, tmp_path):
        pdf = tmp_path / "a.pdf"
        pdf.write_bytes(b"x")
        assert core.expand_targets([tmp_path, pdf]) == [pdf]

    def test_directory_honours_exclude_globs(self, tmp_path):
        (tmp_path / "res").mkdir()
        keep = tmp_path / "Deck_full.pdf"
        for p in (keep, tmp_path / "Deck_preview.pdf", tmp_path / "res" / "logo.pdf"):
            p.write_bytes(b"x")
        assert core.expand_targets([tmp_path], ["*_preview.pdf", "res/**"]) == [keep]

    def test_named_file_ignores_exclude(self, tmp_path):
        """Excluding is a directory-expansion rule, not a refusal to check the file."""
        preview = tmp_path / "Deck_preview.pdf"
        preview.write_bytes(b"x")
        assert core.expand_targets([preview], ["*_preview.pdf"]) == [preview]

    def test_exclude_is_relative_to_the_expanded_directory(self, tmp_path):
        root = tmp_path / "res"
        root.mkdir()
        pdf = root / "a.pdf"
        pdf.write_bytes(b"x")
        assert core.expand_targets([root], ["res/**"]) == [pdf]

    def test_empty_input(self):
        assert core.expand_targets([]) == []


class TestFileReport:
    def test_severity_partitions_and_ok(self, tmp_path):
        report = FileReport(path=tmp_path / "a.pdf")
        assert report.ok
        report.findings.append(Finding("x", Severity.WARN, "advisory"))
        assert report.ok and len(report.warnings) == 1
        report.findings.append(Finding("y", Severity.ERROR, "fatal"))
        assert not report.ok and len(report.errors) == 1

    @pytest.mark.parametrize(
        ("page", "expected"),
        [(None, "[x] hello"), (3, "[x] (page 3) hello")],
    )
    def test_finding_label(self, page, expected):
        assert Finding("x", Severity.ERROR, "hello", page).label() == expected


class TestMissingTools:
    def test_names_only_what_the_profile_needs_and_cannot_find(self, mocker):
        mocker.patch.object(core.shutil, "which", return_value=None)
        both = Profile(pdfa=True, tac=True)
        assert core.missing_tools(both, verapdf_bin="vera", gs_bin="ghost") == ["vera", "ghost"]
        assert core.missing_tools(Profile(tac=True)) == ["gs"]
        assert core.missing_tools(Profile(pdfa=True)) == ["verapdf"]
        assert core.missing_tools(Profile()) == []

    def test_a_tool_that_is_there_is_not_reported(self, mocker):
        mocker.patch.object(core.shutil, "which", return_value="/usr/bin/tool")
        assert core.missing_tools(Profile(pdfa=True, tac=True)) == []
