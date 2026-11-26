"""The Rust core (rust/src/python.rs), built by maturin. Private: the public modules wrap it."""

def verify(file: str, data: bytes, level: str, params: str = "{}") -> str:
    """Verify ``data``, the bytes of ``file``, at a level (``x4``, ``x4p``, ``x5g``, ``x5pg``, ``x5n`` or ``print``).

    ``params`` is a JSON object of the print checks' parameters by name (``small_text_pt``); any left out keep their
    defaults. Returns the report as JSON, schema 1. Raises ``ValueError`` for an unknown level or a bad parameter.
    """
