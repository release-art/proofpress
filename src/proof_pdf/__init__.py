"""Print-PDF conformance verification.

Wraps veraPDF (the PDF/A engine) and adds the checks veraPDF cannot make: the
PDF/X-4 structural identity (page boxes, ``/Trapped``, the X output intent and
XMP), the finished-page geometry, and best-effort colour sanity (total ink
coverage, overprint, rich-black small text, stray RGB, effective image DPI).

The entry point is :func:`proof_pdf.core.verify_file`; the CLI group lives in
:mod:`proof_pdf.cli` and is the ``proof-pdf`` console script.

The package takes PDF paths and nothing else. The finished-page geometry it checks
against is a parameter (:class:`PageGeometry`) with no default, so a file from a
printer is held to its own dimensions or the layer is skipped.
"""
