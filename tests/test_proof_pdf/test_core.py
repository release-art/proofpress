"""Orchestration: which layers run, and what a missing tool means."""

import pytest

from proof_pdf import colour, core, verapdf
from proof_pdf.models import FileReport, Finding, PageGeometry, Profile, Severity

from .conftest import A6


@pytest.fixture
def tools_present(monkeypatch, vera_json):
    """Both external tools resolve and report success."""
    monkeypatch.setattr(verapdf, "available", lambda b: True)
    monkeypatch.setattr(verapdf, "run", lambda *a, **k: vera_json(True))
    monkeypatch.setattr(colour, "gs_available", lambda b: True)
    monkeypatch.setattr(colour, "check_tac", lambda *a, **k: [])


def _checks(report) -> set[str]:
    return {f.check for f in report.findings}


class TestVerifyFile:
    def test_a_correct_file_passes_every_layer(self, make_pdf, tools_present, monkeypatch):
        path = make_pdf()
        # The synthetic PDF has no PDF/X identity, which is a separate concern.
        monkeypatch.setattr(core.structure, "check_pdfx_identity", lambda pdf: [])
        monkeypatch.setattr(core.structure, "check_trapped", lambda pdf: [])
        report = core.verify_file(path, Profile())
        assert report.ok, report.findings

    def test_missing_verapdf_fails_rather_than_skips(self, make_pdf, monkeypatch):
        """An unconfirmable file must not read as OK -- that is the whole point."""
        monkeypatch.setattr(verapdf, "available", lambda b: False)
        monkeypatch.setattr(colour, "gs_available", lambda b: True)
        monkeypatch.setattr(colour, "check_tac", lambda *a, **k: [])
        report = core.verify_file(make_pdf(), Profile())
        assert not report.ok
        assert any("veraPDF not found" in f.message for f in report.findings)

    def test_missing_ghostscript_fails_rather_than_skips(self, make_pdf, monkeypatch, vera_json):
        monkeypatch.setattr(verapdf, "available", lambda b: True)
        monkeypatch.setattr(verapdf, "run", lambda *a, **k: vera_json(True))
        monkeypatch.setattr(colour, "gs_available", lambda b: False)
        report = core.verify_file(make_pdf(), Profile())
        assert any("Ghostscript not found" in f.message for f in report.findings)

    def test_verapdf_tool_error_is_a_finding_not_a_crash(self, make_pdf, monkeypatch):
        monkeypatch.setattr(verapdf, "available", lambda b: True)
        monkeypatch.setattr(verapdf, "run", lambda *a, **k: (_ for _ in ()).throw(verapdf.VeraPdfError("boom")))
        monkeypatch.setattr(colour, "gs_available", lambda b: True)
        monkeypatch.setattr(colour, "check_tac", lambda *a, **k: [])
        report = core.verify_file(make_pdf(), Profile())
        assert any("boom" in f.message for f in report.findings)

    def test_ghostscript_error_is_a_finding_not_a_crash(self, make_pdf, monkeypatch, vera_json):
        monkeypatch.setattr(verapdf, "available", lambda b: True)
        monkeypatch.setattr(verapdf, "run", lambda *a, **k: vera_json(True))
        monkeypatch.setattr(colour, "gs_available", lambda b: True)
        monkeypatch.setattr(
            colour, "check_tac", lambda *a, **k: (_ for _ in ()).throw(colour.GhostscriptError("gs died"))
        )
        report = core.verify_file(make_pdf(), Profile())
        assert any("gs died" in f.message for f in report.findings)

    def test_corrupt_pdf_is_a_finding_not_a_crash(self, tmp_path, monkeypatch):
        monkeypatch.setattr(verapdf, "available", lambda b: True)
        monkeypatch.setattr(verapdf, "run", lambda *a, **k: '{"report": {"jobs": []}}')
        broken = tmp_path / "broken.pdf"
        broken.write_bytes(b"%PDF-1.7\nnot really a pdf")
        report = core.verify_file(broken, Profile())
        assert any("corrupt or encrypted" in f.message for f in report.findings)

    @pytest.mark.parametrize(
        ("profile", "absent"),
        [
            pytest.param(Profile(pdfa=False), "pdfa", id="no-pdfa"),
            pytest.param(Profile(pdfx=False), "trapped", id="no-pdfx"),
            pytest.param(Profile(geometry=None), "geometry", id="no-geometry"),
        ],
    )
    def test_layers_toggle_independently(self, make_pdf, monkeypatch, profile, absent):
        monkeypatch.setattr(verapdf, "available", lambda b: True)
        monkeypatch.setattr(verapdf, "run", lambda *a, **k: '{"report": {"jobs": []}}')
        monkeypatch.setattr(colour, "gs_available", lambda b: True)
        monkeypatch.setattr(colour, "check_tac", lambda *a, **k: [])
        boxes = {"MediaBox": [0, 0, 100, 100], "TrimBox": [0, 0, 100, 100]}
        report = core.verify_file(make_pdf(boxes=boxes), profile)
        assert absent not in _checks(report)

    def test_colour_layer_off_skips_ghostscript_entirely(self, make_pdf, monkeypatch, vera_json):
        monkeypatch.setattr(verapdf, "available", lambda b: True)
        monkeypatch.setattr(verapdf, "run", lambda *a, **k: vera_json(True))
        called = []
        monkeypatch.setattr(colour, "gs_available", lambda b: called.append(b) or True)
        report = core.verify_file(make_pdf(), Profile(colour=False))
        assert called == []
        assert "tac" not in _checks(report)

    def test_geometry_uses_the_profile_dimensions(self, make_pdf, tools_present):
        """The same file passes or fails depending on the stock it is held to."""
        path = make_pdf()
        profile = Profile(pdfa=False, pdfx=False, colour=False, geometry=A6)
        assert "geometry" not in _checks(core.verify_file(path, profile))
        wrong = Profile(
            pdfa=False, pdfx=False, colour=False, geometry=PageGeometry(trim_w_mm=85, trim_h_mm=55, bleed_mm=3)
        )
        assert "geometry" in _checks(core.verify_file(path, wrong))


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
