"""The veraPDF layer: shelling out, and parsing the JSON verdict."""

import pytest

from proof_pdf import proc, verapdf
from proof_pdf.models import Severity

RULE = {
    "clause": "6.2.11.4.2",
    "testNumber": 2,
    "description": "The CIDSet stream shall identify all glyphs.",
    "checks": [{"errorMessage": "CIDSet is incomplete"}],
}

DEVICE_GRAY = {
    "clause": "6.2.4.3",
    "testNumber": 4,
    "description": "DeviceGray shall only be used if a PDF/A OutputIntent is present.",
    "checks": [{"errorMessage": "DeviceGray colour space is used without output intent profile"}],
}


class TestRun:
    def test_available(self, mocker):
        mocker.patch.object(
            verapdf.shutil, "which", side_effect=lambda b: "/usr/bin/verapdf" if b == "verapdf" else None
        )
        assert verapdf.available("verapdf")
        assert not verapdf.available("missing")

    @pytest.mark.parametrize("returncode", [0, 1], ids=["valid", "invalid"])
    async def test_both_verdict_exits_return_stdout(self, fake_run, returncode):
        """veraPDF exits 1 for a non-conformant file; that is a verdict, not an error."""
        calls = fake_run(returncode=returncode, stdout='{"report": {}}')
        assert await verapdf.run("verapdf", "2b", "x.pdf") == '{"report": {}}'
        assert calls == [["verapdf", "--format", "json", "-f", "2b", "x.pdf"]]

    @pytest.mark.parametrize(
        ("stderr", "expected"), [("java exploded", "java exploded"), ("", "exited 2")], ids=["detail", "default"]
    )
    async def test_non_verdict_exit_raises(self, fake_run, stderr, expected):
        fake_run(returncode=2, stderr=stderr)
        with pytest.raises(verapdf.VeraPdfError, match=expected):
            await verapdf.run("verapdf", "2b", "x.pdf")

    async def test_timeout_raises(self, mocker):
        mocker.patch.object(proc, "run", side_effect=proc.ProcTimeoutError("timed out"))
        with pytest.raises(verapdf.VeraPdfError, match="timed out"):
            await verapdf.run("verapdf", "2b", "x.pdf", timeout=5)


class TestHints:
    def test_missing_output_intent_gets_a_fix(self, vera_json):
        message = verapdf.parse(vera_json(False, rule=DEVICE_GRAY), "2b")[0].message
        assert "6.2.4.3-4" in message and "hint: the file has no PDF/A output intent" in message

    def test_the_cmyk_wording_gets_the_hint_too(self, vera_json):
        """veraPDF says "without CMYK output intent profile" for DeviceCMYK, not "without output intent profile"."""
        rule = {
            **DEVICE_GRAY,
            "testNumber": 3,
            "checks": [{"errorMessage": "DeviceCMYK colour space is used without CMYK output intent profile"}],
        }
        assert (
            "hint: the file has no PDF/A output intent" in verapdf.parse(vera_json(False, rule=rule), "2b")[0].message
        )

    def test_a_different_6_2_4_3_failure_gets_no_hint(self, vera_json):
        rule = {**DEVICE_GRAY, "checks": [{"errorMessage": "DefaultGray has the wrong number of components"}]}
        assert "hint" not in verapdf.parse(vera_json(False, rule=rule), "2b")[0].message

    def test_other_clauses_get_no_hint(self, vera_json):
        rule = {**RULE, "checks": [{"errorMessage": "without output intent"}]}
        assert "hint" not in verapdf.parse(vera_json(False, rule=rule), "2b")[0].message


class TestParse:
    def test_compliant_is_clean(self, vera_json):
        assert verapdf.parse(vera_json(True), "2b") == []

    def test_failed_rule_carries_clause_and_detail(self, vera_json):
        findings = verapdf.parse(vera_json(False, rule=RULE), "2b")
        assert len(findings) == 1
        assert findings[0].severity is Severity.ERROR
        assert "PDF/A-2b 6.2.11.4.2-2" in findings[0].message
        assert "CIDSet is incomplete" in findings[0].message

    def test_failed_rule_without_detail(self, vera_json):
        rule = {"clause": "6.6.4", "testNumber": 1, "description": "Identification required."}
        findings = verapdf.parse(vera_json(False, rule=rule), "2b")
        assert "Identification required." in findings[0].message

    @pytest.mark.parametrize(
        ("kwargs", "expected"),
        [
            pytest.param({"jobs": False}, "no validation result", id="no-jobs"),
            pytest.param({"results": False}, "could not validate", id="no-results"),
            pytest.param({}, "no rule detail given", id="noncompliant-without-summaries"),
        ],
    )
    def test_degenerate_reports_still_fail(self, vera_json, kwargs, expected):
        findings = verapdf.parse(vera_json(False, **kwargs), "2b")
        assert findings and expected in findings[0].message

    def test_unreadable_json_raises_rather_than_passing(self, vera_json):
        """A parse failure must never read as a clean file."""
        with pytest.raises(verapdf.VeraPdfError, match="unreadable JSON"):
            verapdf.parse("not json at all", "2b")
