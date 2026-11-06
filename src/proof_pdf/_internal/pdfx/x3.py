"""PDF/X-3: every rule is still ``todo``; see :mod:`.x1a` for why."""

from ...models import Severity
from .rule import TODO, Rule

X3 = frozenset({"x3"})

RULES = (
    Rule("page-boxes", "TrimBox or ArtBox, nested inside the BleedBox and MediaBox", X3, Severity.ERROR, TODO),
    Rule("trapped", "/Trapped in the Info dictionary is True or False", X3, Severity.ERROR, TODO),
    Rule(
        "identity",
        "GTS_PDFXVersion / GTS_PDFXConformance in Info and XMP, and the output intent",
        X3,
        Severity.ERROR,
        TODO,
    ),
    Rule("pdf-version", "the PDF version does not exceed the flavour's ceiling", X3, Severity.ERROR, TODO),
    Rule("transparency", "no transparency", X3, Severity.ERROR, TODO),
    Rule("colour-spaces", "ICC-based and Lab colour allowed alongside CMYK, gray and spot", X3, Severity.ERROR, TODO),
    Rule("fonts", "all fonts embedded", X3, Severity.ERROR, TODO),
    Rule("encryption", "no encryption", X3, Severity.ERROR, TODO),
)
