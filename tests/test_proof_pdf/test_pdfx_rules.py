"""The PDF/X rule tables: every row is well formed, the coverage file is current, and a flavour with no rules fails."""

import pikepdf
import pytest

from proof_pdf import core
from proof_pdf._internal import pdfx
from proof_pdf._internal.pdfx import x4
from proof_pdf.models import PdfxVersion, Profile, Severity

from .conftest import xmp_bytes
from .test_cli import _pdf, runner, stub_verify  # noqa: F401  (fixtures)

ALL_RULES = [(flavour, rule) for flavour in pdfx.FLAVOURS for rule in pdfx.rules(flavour)]


def test_the_flavours_are_the_enum():
    assert pdfx.FLAVOURS == tuple(PdfxVersion) == ("x1a", "x3", "x4")


@pytest.mark.parametrize(("flavour", "rule"), ALL_RULES, ids=[f"{f}-{r.id}" for f, r in ALL_RULES])
def test_each_rule_is_well_formed(flavour, rule):
    assert rule.part and rule.clause and rule.summary
    assert rule.flavours == {flavour}
    assert (rule.check is None) == (rule.status == "todo")
    assert rule.status in {"implemented", "partial", "todo"}


def test_rule_ids_are_unique_within_a_flavour():
    for flavour in pdfx.FLAVOURS:
        ids = [r.id for r in pdfx.rules(flavour)]
        assert len(ids) == len(set(ids))


def test_no_clause_is_claimed_without_a_source():
    """A clause is only written from the standard; until it is to hand every row says so."""
    assert {r.clause for _, r in ALL_RULES} == {"unsourced"}


def test_coverage_md_is_current():
    assert pdfx.COVERAGE.read_text() == pdfx.coverage_markdown(), (
        "regenerate: pdm run python -m proof_pdf._internal.pdfx"
    )


def _bare() -> pikepdf.Pdf:
    pdf = pikepdf.new()
    pdf.add_blank_page(page_size=(200, 200))
    return pdf


class TestX4IsTheOldBehaviour:
    def test_a_bare_page_gets_the_same_findings_as_before(self):
        findings = pdfx.check(_bare(), "x4")
        assert [(f.check, f.severity) for f in findings] == [
            ("trimbox", Severity.ERROR),
            ("trapped", Severity.ERROR),
            ("pdfx-xmp", Severity.ERROR),
            ("pdfx-intent", Severity.ERROR),
        ]

    def test_a_complete_file_is_clean(self):
        pdf = _bare()
        pdf.pages[0].TrimBox = [0, 0, 200, 200]
        pdf.pages[0].BleedBox = [0, 0, 200, 200]
        pdf.docinfo["/Trapped"] = pikepdf.Name("/False")
        pdf.Root.Metadata = pdf.make_stream(xmp_bytes())
        pdf.Root.OutputIntents = pikepdf.Array([pikepdf.Dictionary(S=pikepdf.Name.GTS_PDFX)])
        assert pdfx.check(pdf, "x4") == []

    def test_the_rule_functions_are_the_moved_ones(self):
        by_id = {r.id: r.check for r in x4.RULES}
        assert by_id["page-boxes"] is x4.check_page_boxes
        assert by_id["trapped"] is x4.check_trapped
        assert by_id["identity"] is x4.check_pdfx_identity


@pytest.mark.parametrize("flavour", ["x1a", "x3"])
def test_a_flavour_without_rules_fails_rather_than_passes(flavour):
    [finding] = pdfx.check(_bare(), flavour)
    assert finding.check == "pdfx-coverage" and finding.severity is Severity.ERROR
    assert "cannot be confirmed" in finding.message and "COVERAGE.md" in finding.message


def test_the_flavour_reaches_the_structural_layer(tmp_path):
    path = tmp_path / "a.pdf"
    _bare().save(path)
    ids = lambda v: {f.check for f in core._check_pdf(path, Profile(pdfx_version=v))[0]}  # noqa: E731
    assert "pdfx-coverage" in ids(PdfxVersion.X1A) and "trimbox" not in ids(PdfxVersion.X1A)
    assert "pdfx-coverage" not in ids(PdfxVersion.X4) and "trimbox" in ids(PdfxVersion.X4)
    assert "pdfx-coverage" not in {
        f.check for f in core._check_pdf(path, Profile(pdfx=False, pdfx_version=PdfxVersion.X1A))[0]
    }


def test_the_cli_flag_reaches_the_profile(runner, tmp_path, stub_verify):  # noqa: F811
    seen = stub_verify({})
    runner.invoke(pdf_app(), ["verify", str(_pdf(tmp_path, "a.pdf")), "--pdfx-version", "x3"])
    assert seen[0][1].pdfx_version is PdfxVersion.X3
    runner.invoke(pdf_app(), ["verify", str(_pdf(tmp_path, "a.pdf"))])
    assert seen[1][1].pdfx_version is PdfxVersion.X4


def test_an_unknown_flavour_is_refused(runner, tmp_path, stub_verify):  # noqa: F811
    stub_verify({})
    assert runner.invoke(pdf_app(), ["verify", str(_pdf(tmp_path, "a.pdf")), "--pdfx-version", "x5"]).exit_code != 0


def pdf_app():
    from proof_pdf import cli

    return cli.pdf_app
