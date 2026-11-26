"""The Rust core (rust/src/python.rs), built by maturin. Private: the public modules wrap it."""

def verify(file: str, data: bytes, level: str, params: str = "{}") -> str:
    """Verify ``data``, the bytes of ``file``, at a level (``x4``, ``x4p``, ``x5g``, ``x5pg``, ``x5n`` or ``print``).

    ``params`` is a JSON object of the print checks' parameters by name (``small_text_pt``); any left out keep their
    defaults. Returns the report as JSON, schema 1. Raises ``ValueError`` for an unknown level or a bad parameter.
    """

def run(argv: list[str], colour: bool = False) -> tuple[int, str, str]:
    """Run the ``proof-pdf`` command line on ``argv`` (program name first); return its exit code, stdout and stderr.

    ``colour`` styles the text output for a terminal.
    """

class UnreadablePdfError(Exception):
    """A PDF that cannot be read: its structure is too damaged, or it is encrypted."""

def crop(data: bytes, page: int, bounds: tuple[float, float, float, float], pad: float = 0.0) -> bytes:
    """Cut ``bounds`` (x0, y0, x1, y1 in points, either corner order, widened by ``pad``) of page ``page`` (from 1) of
    the PDF ``data`` out as a one-page PDF. Raises ``ValueError`` for a missing page or a box that misses it, and
    ``UnreadablePdfError`` for a file that cannot be read.
    """
