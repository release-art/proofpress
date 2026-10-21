"""The veraPDF layer: run the validator and turn its JSON into findings.

veraPDF owns the PDF/A side (it has no PDF/X profile). It is an external Java
tool, so it is isolated behind two small functions -- :func:`run` shells out (awaited),
:func:`parse` is pure. Its absence is surfaced by the caller as a failure, not a
silent pass: the point of the tool is to *confirm* conformance.
"""

import json
import logging
import shutil

from . import proc
from .models import Finding, Severity

logger = logging.getLogger(__name__)

# veraPDF exits 0 for a valid file and 1 for an invalid one; both still emit a
# JSON report. Anything else is a tool/parse failure we must not mistake for a
# validation verdict.
_VALIDATION_EXITS = (0, 1)


class VeraPdfError(RuntimeError):
    """veraPDF could not be run, or exited in a way that is not a verdict."""


def available(binary: str) -> bool:
    """Whether *binary* resolves on PATH (or as an explicit path)."""
    return shutil.which(binary) is not None


async def run(binary: str, flavour: str, path: str, *, timeout: float | None = None) -> str:
    """Validate *path* against *flavour* and return veraPDF's JSON stdout."""
    logger.debug(f"verapdf: validating {path} against PDF/A-{flavour}")
    try:
        result = await proc.run([binary, "--format", "json", "-f", flavour, path], timeout=timeout)
    except proc.ProcTimeoutError as exc:
        raise VeraPdfError(f"veraPDF timed out after {timeout:.0f}s") from exc
    if result.returncode not in _VALIDATION_EXITS:
        message = result.stderr.strip() or f"veraPDF exited {result.returncode}"
        logger.error(f"verapdf failed (rc={result.returncode}): {message}")
        raise VeraPdfError(message)
    return result.stdout


def parse(stdout: str, flavour: str) -> list[Finding]:
    """Turn veraPDF's JSON report into an error :class:`Finding` per failed rule."""
    try:
        data = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise VeraPdfError(f"veraPDF emitted unreadable JSON: {exc}") from exc
    jobs = data.get("report", {}).get("jobs", [])
    if not jobs:
        return [Finding("pdfa", Severity.ERROR, "veraPDF returned no validation result")]

    findings: list[Finding] = []
    for job in jobs:
        results = job.get("validationResult") or []
        if not results:
            findings.append(Finding("pdfa", Severity.ERROR, "veraPDF could not validate the file"))
            continue
        for vr in results:
            if vr.get("compliant"):
                continue
            summaries = vr.get("details", {}).get("ruleSummaries", [])
            if not summaries:  # non-compliant but no per-rule detail -- still a failure
                findings.append(
                    Finding(
                        "pdfa", Severity.ERROR, f"veraPDF reports PDF/A-{flavour} non-compliant, no rule detail given"
                    )
                )
                continue
            findings += [_rule_finding(flavour, rule) for rule in summaries]
    return findings


def _rule_finding(flavour: str, rule: dict) -> Finding:
    """One failed veraPDF rule, described with its clause and an example message."""
    clause = rule.get("clause", "?")
    test = rule.get("testNumber", "?")
    description = (rule.get("description") or "").strip()
    detail = _first_error_message(rule)
    suffix = f" -- {detail}" if detail else ""
    message = f"PDF/A-{flavour} {clause}-{test}: {description}{suffix}{_hint(clause, detail)}"
    return Finding("pdfa", Severity.ERROR, message)


# 6.2.4.3 is the "device colour space needs an output intent" family (DeviceGray, DeviceRGB, DeviceCMYK). With no
# output intent a single stray operator fails the file, and the message does not say what to add.
_OUTPUT_INTENT_HINT = (
    " (hint: the file has no PDF/A output intent for this colour space; declare one with an ICC profile, "
    "or avoid device colour operators)"
)


def _hint(clause: str, detail: str) -> str:
    """A one-line fix for the failures whose veraPDF message names the symptom but not the cause."""
    lowered = detail.lower()
    # "...used without output intent profile" (Gray, RGB) / "...without CMYK output intent profile"
    if clause == "6.2.4.3" and "without" in lowered and "output intent" in lowered:
        return _OUTPUT_INTENT_HINT
    return ""


def _first_error_message(rule: dict) -> str:
    """The first concrete per-check error message, if veraPDF gave one."""
    for check in rule.get("checks", []):
        message = (check.get("errorMessage") or "").strip()
        if message:
            return message
    return ""
