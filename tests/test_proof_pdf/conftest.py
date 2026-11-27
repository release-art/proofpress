"""Shared PDF builders for the pdfverify tests.

Every check reads a real :class:`pikepdf.Pdf`, so the fixtures here build real
(if tiny) PDFs rather than mocking pikepdf: a fake object graph would test the
mock's idea of the format instead of the format. The builders are factories --
each test asks for the one shape it needs -- so a new case adds a call, not a
new block of setup.
"""

import json
import pathlib
import tempfile
import zlib
from typing import Any, cast

import pikepdf
import pytest

from proof_pdf._internal import _core
from proof_pdf.models import Finding, PageGeometry, Severity

# An A6 card (105x148 mm) centred inside a 3 mm bleed: the "correct" baseline.
A6 = PageGeometry(trim_w_mm=105.0, trim_h_mm=148.0, bleed_mm=3.0)
MM = 72.0 / 25.4
TRIM_W, TRIM_H = 105.0 * MM, 148.0 * MM
BLEED = 3.0 * MM
MEDIA_W, MEDIA_H = TRIM_W + 2 * BLEED, TRIM_H + 2 * BLEED
GOOD_BOXES = {
    "MediaBox": [0, 0, MEDIA_W, MEDIA_H],
    "TrimBox": [BLEED, BLEED, BLEED + TRIM_W, BLEED + TRIM_H],
    "BleedBox": [0, 0, MEDIA_W, MEDIA_H],
}


def core_run(path: pathlib.Path, params: dict[str, object] | None = None) -> list[Finding]:
    """Every print check's findings on the PDF at *path*, from the Rust core, with the print checks' *params* (the
    core's defaults for any left out); a check the core could not judge fails as "could not be checked"."""
    report = json.loads(_core.verify(str(path), path.read_bytes(), "print", json.dumps(params or {})))
    severity = {"error": Severity.ERROR, "warning": Severity.WARN}
    findings = [Finding(f["check"], severity[f["severity"]], f["message"], f["page"]) for f in report["findings"]]
    findings += [
        Finding(c["id"], Severity.ERROR, f"could not be checked: {c['reason']}")
        for c in report["checks"]
        if c["status"] == "not_evaluated"
    ]
    return findings


def core_findings(pdf: pikepdf.Pdf, checks: set[str], **params: object) -> list[Finding]:
    """The Rust core's findings of *checks* on *pdf*, with the print checks' *params*."""
    with tempfile.TemporaryDirectory() as tmp:
        path = pathlib.Path(tmp) / "checked.pdf"
        pdf.save(path)
        return [f for f in core_run(path, params) if f.check in checks]


@pytest.fixture
def make_pdf(tmp_path):
    """Build a PDF with *pages* blank pages and the given boxes."""

    def _make(name: str = "test.pdf", *, pages: int = 1, boxes: dict | None = None) -> pathlib.Path:
        path = tmp_path / name
        pdf = pikepdf.new()
        for _ in range(pages):
            page = pikepdf.Dictionary(Type=pikepdf.Name.Page, Contents=pdf.make_stream(b""))
            for key, value in (boxes if boxes is not None else GOOD_BOXES).items():
                page[f"/{key}"] = value
            pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        pdf.save(path)
        return path

    return _make


@pytest.fixture
def password_pdf(tmp_path) -> pathlib.Path:
    """A one-page PDF that opens only with a password (pikepdf raises PasswordError, not PdfError)."""
    path = tmp_path / "locked.pdf"
    pdf = pikepdf.new()
    pdf.add_blank_page()
    pdf.save(path, encryption=pikepdf.Encryption(user="secret", owner="secret"))
    return path


def xmp_bytes(*, pdfx: bool = True, trapped: str | None = "False") -> bytes:
    """A well-formed XMP packet, optionally carrying the PDF/X version key and ``pdf:Trapped``.

    It has to be genuinely well-formed: pikepdf parses /Metadata on save and
    silently replaces anything it cannot read with an empty packet, so a
    hand-waved XML blob would be gone by the time the check reads it back.
    """
    version = "<pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion>" if pdfx else ""
    return (
        '<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>'
        '<x:xmpmeta xmlns:x="adobe:ns:meta/">'
        '<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
        '<rdf:Description rdf:about="" xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/"'
        ' xmlns:pdf="http://ns.adobe.com/pdf/1.3/">'
        f"{version}"
        f"{'' if trapped is None else f'<pdf:Trapped>{trapped}</pdf:Trapped>'}"
        "</rdf:Description></rdf:RDF></x:xmpmeta>"
        '<?xpacket end="w"?>'
    ).encode()


@pytest.fixture
def make_content_pdf(tmp_path):
    """Build a one-page PDF whose content stream is *stream*.

    *xobjects* maps a resource name to a raw dictionary, so a test can place a
    form or an image without hand-assembling the whole resource tree.
    """

    def _make(
        stream: bytes,
        *,
        name: str = "content.pdf",
        font: bool = True,
        xobjects: dict[str, pikepdf.Object] | None = None,
        extgstates: dict[str, pikepdf.Object] | None = None,
        colorspaces: dict[str, pikepdf.Object] | None = None,
    ) -> pathlib.Path:
        path = tmp_path / name
        pdf = pikepdf.new()
        resources = pikepdf.Dictionary()
        if font:
            resources["/Font"] = pikepdf.Dictionary(
                F1=pdf.make_indirect(
                    pikepdf.Dictionary(
                        Type=pikepdf.Name.Font,
                        Subtype=pikepdf.Name.Type1,
                        BaseFont=pikepdf.Name("/Helvetica"),
                    )
                )
            )
        for key, table in (("/XObject", xobjects), ("/ExtGState", extgstates), ("/ColorSpace", colorspaces)):
            if table:
                resources[key] = pikepdf.Dictionary(
                    **cast("dict[str, Any]", {k.lstrip("/"): v for k, v in table.items()})
                )
        page = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            Contents=pdf.make_stream(stream),
            Resources=resources,
            **GOOD_BOXES,
        )
        pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        pdf.save(path)
        return path

    return _make


@pytest.fixture
def make_form():
    """A Form XObject wrapping *stream*, for the nesting tests."""

    def _make(
        pdf: pikepdf.Pdf,
        stream: bytes,
        *,
        matrix: list | None = None,
        resources: pikepdf.Object | None = None,
    ) -> pikepdf.Object:
        form = pdf.make_stream(stream)
        form["/Type"] = pikepdf.Name.XObject
        form["/Subtype"] = pikepdf.Name.Form
        form["/BBox"] = [0, 0, 1000, 1000]
        if matrix is not None:
            form["/Matrix"] = matrix
        if resources is not None:
            form["/Resources"] = resources
        return form

    return _make


@pytest.fixture
def make_image():
    """A minimal image XObject of *width* x *height* pixels in *colorspace*."""

    def _make(
        pdf: pikepdf.Pdf,
        *,
        width: int = 100,
        height: int = 100,
        colorspace: pikepdf.Object | None = None,
    ) -> pikepdf.Object:
        components = 1
        space = colorspace if colorspace is not None else pikepdf.Name.DeviceGray
        image = pdf.make_stream(zlib.compress(b"\x00" * (width * height * components)))
        image["/Type"] = pikepdf.Name.XObject
        image["/Subtype"] = pikepdf.Name.Image
        image["/Width"] = width
        image["/Height"] = height
        image["/BitsPerComponent"] = 8
        image["/ColorSpace"] = space
        image["/Filter"] = pikepdf.Name.FlateDecode
        return image

    return _make


@pytest.fixture
def vera_json():
    """A veraPDF JSON report, compliant or with one failed rule."""

    def _make(compliant: bool, *, rule: dict | None = None, jobs: bool = True, results: bool = True) -> str:
        import json

        if not jobs:
            return json.dumps({"report": {"jobs": []}})
        if not results:
            return json.dumps({"report": {"jobs": [{"validationResult": []}]}})
        details = {"ruleSummaries": [rule] if rule else []}
        return json.dumps({"report": {"jobs": [{"validationResult": [{"compliant": compliant, "details": details}]}]}})

    return _make
