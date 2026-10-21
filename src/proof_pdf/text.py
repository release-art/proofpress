"""Checks on the text and lines a page paints: missing glyphs, size floors, hairlines, safe area.

All of them read the shared paint walk (:mod:`.paint`), so they see text inside Form XObjects and measure it in page
points. ``missing-glyph`` is exact and always worth running; the others take a threshold from the caller and have no
default: a minimum size or a safe margin is a house rule, never an inference.

``missing-glyph`` sees only what is in the PDF. LuaTeX typesets the ``.notdef`` glyph for a character the font lacks,
so it is found here; pdfTeX drops the character, which leaves no trace in the PDF (the log says "Missing character"; see
:mod:`.texlog`).
"""

import pikepdf

from . import structure
from .models import MM_PER_IN, PT_PER_IN, Finding, Severity
from .paint import Paint, paint_events

# A glyph box rounds off at about a thousandth of a point; anything inside this is not "outside".
_EDGE_TOLERANCE_PT = 0.05


def _text_runs(page: pikepdf.Page) -> list[Paint]:
    return [p for p in paint_events(page) if p.kind == "text" and p.glyphs]


def check_missing_glyphs(pdf: pikepdf.Pdf) -> list[Finding]:
    """Flag characters shown with the ``.notdef`` glyph, which prints as a box or nothing."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        per_font: dict[str, int] = {}
        for run in _text_runs(page):
            if run.notdef and run.font is not None:
                per_font[run.font.name] = per_font.get(run.font.name, 0) + run.notdef
        if per_font:
            detail = ", ".join(f"{count} in {name}" for name, count in sorted(per_font.items()))
            findings.append(
                Finding(
                    "missing-glyph",
                    Severity.ERROR,
                    f"{sum(per_font.values())} character(s) are shown with the .notdef glyph ({detail}): "
                    "the font has no glyph for them",
                    i,
                )
            )
    return findings


def check_min_text_size(pdf: pikepdf.Pdf, min_pt: float) -> list[Finding]:
    """Flag text set smaller than *min_pt* on the page (effective size, after every scale)."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        sizes = [run.size for run in _text_runs(page) if 0 < run.size < min_pt]
        if sizes:
            findings.append(
                Finding(
                    "text-size",
                    Severity.ERROR,
                    f"text as small as {min(sizes):.1f}pt is below the {min_pt:g}pt minimum ({len(sizes)} run(s))",
                    i,
                )
            )
    return findings


def check_min_stroke(pdf: pikepdf.Pdf, min_pt: float) -> list[Finding]:
    """Flag stroked lines thinner than *min_pt*; a zero width is the thinnest line the device can draw."""
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        widths = [p.stroke_width for p in paint_events(page) if p.stroke_width is not None and p.stroke_width < min_pt]
        if widths:
            thinnest = min(widths)
            label = "a hairline (zero width)" if thinnest <= 0 else f"{thinnest:.2f}pt"
            findings.append(
                Finding(
                    "hairline",
                    Severity.ERROR,
                    f"lines as thin as {label} are below the {min_pt:g}pt minimum ({len(widths)} stroke(s))",
                    i,
                )
            )
    return findings


def check_safe_area(pdf: pikepdf.Pdf, safe_mm: float) -> list[Finding]:
    """Flag text that reaches past the trim edge inset by *safe_mm*.

    The trim box is the finished edge (the media box when a page declares none). A text box is the glyph advance
    from the font's declared widths and its ascent and descent, so it is exact for fonts that declare widths and an
    estimate for those that do not; an estimate is reported as a warning.
    """
    inset = safe_mm / MM_PER_IN * PT_PER_IN
    findings: list[Finding] = []
    for i, page in enumerate(pdf.pages, start=1):
        trim = structure.own_box(page.obj, "TrimBox") or structure.media_box(page.obj)
        if trim is None:
            continue
        safe = (trim[0] + inset, trim[1] + inset, trim[2] - inset, trim[3] - inset)
        outside: list[tuple[float, Paint]] = []
        for run in _text_runs(page):
            overshoot = _overshoot(run.bbox, safe)
            if overshoot > _EDGE_TOLERANCE_PT:
                outside.append((overshoot, run))
        if outside:
            worst, run = max(outside, key=lambda item: item[0])
            measured = run.font is not None and bool(run.font.widths)
            findings.append(
                Finding(
                    "safe-area",
                    Severity.ERROR if measured else Severity.WARN,
                    f"{len(outside)} text run(s) reach outside the {safe_mm:g}mm safe area, "
                    f"by up to {worst / PT_PER_IN * MM_PER_IN:.1f}mm"
                    + ("" if measured else " (estimated: the font declares no glyph widths)"),
                    i,
                )
            )
    return findings


def _overshoot(bbox: structure.Rect | None, safe: structure.Rect) -> float:
    """How far *bbox* extends past *safe*, in points (0 when inside)."""
    if bbox is None:
        return 0.0
    return max(safe[0] - bbox[0], safe[1] - bbox[1], bbox[2] - safe[2], bbox[3] - safe[3], 0.0)
