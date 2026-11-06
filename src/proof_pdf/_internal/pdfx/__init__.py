"""The PDF/X-4 rules, one at a time, organised the way the standard is.

Every PDF/X rule is a row in :mod:`.x4`, so completing the standard means filling rows in. Colour *policy* (RGB against
CMYK, ink limits, profile consistency, spot allowlists) is not a PDF/X rule and stays in :mod:`..colour`.
"""

import pikepdf

from ...models import Finding
from . import x4
from .rule import Rule

__all__ = ["RULES", "Rule", "check"]

RULES = x4.RULES


def check(pdf: pikepdf.Pdf) -> list[Finding]:
    """Run every implemented or partial rule."""
    return [finding for rule in RULES if rule.check is not None for finding in rule.check(pdf)]
