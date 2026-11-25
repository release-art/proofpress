"""The Rust core (rust/src/python.rs), built by maturin. Private: the public modules wrap it."""

def verify(file: str, data: bytes, level: str) -> str:
    """Verify ``data``, the bytes of ``file``, at a PDF/X level (``x4``, ``x4p``, ``x5g``, ``x5pg``, ``x5n``).

    Returns the report as JSON, schema 1. Raises ``ValueError`` for an unknown level.
    """
