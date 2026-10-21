"""Orchestration: which layers run, and what a missing tool means."""

import asyncio
import pathlib

import pytest

from proof_pdf import core
from proof_pdf._internal import colour, verapdf
from proof_pdf.models import FileReport, Finding, PageGeometry, Profile, Severity

from .conftest import A6


@pytest.fixture
def tools_present(mocker, vera_json):
    """Both external tools resolve and report success."""
    mocker.patch.object(verapdf, "available", return_value=True)
    mocker.patch.object(verapdf, "run", return_value=vera_json(True))
    mocker.patch.object(colour, "gs_available", return_value=True)
    mocker.patch.object(colour, "check_tac", return_value=[])


def _checks(report) -> set[str]:
    return {f.check for f in report.findings}


class TestVerifyFile:
    async def test_a_correct_file_passes_every_layer(self, make_pdf, tools_present, mocker):
        path = make_pdf()
        # The synthetic PDF has no PDF/X identity, which is a separate concern.
        mocker.patch.object(core.structure, "check_pdfx_identity", return_value=[])
        mocker.patch.object(core.structure, "check_trapped", return_value=[])
        report = await core.verify_file(path, Profile())
        assert report.ok, report.findings

    async def test_missing_verapdf_fails_rather_than_skips(self, make_pdf, mocker):
        """An unconfirmable file must not read as OK -- that is the whole point."""
        mocker.patch.object(verapdf, "available", return_value=False)
        mocker.patch.object(colour, "gs_available", return_value=True)
        mocker.patch.object(colour, "check_tac", return_value=[])
        report = await core.verify_file(make_pdf(), Profile())
        assert not report.ok
        assert any("veraPDF not found" in f.message for f in report.findings)

    async def test_missing_ghostscript_fails_rather_than_skips(self, make_pdf, mocker, vera_json):
        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(verapdf, "run", return_value=vera_json(True))
        mocker.patch.object(colour, "gs_available", return_value=False)
        report = await core.verify_file(make_pdf(), Profile())
        assert any("Ghostscript not found" in f.message for f in report.findings)

    async def test_verapdf_tool_error_is_a_finding_not_a_crash(self, make_pdf, mocker):
        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(verapdf, "run", side_effect=verapdf.VeraPdfError("boom"))
        mocker.patch.object(colour, "gs_available", return_value=True)
        mocker.patch.object(colour, "check_tac", return_value=[])
        report = await core.verify_file(make_pdf(), Profile())
        assert any("boom" in f.message for f in report.findings)

    async def test_ghostscript_error_is_a_finding_not_a_crash(self, make_pdf, mocker, vera_json):
        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(verapdf, "run", return_value=vera_json(True))
        mocker.patch.object(colour, "gs_available", return_value=True)
        mocker.patch.object(colour, "check_tac", side_effect=colour.GhostscriptError("gs died"))
        report = await core.verify_file(make_pdf(), Profile())
        assert any("gs died" in f.message for f in report.findings)

    async def test_corrupt_pdf_is_a_finding_not_a_crash(self, tmp_path, mocker):
        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(verapdf, "run", return_value='{"report": {"jobs": []}}')
        broken = tmp_path / "broken.pdf"
        broken.write_bytes(b"%PDF-1.7\nnot really a pdf")
        report = await core.verify_file(broken, Profile())
        assert any("corrupt or encrypted" in f.message for f in report.findings)

    @pytest.mark.parametrize(
        ("profile", "absent"),
        [
            pytest.param(Profile(pdfa=False), "pdfa", id="no-pdfa"),
            pytest.param(Profile(pdfx=False), "trapped", id="no-pdfx"),
            pytest.param(Profile(geometry=None), "geometry", id="no-geometry"),
        ],
    )
    async def test_layers_toggle_independently(self, make_pdf, mocker, profile, absent):
        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(verapdf, "run", return_value='{"report": {"jobs": []}}')
        mocker.patch.object(colour, "gs_available", return_value=True)
        mocker.patch.object(colour, "check_tac", return_value=[])
        boxes = {"MediaBox": [0, 0, 100, 100], "TrimBox": [0, 0, 100, 100]}
        report = await core.verify_file(make_pdf(boxes=boxes), profile)
        assert absent not in _checks(report)

    async def test_colour_layer_off_skips_ghostscript_entirely(self, make_pdf, mocker, vera_json):
        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(verapdf, "run", return_value=vera_json(True))
        gs = mocker.patch.object(colour, "gs_available", return_value=True)
        report = await core.verify_file(make_pdf(), Profile(colour=False))
        gs.assert_not_called()
        assert "tac" not in _checks(report)

    async def test_geometry_uses_the_profile_dimensions(self, make_pdf, tools_present):
        """The same file passes or fails depending on the stock it is held to."""
        path = make_pdf()
        profile = Profile(pdfa=False, pdfx=False, colour=False, geometry=A6)
        assert "geometry" not in _checks(await core.verify_file(path, profile))
        wrong = Profile(
            pdfa=False, pdfx=False, colour=False, geometry=PageGeometry(trim_w_mm=85, trim_h_mm=55, bleed_mm=3)
        )
        assert "geometry" in _checks(await core.verify_file(path, wrong))


class TestConcurrency:
    async def test_the_external_tools_overlap_within_one_file(self, make_pdf, mocker):
        """veraPDF and the Ghostscript scan each wait for the other to start: sequential execution would hang."""
        vera_started, tac_started = asyncio.Event(), asyncio.Event()

        async def _vera(*a, **k):
            vera_started.set()
            await tac_started.wait()
            return '{"report": {"jobs": []}}'

        async def _tac(*a, **k):
            tac_started.set()
            await vera_started.wait()
            return []

        mocker.patch.object(verapdf, "available", return_value=True)
        mocker.patch.object(colour, "gs_available", return_value=True)
        mocker.patch.object(verapdf, "run", side_effect=_vera)
        mocker.patch.object(colour, "check_tac", side_effect=_tac)
        await asyncio.wait_for(core.verify_file(make_pdf(), Profile()), timeout=5)

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
        reports = await core.verify_files(paths, Profile(), jobs=2)
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
