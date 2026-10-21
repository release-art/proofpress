"""The committed integration PDFs are exactly what the builder produces (no tools needed)."""

import importlib.util
import pathlib
import sys

FIXTURES = pathlib.Path(__file__).parents[1] / "fixtures"


def _builder():
    spec = importlib.util.spec_from_file_location("build_pdfs", FIXTURES / "build_pdfs.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_committed_pdfs_match_a_rebuild():
    """Regenerate with ``pdm run python tests/fixtures/build_pdfs.py`` after changing the builder."""
    built = _builder().render()
    committed = {p.stem: p.read_bytes() for p in (FIXTURES / "pdfs").glob("*.pdf")}
    assert committed.keys() == built.keys()
    assert [name for name in built if built[name] != committed[name]] == []
