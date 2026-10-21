"""Typed values shared across the PDF verification checks.

A :class:`Finding` is one thing a check noticed; a :class:`Profile` is what a
file is being held to (a veraPDF PDF/A flavour plus the optional PDF/X-4,
geometry and colour layers); a :class:`FileReport` collects everything found in
one file.

These are pure in-process value objects, so they are frozen dataclasses living
beside the code that builds them.
"""

import dataclasses
import enum
import pathlib

MM_PER_IN = 25.4
PT_PER_IN = 72.0


def mm_to_pt(mm: float) -> float:
    """Millimetres as PDF user-space points."""
    return mm / MM_PER_IN * PT_PER_IN


def pt_to_mm(pt: float) -> float:
    """PDF user-space points as millimetres."""
    return pt / PT_PER_IN * MM_PER_IN


@enum.unique
class Severity(enum.StrEnum):
    """Whether a finding fails the file or is only advisory."""

    ERROR = "error"
    WARN = "warn"


@dataclasses.dataclass(frozen=True)
class Finding:
    """One issue a check raised, optionally tied to a 1-based page number."""

    check: str
    severity: Severity
    message: str
    page: int | None = None

    def label(self) -> str:
        """A one-line rendering: ``[check] (page N) message``."""
        where = f" (page {self.page})" if self.page is not None else ""
        return f"[{self.check}]{where} {self.message}"


# The FOGRA51 / PSO Coated v3 total-area-coverage ceiling print jobs commonly target.
DEFAULT_MAX_TAC = 300.0
# Text at or below this point size is where rich-black mis-registration shows.
DEFAULT_SMALL_TEXT_PT = 12.0
# Placed artwork below this effective resolution shows softness on a coated press.
DEFAULT_MIN_DPI = 300.0


@dataclasses.dataclass(frozen=True)
class PageGeometry:
    """The finished-page size a PDF is expected to declare.

    There is deliberately no default size: the caller says what the stock is.
    """

    trim_w_mm: float
    trim_h_mm: float
    bleed_mm: float  # per side
    tolerance_mm: float = 0.25


@dataclasses.dataclass(frozen=True)
class Profile:
    """What a PDF is checked against.

    *flavour* is a veraPDF PDF/A flavour string (``2b``, ``2a``, ``2u``, ...) and
    is passed through verbatim, so the machinery is universal even though the
    print jobs usually target ``2b``. *pdfx* toggles the PDF/X-4 structural layer;
    *colour* toggles the ink-coverage / overprint / rich-black / colour-space
    layer; *geometry* is the expected finished size, or ``None`` to skip it.
    *k_only_text* is an opt-in house rule: text may be painted with the K plate only.
    *missing_glyphs* flags ``.notdef`` glyphs; *min_text_pt*, *min_stroke_pt* and *safe_mm* are opt-in thresholds with
    no default, because a size floor or a safe margin is a house rule and not something to infer.
    *fonts* flags unembedded and Type 3 fonts; *allow_spots* (exact names) turns on the spot-ink allowlist;
    *output_condition* names the printing condition the output intent must carry; *uniform_pages* and
    *pages_multiple_of* are page-set rules.
    *min_delta_l* turns on the contrast check (an L* difference floor), measured through *proof_profile* or the PDF's
    own output intent.
    """

    flavour: str = "2b"
    pdfa: bool = True
    pdfx: bool = True
    colour: bool = True
    geometry: PageGeometry | None = None
    max_tac: float = DEFAULT_MAX_TAC
    small_text_pt: float = DEFAULT_SMALL_TEXT_PT
    min_dpi: float = DEFAULT_MIN_DPI
    strict_cmyk: bool = False
    k_only_text: bool = False
    missing_glyphs: bool = True
    min_text_pt: float | None = None
    min_stroke_pt: float | None = None
    safe_mm: float | None = None
    min_delta_l: float | None = None
    proof_profile: pathlib.Path | None = None
    fonts: bool = True
    allow_spots: tuple[str, ...] | None = None
    output_condition: str | None = None
    uniform_pages: bool = False
    pages_multiple_of: int | None = None


@dataclasses.dataclass
class FileReport:
    """Every finding gathered for a single PDF, or the reason it was exempt from checking."""

    path: pathlib.Path
    findings: list[Finding] = dataclasses.field(default_factory=list)
    exempt: str | None = None  # why the file was declared exempt and not checked; never a failure

    @property
    def errors(self) -> list[Finding]:
        return [f for f in self.findings if f.severity is Severity.ERROR]

    @property
    def warnings(self) -> list[Finding]:
        return [f for f in self.findings if f.severity is Severity.WARN]

    @property
    def ok(self) -> bool:
        """True when nothing failed (advisory warnings do not count)."""
        return not self.errors
