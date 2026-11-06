"""PDF/X-4: the checks proof-pdf has always made, as rules.

PDF/A has no opinion on page boxes, ``/Trapped`` or the PDF/X output intent, so dual PDF/X-4 + PDF/A files get zero
coverage on exactly the things a printer bounces a job for. These close that gap against a :class:`pikepdf.Pdf`. Their
check ids, messages and severities are the original ones; the clause each comes from is not yet sourced.
"""

import pikepdf

from ...models import Finding, Severity
from ..spaces import output_intents
from ..structure import _contains, media_box, own_box
from .rule import IMPLEMENTED, PARTIAL, TODO, Rule

X4 = frozenset({"x4"})


def check_page_boxes(pdf: pikepdf.Pdf) -> list[Finding]:
    """Verify each page carries the finished-page geometry PDF/X-4 demands."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        obj = page.obj
        media = media_box(obj)
        trim = own_box(obj, "TrimBox")
        art = own_box(obj, "ArtBox")
        bleed = own_box(obj, "BleedBox")

        if media is None:
            findings.append(Finding("boxes", Severity.ERROR, "no MediaBox", i))
            continue

        finished = trim or art
        if trim is None and art is None:
            findings.append(
                Finding(
                    "trimbox",
                    Severity.ERROR,
                    "no TrimBox or ArtBox -- PDF/X-4 needs one to mark the finished page",
                    i,
                )
            )
        elif trim is not None and art is not None:
            findings.append(
                Finding("trimbox", Severity.ERROR, "both TrimBox and ArtBox present -- PDF/X-4 allows only one", i)
            )

        if finished is not None and not _contains(media, finished):
            findings.append(Finding("trimbox", Severity.ERROR, "the trim/art box extends beyond the MediaBox", i))
        if bleed is not None:
            if not _contains(media, bleed):
                findings.append(Finding("bleedbox", Severity.ERROR, "the BleedBox extends beyond the MediaBox", i))
            if finished is not None and not _contains(bleed, finished):
                findings.append(
                    Finding("bleedbox", Severity.ERROR, "the BleedBox does not contain the trim/art box", i)
                )
        elif finished is not None:
            findings.append(
                Finding(
                    "bleedbox",
                    Severity.WARN,
                    "no BleedBox -- add bleed if the artwork runs to the trim edge",
                    i,
                )
            )
    return findings


def check_trapped(pdf: pikepdf.Pdf) -> list[Finding]:
    """PDF/X requires ``/Trapped`` in the Info dict to be True or False."""
    info = pdf.docinfo
    trapped = info.get("/Trapped") if info is not None else None
    if trapped is None:
        return [Finding("trapped", Severity.ERROR, "/Trapped missing from Info -- PDF/X requires True or False")]
    value = str(trapped)
    if value not in ("/True", "/False"):
        return [Finding("trapped", Severity.ERROR, f"/Trapped is {value.lstrip('/')} -- PDF/X requires True or False")]
    return []


def check_pdfx_identity(pdf: pikepdf.Pdf) -> list[Finding]:
    """Verify the PDF/X identity: the XMP version key and a GTS_PDFX output intent."""
    findings: list[Finding] = []
    if b"GTS_PDFXVersion" not in _xmp_bytes(pdf):
        findings.append(
            Finding("pdfx-xmp", Severity.ERROR, "XMP lacks pdfxid:GTS_PDFXVersion -- the PDF/X version identifier")
        )
    if not _has_pdfx_output_intent(pdf):
        findings.append(
            Finding(
                "pdfx-intent",
                Severity.ERROR,
                "no OutputIntent with subtype GTS_PDFX -- PDF/X-4 needs a print output intent",
            )
        )
    return findings


def _xmp_bytes(pdf: pikepdf.Pdf) -> bytes:
    metadata = pdf.Root.get("/Metadata")
    if metadata is None:
        return b""
    return bytes(metadata.read_bytes())


def _has_pdfx_output_intent(pdf: pikepdf.Pdf) -> bool:
    return any(str(intent.get("/S")) == "/GTS_PDFX" for intent in output_intents(pdf))


RULES = (
    Rule(
        "page-boxes",
        "each page has a MediaBox and exactly one of TrimBox or ArtBox, nested inside a BleedBox inside the MediaBox",
        X4,
        Severity.ERROR,
        PARTIAL,
        check_page_boxes,
    ),
    Rule("trapped", "/Trapped in the Info dictionary is True or False", X4, Severity.ERROR, IMPLEMENTED, check_trapped),
    Rule(
        "identity",
        "XMP carries pdfxid:GTS_PDFXVersion and an output intent of subtype GTS_PDFX is present",
        X4,
        Severity.ERROR,
        PARTIAL,
        check_pdfx_identity,
    ),
    Rule("info-version", "GTS_PDFXVersion / GTS_PDFXConformance in the Info dictionary", X4, Severity.ERROR, TODO),
    Rule("pdf-version", "the PDF version does not exceed the flavour's ceiling", X4, Severity.ERROR, TODO),
    Rule("the-rest", "everything else the standard requires has not been enumerated", X4, Severity.ERROR, TODO),
)
