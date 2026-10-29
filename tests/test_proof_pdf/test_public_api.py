"""The public/private line: top-level modules are public API, everything else lives in ``_internal``."""

import pkgutil

import proof_pdf

PUBLIC = {"cli", "config", "core", "crop", "extract", "inventory", "layout", "models", "preview", "texlog"}


def test_only_the_public_modules_sit_at_the_top_level():
    top = {m.name for m in pkgutil.iter_modules(proof_pdf.__path__)}
    assert top - {"_internal"} == PUBLIC, (
        "a new module is either public (add it here, document it) or goes in _internal"
    )
