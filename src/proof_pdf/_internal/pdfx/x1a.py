"""PDF/X-1a: every rule is still ``todo``.

The rows name what the owner's brief says X-1a restricts, so the gap is visible; none has a clause or a check, because
the standard's text has not been to hand. Fill a row in (clause, summary, check, status) from the standard, never from
memory.
"""

from ...models import Severity
from .rule import TODO, Rule

X1A = frozenset({"x1a"})

RULES = (
    Rule("page-boxes", "TrimBox or ArtBox, nested inside the BleedBox and MediaBox", X1A, Severity.ERROR, TODO),
    Rule("trapped", "/Trapped in the Info dictionary is True or False", X1A, Severity.ERROR, TODO),
    Rule(
        "identity",
        "GTS_PDFXVersion / GTS_PDFXConformance in Info and XMP, and the output intent",
        X1A,
        Severity.ERROR,
        TODO,
    ),
    Rule("pdf-version", "the PDF version does not exceed the flavour's ceiling", X1A, Severity.ERROR, TODO),
    Rule("transparency", "no transparency", X1A, Severity.ERROR, TODO),
    Rule(
        "colour-spaces",
        "only CMYK, gray and spot colour spaces; RGB, Lab and ICC-based content are errors, bar the intent's profile",
        X1A,
        Severity.ERROR,
        TODO,
    ),
    Rule("fonts", "all fonts embedded", X1A, Severity.ERROR, TODO),
    Rule("encryption", "no encryption", X1A, Severity.ERROR, TODO),
)
