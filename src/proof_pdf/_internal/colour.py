"""Total area coverage, the colour check neither PDF/A nor PDF/X judges that still runs here.

The summed ink of the plates: rasterise each page with Ghostscript's ``tiffsep`` and take the peak; over the FOGRA51
ceiling the ink will not dry and will set off. This is the objective one, so it is an ``ERROR``. Ghostscript's absence
or failure is surfaced as a failure by the caller, not silently skipped.

The other colour checks (overprint, rich black, K-only text, the blend space, spot inks and their definitions, the CMYK,
RGB and gray job rules and the ICC profile rules) run in the Rust core (``rust/src/checks``, through :mod:`.ported`).
"""

import asyncio
import logging
import pathlib
import shutil
import tempfile

from PIL import Image, ImageMath

from ..models import Finding, Severity
from . import proc

logger = logging.getLogger(__name__)


class GhostscriptError(RuntimeError):
    """Ghostscript exited non-zero, or timed out, while rasterising for ink coverage."""


# Rasterising DeviceCMYK: 150 dpi resolves solids and small type without the
# per-pixel cost of press resolution. TAC is an ink property, so more dpi buys
# nothing here.
_TAC_DPI = 150
# An ink amount this close to zero is no ink, and this close to one is solid.


def gs_available(binary: str) -> bool:
    """Whether the Ghostscript *binary* resolves (TAC needs it, nothing else)."""
    return shutil.which(binary) is not None


# ------------------------------------------------------------ total ink ----
async def check_tac(
    binary: str, path: str, max_tac: float, *, warn_tac: float | None = None, timeout: float | None = None
) -> list[Finding]:
    """Flag any page whose peak total ink coverage exceeds *max_tac* percent (and, as a warning, *warn_tac*)."""
    findings: list[Finding] = []
    for i, peak in enumerate(await _page_tac_peaks(binary, path, timeout=timeout), start=1):
        if peak > max_tac:
            findings.append(
                Finding("tac", Severity.ERROR, f"peak ink coverage {peak:.0f}% exceeds the {max_tac:.0f}% limit", i)
            )
        elif warn_tac is not None and peak > warn_tac:
            findings.append(
                Finding(
                    "tac", Severity.WARN, f"peak ink coverage {peak:.0f}% exceeds the {warn_tac:.0f}% advisory level", i
                )
            )
    return findings


async def _page_tac_peaks(binary: str, path: str, *, timeout: float | None = None) -> list[float]:
    """The peak total ink (100% per ink) of each page, from Ghostscript's ``tiffsep``: one plate per ink.

    Each spot colour is its own plate at its tint, never its CMYK alternate, and a spot overprinting process ink adds to
    it, as on press (decided 2026-11-07). ``tiff32nc`` converted spots to CMYK and dropped overprinted ones.
    """
    with tempfile.TemporaryDirectory() as tmp:
        pattern = str(pathlib.Path(tmp) / "page-%03d.tif")
        try:
            result = await proc.run(
                [
                    binary,
                    "-q",
                    "-dNOPAUSE",
                    "-dBATCH",
                    "-dSAFER",
                    "-sDEVICE=tiffsep",
                    f"-r{_TAC_DPI}",
                    f"-sOutputFile={pattern}",
                    path,
                ],
                timeout=timeout,
            )
        except proc.ProcTimeoutError as exc:
            raise GhostscriptError(f"ghostscript timed out after {timeout:.0f}s") from exc
        if result.returncode != 0:
            message = result.stderr.strip() or "ghostscript failed"
            logger.error(f"ghostscript failed (rc={result.returncode}): {message}")
            raise GhostscriptError(message)
        # tiffsep writes page-NNN.tif (a composite, unused) and page-NNN(Ink).tif per plate. Sort by the numeric page
        # index, not lexicographically: `page-1000` must not sort before `page-999`.
        pages = sorted(pathlib.Path(tmp).glob("page-[0-9]*[0-9].tif"), key=lambda t: int(t.stem.rsplit("-", 1)[-1]))
        peaks: list[float] = []
        for composite in pages:
            plates = sorted(composite.parent.glob(f"{composite.stem}(*).tif"))
            peaks.append(await asyncio.to_thread(_peak_tac, plates))
            # A 50-page file at 150 dpi is hundreds of MB of plates if it all stays on disk; each page's plates are
            # dead the moment its peak is read.
            for tif in (composite, *plates):
                tif.unlink(missing_ok=True)
        return peaks


def _peak_tac(plates: list[pathlib.Path]) -> float:
    """Peak ink summed over every plate at each pixel, as a percentage. A ``tiffsep`` plate is 0 for full ink."""
    total = None
    for tif in plates:
        with Image.open(tif) as img:
            ink = ImageMath.lambda_eval(lambda a: 255 - a["v"], v=img.convert("I"))
        total = ink if total is None else ImageMath.lambda_eval(lambda a: a["t"] + a["i"], t=total, i=ink)
    return 0.0 if total is None else total.getextrema()[1] / 255.0 * 100.0
