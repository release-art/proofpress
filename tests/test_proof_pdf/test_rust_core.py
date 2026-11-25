"""The Rust core, as Python sees it: the extension module built from rust/ by maturin."""

import json

import pytest

from proof_pdf._internal import _core

from .test_integration import PDFS


def test_verify_returns_a_schema_1_report():
    report = json.loads(_core.verify("x4-good.pdf", (PDFS / "x4-good.pdf").read_bytes(), "x4"))
    assert (report["schema"], report["file"], report["structure"]) == (1, "x4-good.pdf", "sound")


def test_an_unknown_level_is_a_value_error():
    with pytest.raises(ValueError, match="unknown level"):
        _core.verify("x.pdf", b"", "x3")
