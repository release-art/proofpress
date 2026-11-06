"""The PDF/X rule tables: every row is well formed."""

import pikepdf
import pytest

from proof_pdf._internal import pdfx
from proof_pdf._internal.pdfx import x4
from proof_pdf.models import Severity

from .conftest import xmp_bytes


@pytest.mark.parametrize("rule", pdfx.RULES, ids=[r.id for r in pdfx.RULES])
def test_each_rule_is_well_formed(rule):
    assert rule.part and rule.clause and rule.summary
    assert rule.flavours == {"x4"}
    assert (rule.check is None) == (rule.status == "todo")
    assert rule.status in {"implemented", "partial", "todo"}


def test_rule_ids_are_unique():
    ids = [r.id for r in pdfx.RULES]
    assert len(ids) == len(set(ids))


def test_no_clause_is_claimed_without_a_source():
    """A clause is only written from the standard; until it is to hand every row says so."""
    assert {r.clause for r in pdfx.RULES} == {"unsourced"}


def _bare() -> pikepdf.Pdf:
    pdf = pikepdf.new()
    pdf.add_blank_page(page_size=(200, 200))
    return pdf


class TestX4IsTheOldBehaviour:
    def test_a_bare_page_gets_the_same_findings_as_before(self):
        findings = pdfx.check(_bare())
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
        assert pdfx.check(pdf) == []

    def test_the_rule_functions_are_the_moved_ones(self):
        by_id = {r.id: r.check for r in x4.RULES}
        assert by_id["page-boxes"] is x4.check_page_boxes
        assert by_id["trapped"] is x4.check_trapped
        assert by_id["identity"] is x4.check_pdfx_identity
