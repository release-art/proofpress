"""The LaTeX log linter, on log lines in the shapes pdfTeX, XeTeX and LuaTeX write them."""

import pytest

from proof_pdf import texlog
from proof_pdf.models import Severity


def _by_check(findings) -> dict[str, list]:
    out: dict[str, list] = {}
    for f in findings:
        out.setdefault(f.check, []).append(f)
    return out


class TestMissingCharacters:
    def test_luatex_wording_with_a_code_point(self):
        log = "Missing character: There is no ö (U+00F6) in font Nunito:script=latn;+tlig;!\n"
        [finding] = texlog.parse_log(log)
        assert finding.check == "missing-character" and finding.severity is Severity.ERROR
        assert "'ö' (U+00F6)" in finding.message and "Nunito" in finding.message and "log line 1" in finding.message

    def test_pdftex_wording(self):
        [finding] = texlog.parse_log("Missing character: There is no ^^A in font cmr10!\n")
        assert "'^^A'" in finding.message and "cmr10" in finding.message

    def test_repeats_fold_into_one_finding_that_counts_them(self):
        log = "x\n" + "Missing character: There is no ♥ (U+2665) in font Nunito!\n" * 3
        [finding] = texlog.parse_log(log)
        assert "reported 3 times, first at log line 2" in finding.message

    def test_line_numbers_are_physical_even_after_unwrapping(self):
        wrapped = "Missing character: There is no ö (U+00F6) in font Nunito-Regular:mode=base;script=latn;lang"[:79]
        log = "a\nb\n" + wrapped + "\nrest!\nLaTeX Warning: Reference `x' on page 1 undefined on input line 9.\n"
        by = _by_check(texlog.parse_log(log))
        assert "log line 3" in by["missing-character"][0].message
        assert "log line 5" in by["undefined-reference"][0].message  # the wrap above took two physical lines

    def test_different_characters_or_fonts_stay_separate(self):
        log = (
            "Missing character: There is no ♥ (U+2665) in font Nunito!\n"
            "Missing character: There is no ♥ (U+2665) in font Other!\n"
            "Missing character: There is no ★ (U+2605) in font Nunito!\n"
        )
        assert len(texlog.parse_log(log)) == 3

    def test_a_line_wrapped_at_79_columns_is_rejoined(self):
        full = "Missing character: There is no ö (U+00F6) in font Nunito-Regular:mode=base;script=latn;lang"
        line = full[:79]
        rest = full[79:] + "!"
        assert len(line) == 79
        [finding] = texlog.parse_log(line + "\n" + rest + "\n")
        assert "mode=base" in finding.message and "lang" in finding.message


class TestOtherProblems:
    def test_undefined_reference_and_citation_are_errors(self):
        log = (
            "LaTeX Warning: Reference `fig:x' on page 2 undefined on input line 55.\n"
            "LaTeX Warning: Citation `smith' on page 3 undefined on input line 60.\n"
        )
        findings = texlog.parse_log(log)
        assert {f.severity for f in findings} == {Severity.ERROR}
        assert {f.check for f in findings} == {"undefined-reference"}
        assert any("reference 'fig:x'" in f.message for f in findings)
        assert any("citation 'smith'" in f.message for f in findings)

    def test_a_tex_error_carries_its_source_line(self):
        log = "! Undefined control sequence.\nl.12 \\foo\n                 bar\n"
        [finding] = texlog.parse_log(log)
        assert finding.check == "tex-error" and finding.severity is Severity.ERROR
        assert "Undefined control sequence." in finding.message and "l.12 \\foo" in finding.message

    def test_font_substitution_is_a_warning_with_its_continuation(self):
        log = (
            "LaTeX Font Warning: Font shape `TU/Nunito(0)/m/sl' undefined\n"
            "(Font)              using `TU/Nunito(0)/m/n' instead on input line 12.\n"
        )
        [finding] = texlog.parse_log(log)
        assert finding.check == "font" and finding.severity is Severity.WARN
        assert "undefined" in finding.message and "using `TU/Nunito(0)/m/n' instead" in finding.message

    def test_package_warnings_are_warnings(self):
        log = (
            "Package hyperref Warning: Token not allowed in a PDF string (Unicode):\n"
            "(hyperref)                removing `math shift'.\n"
        )
        [finding] = texlog.parse_log(log)
        assert finding.check == "package" and "Package hyperref" in finding.message and "removing" in finding.message

    def test_a_pending_rerun_is_a_warning(self):
        [finding] = texlog.parse_log("LaTeX Warning: Label(s) may have changed. Rerun to get cross-references right.\n")
        assert finding.check == "rerun" and finding.severity is Severity.WARN

    def test_overfull_is_a_warning_unless_the_caller_sets_a_limit(self):
        log = "Overfull \\hbox (12.5pt too wide) in paragraph at lines 42--43\n"
        [plain] = texlog.parse_log(log)
        [limited] = texlog.parse_log(log, max_overfull_pt=10)
        [within] = texlog.parse_log(log, max_overfull_pt=20)
        assert plain.severity is Severity.WARN and within.severity is Severity.WARN
        assert limited.severity is Severity.ERROR and "12.5pt too wide" in limited.message

    def test_a_clean_log_has_no_findings(self):
        log = "This is LuaHBTeX, Version 1.18.0\n(./book.tex\nLaTeX2e <2025-06-01>\nOutput written on book.pdf.\n"
        assert texlog.parse_log(log) == []

    @pytest.mark.parametrize("underfull", ["Underfull \\hbox (badness 10000) in paragraph at lines 1--2"])
    def test_underfull_is_not_reported(self, underfull):
        assert texlog.parse_log(underfull + "\n") == []

    def test_a_realistic_mixed_log(self):
        log = "\n".join(
            [
                "(./card.tex",
                "Missing character: There is no ö (U+00F6) in font Nunito!",
                "Overfull \\hbox (3.0pt too wide) in paragraph at lines 5--6",
                "LaTeX Warning: Reference `x' on page 1 undefined on input line 9.",
                "Missing character: There is no ö (U+00F6) in font Nunito!",
            ]
        )
        by = _by_check(texlog.parse_log(log))
        assert set(by) == {"missing-character", "overfull", "undefined-reference"}
        assert "2 times" in by["missing-character"][0].message


def test_check_log_reads_a_file_and_survives_bad_bytes(tmp_path):
    path = tmp_path / "book.log"
    path.write_bytes(b"Missing character: There is no \xff in font y!\n")
    assert [f.check for f in texlog.check_log(path)] == ["missing-character"]
