"""``python -m proof_pdf._internal.pdfx``: rewrite COVERAGE.md from the rule tables."""

from . import COVERAGE, coverage_markdown

COVERAGE.write_text(coverage_markdown())
