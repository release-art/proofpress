"""One PDF/X rule: where the standard states it, what it says, which flavours it binds, and the code that checks it."""

import collections.abc
import dataclasses

import pikepdf

from ...models import Finding, Severity

# part and clause of a rule nobody has yet matched to the standard's text. A clause is only ever filled in from the
# standard or a primary summary of it, never from memory, so this stays until the text is to hand.
UNSOURCED = "unsourced"
IMPLEMENTED, PARTIAL, TODO = "implemented", "partial", "todo"


@dataclasses.dataclass(frozen=True)
class Rule:
    """A PDF/X requirement. *part* and *clause* are the ISO 15930 part and clause it comes from.

    *status* is ``implemented``, ``partial`` (checks some of what the clause says) or ``todo`` (listed so the gap is
    visible; it has no *check*). *check* reads the open PDF and returns the findings for every page at once.
    """

    id: str
    summary: str
    flavours: frozenset[str]
    severity: Severity
    status: str
    check: collections.abc.Callable[[pikepdf.Pdf], list[Finding]] | None = None
    part: str = UNSOURCED
    clause: str = UNSOURCED
