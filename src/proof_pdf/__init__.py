"""Print-PDF conformance verification.

Wraps veraPDF (the PDF/A engine) and adds the checks veraPDF cannot make: the
PDF/X-4 structural identity (page boxes, ``/Trapped``, the X output intent and
XMP), the finished-page geometry, and best-effort colour sanity (total ink
coverage, overprint, rich-black small text, stray RGB, effective image DPI).

The entry point is :func:`proof_pdf.core.verify_file`; the CLI group lives in
:mod:`proof_pdf.cli` and is the ``proof-pdf`` console script.

Public API is the top-level modules (:mod:`~proof_pdf.core`, :mod:`~proof_pdf.models`,
:mod:`~proof_pdf.inventory`,
:mod:`~proof_pdf.extract`, :mod:`~proof_pdf.crop`, :mod:`~proof_pdf.layout`, :mod:`~proof_pdf.preview`,
:mod:`~proof_pdf.texlog`, :mod:`~proof_pdf.config`, :mod:`~proof_pdf.cli`) and the
:func:`~proof_pdf.config.load_profile` re-export. Everything under ``proof_pdf._internal`` (the checks, the content and
paint walks, the tool wrappers, the config reader) is private and may change at any time.

The package takes PDF paths and nothing else. The finished-page geometry it checks
against is a parameter (:class:`PageGeometry`) with no default, so a file from a
printer is held to its own dimensions or the layer is skipped.
"""

from .config import ConfigError, LoadedConfig, load_profile

__all__ = ["ConfigError", "LoadedConfig", "load_profile"]
