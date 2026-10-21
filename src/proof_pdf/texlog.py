"""Lint a LaTeX build log for the problems that never show up in the PDF.

The PDF only shows what was typeset. A character the font lacked is reported in the log as "Missing character" and then
handled differently by each engine (pdfTeX drops it; LuaTeX typesets the ``.notdef`` glyph, which ``proof-pdf verify``
also finds), an undefined reference prints as ``??``, and a substituted font is just a different font: none of that
fails a PDF check. So the log is where the full story is, and this reads it.

Works on the logs of pdfTeX, XeTeX and LuaTeX. Severity follows consequence: data silently lost or output known to be
wrong is an error (missing characters, TeX errors, undefined references and citations); a changed or imperfect result is
a warning (font substitution, overfull boxes, package warnings, a pending rerun).
"""

import collections
import pathlib
import re

from .models import Finding, Severity

# TeX breaks log lines at this width; a wrapped line carries on in the next one.
_LOG_WIDTH = 79

_MISSING = re.compile(
    r"^Missing character: There is no (?P<char>.+?)(?: \((?P<code>U\+[0-9A-Fa-f]+)\))? in font (?P<font>.+?)!?$"
)
_OVERFULL = re.compile(r"^Overfull \\hbox \((?P<pt>[\d.]+)pt too wide\)(?P<where>.*)$")
_UNDEFINED = re.compile(r"^(?:LaTeX|Package \w+) Warning: (?P<kind>Reference|Citation) `(?P<key>[^']*)' .*undefined")
_FONT_WARNING = re.compile(r"^LaTeX Font Warning: (?P<message>.+)$")
_PACKAGE_WARNING = re.compile(r"^(?P<who>(?:Package|Class) [\w-]+) Warning: (?P<message>.+)$")
_RERUN = re.compile(
    r"^(?:LaTeX Warning: Label\(s\) may have changed\. Rerun|Package rerunfilecheck Warning: File .* has changed)"
)
_ERROR = re.compile(r"^! (?P<message>.+)$")
_CONTINUATION = re.compile(r"^\((?:Font|[\w-]+)\)\s+(?P<text>.*)$")


def parse_log(text: str, *, max_overfull_pt: float | None = None) -> list[Finding]:
    """Findings for every problem in the log *text*, with repeats folded into one finding that counts them."""
    lines, numbers = _unwrap(text.splitlines())
    missing: collections.OrderedDict[tuple[str, str, str | None], list[int]] = collections.OrderedDict()
    findings: list[Finding] = []
    seen: dict[tuple[str, str], list[int]] = {}

    def note(check: str, severity: Severity, message: str, number: int) -> None:
        key = (check, message)
        if key in seen:
            seen[key].append(number)
        else:
            seen[key] = [number]
            findings.append(Finding(check, severity, message))

    for index, line in enumerate(lines):
        if m := _MISSING.match(line):
            missing.setdefault((m["char"], m["font"], m["code"]), []).append(numbers[index])
        elif (found := _classify(line, lines, index + 1, max_overfull_pt)) is not None:
            note(*found, numbers[index])

    folded = [_missing_finding(key, at) for key, at in missing.items()]
    return folded + [_counted(f, seen[(f.check, f.message)]) for f in findings]


def _classify(
    line: str, lines: list[str], number: int, max_overfull_pt: float | None
) -> tuple[str, Severity, str] | None:
    """The (check, severity, message) a log line stands for, or ``None`` for a line that is not a problem."""
    if m := _OVERFULL.match(line):
        pt = float(m["pt"])
        severity = Severity.ERROR if max_overfull_pt is not None and pt > max_overfull_pt else Severity.WARN
        return "overfull", severity, f"\\hbox {pt:g}pt too wide{m['where']}"
    if m := _UNDEFINED.match(line):
        return "undefined-reference", Severity.ERROR, f"{m['kind'].lower()} '{m['key']}' is undefined"
    if m := _FONT_WARNING.match(line):
        return "font", Severity.WARN, _with_continuation(m["message"], lines, number)
    if m := _PACKAGE_WARNING.match(line):
        return "package", Severity.WARN, f"{m['who']}: {_with_continuation(m['message'], lines, number)}"
    if _RERUN.match(line):
        return "rerun", Severity.WARN, "labels or files changed: run LaTeX again"
    if m := _ERROR.match(line):
        return "tex-error", Severity.ERROR, _error_message(m["message"], lines, number)
    return None


def check_log(path: pathlib.Path, *, max_overfull_pt: float | None = None) -> list[Finding]:
    """:func:`parse_log` for a file; undecodable bytes are replaced rather than failing the lint."""
    return parse_log(path.read_bytes().decode("utf-8", errors="replace"), max_overfull_pt=max_overfull_pt)


def _unwrap(lines: list[str]) -> tuple[list[str], list[int]]:
    """Rejoin lines TeX wrapped at 79 columns, with the physical line number each joined line started on."""
    out: list[str] = []
    numbers: list[int] = []
    carry, start = "", 0
    for number, line in enumerate(lines, start=1):
        if not carry:
            start = number
        line = carry + line
        carry = ""
        if len(line) == _LOG_WIDTH:
            carry = line
            continue
        out.append(line)
        numbers.append(start)
    if carry:
        out.append(carry)
        numbers.append(start)
    return out, numbers


def _with_continuation(message: str, lines: list[str], number: int) -> str:
    """*message* plus the ``(Font)``/``(package)`` continuation lines that follow it."""
    parts = [message.rstrip()]
    for following in lines[number : number + 4]:
        m = _CONTINUATION.match(following)
        if not m:
            break
        parts.append(m["text"].strip())
    return " ".join(p for p in parts if p).replace("  ", " ")


def _error_message(message: str, lines: list[str], number: int) -> str:
    """A TeX error with the source line (``l.12 ...``) that follows it."""
    for following in lines[number : number + 6]:
        if following.startswith("l."):
            return f"{message} ({following.strip()})"
    return message


def _missing_finding(key: tuple[str, str, str | None], at: list[int]) -> Finding:
    char, font, code = key
    shown = f"'{char}' ({code})" if code else f"'{char}'"
    times = f"reported {len(at)} times, first at log line {at[0]}" if len(at) > 1 else f"log line {at[0]}"
    return Finding(
        "missing-character",
        Severity.ERROR,
        f"{shown} was dropped: font {font} has no glyph for it ({times})",
    )


def _counted(finding: Finding, at: list[int]) -> Finding:
    if len(at) == 1:
        return Finding(finding.check, finding.severity, f"{finding.message} (log line {at[0]})")
    return Finding(finding.check, finding.severity, f"{finding.message} ({len(at)} times, first at log line {at[0]})")
