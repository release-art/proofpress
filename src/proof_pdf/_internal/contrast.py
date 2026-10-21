"""How far apart text and its background are in lightness, measured the way ink on paper is read.

This is deliberately **not** WCAG. WCAG contrast is a screen model; for ink on a substrate the figure that means
anything is the L* difference, measured through the colour profile the job prints with. So colours go through an ICC
profile to CIE L*a*b* (the PDF's own output intent, or one passed in), the page's white is the paper, and the report is
the L* difference (and CIEDE2000 beside it). A threshold is the caller's: there is no default.

The background of a text run is the last filled shape painted under the centre of its box, or the paper if there is
none. A shape that is an image or a shading has an unknown colour, so text over one is reported as unmeasured and
never as a pass or a fail. Clipping, transparency and blend modes are not modelled, so this is an estimate of the
design's intent, and says so.
"""

import dataclasses
import io
import pathlib

import pikepdf
from PIL import Image, ImageCms

from ..models import Finding, Severity
from . import deltae
from .paint import Fill, Rect, paint_events
from .spaces import CMYK, GRAY, RGB

Lab = tuple[float, float, float]

_INTENT = ImageCms.Intent.RELATIVE_COLORIMETRIC
_UNBOUNDED: Rect = (float("-inf"), float("-inf"), float("inf"), float("inf"))


class NoProfileError(RuntimeError):
    """A colour needs a profile the job does not have (CMYK ink with no CMYK output intent or ``--proof-profile``)."""


class ColourModel:
    """Converts fills to L*a*b* through the job's ICC profiles."""

    def __init__(self, cmyk: ImageCms.ImageCmsProfile | None, rgb: ImageCms.ImageCmsProfile | None = None) -> None:
        lab = ImageCms.createProfile("LAB")
        srgb = rgb or ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB"))
        self._to_lab = {
            "RGB": ImageCms.buildTransform(srgb, lab, "RGB", "LAB", renderingIntent=_INTENT),
        }
        if cmyk is not None:
            self._to_lab["CMYK"] = ImageCms.buildTransform(cmyk, lab, "CMYK", "LAB", renderingIntent=_INTENT)

    @classmethod
    def for_pdf(cls, pdf: pikepdf.Pdf, proof_profile: pathlib.Path | None = None) -> "ColourModel":
        """The profiles a job prints with: *proof_profile* if given, else the PDF's output intent."""
        cmyk = rgb = None
        profiles = [_load(proof_profile.read_bytes())] if proof_profile else [_load(b) for b in _intent_profiles(pdf)]
        for profile in profiles:
            space = profile.profile.xcolor_space.strip()  # pyright: ignore[reportAttributeAccessIssue]
            if space == "CMYK":
                cmyk = cmyk or profile
            elif space == "RGB":
                rgb = rgb or profile
        return cls(cmyk, rgb)

    @property
    def paper(self) -> Fill:
        return Fill(CMYK, (0.0, 0.0, 0.0, 0.0)) if "CMYK" in self._to_lab else Fill(RGB, (1.0, 1.0, 1.0))

    def lab(self, fill: Fill) -> Lab | None:
        """*fill* as L*a*b*, or ``None`` for a colour no profile describes (a spot ink or pattern)."""
        if fill.family == CMYK and len(fill.value) >= 4:
            return self._convert("CMYK", fill.value[:4])
        if fill.family == RGB and len(fill.value) >= 3:
            return self._convert("RGB", fill.value[:3])
        if fill.family == GRAY and fill.value:
            if "CMYK" in self._to_lab:  # DeviceGray black is the K plate
                return self._convert("CMYK", (0.0, 0.0, 0.0, 1.0 - fill.value[0]))
            return self._convert("RGB", (fill.value[0],) * 3)
        return None

    def _convert(self, mode: str, components: tuple[float, ...]) -> Lab:
        if mode not in self._to_lab:
            raise NoProfileError("no CMYK profile: the PDF has no CMYK output intent and none was given")
        pixel = tuple(max(0, min(255, round(c * 255))) for c in components)
        out = ImageCms.applyTransform(Image.new(mode, (1, 1), pixel), self._to_lab[mode])
        assert out is not None
        raw_l, raw_a, raw_b = out.getpixel((0, 0))  # pyright: ignore[reportGeneralTypeIssues]
        return (raw_l * 100.0 / 255.0, float(raw_a) - 128.0, float(raw_b) - 128.0)


def _load(data: bytes) -> ImageCms.ImageCmsProfile:
    return ImageCms.ImageCmsProfile(io.BytesIO(data))


def _intent_profiles(pdf: pikepdf.Pdf) -> list[bytes]:
    profiles: list[bytes] = []
    for intent in pdf.Root.get("/OutputIntents", []):
        stream = intent.get("/DestOutputProfile")
        if stream is not None:
            profiles.append(stream.read_bytes())
    return profiles


@dataclasses.dataclass(frozen=True)
class ContrastPair:
    """One text colour on one background, on one page."""

    page: int
    text: Fill
    ground: Fill | None  # None: an image or shading, colour unknown
    on_paper: bool
    runs: int
    smallest_pt: float
    delta_l: float | None  # L* difference; None when either colour cannot be converted or the ground is unknown
    delta_e: float | None  # CIEDE2000


def measure(pdf: pikepdf.Pdf, model: ColourModel) -> list[ContrastPair]:
    """Every distinct (text colour, background) pair on every page, with its L* and CIEDE2000 difference."""
    pairs: list[ContrastPair] = []
    for number, page in enumerate(pdf.pages, start=1):
        found: dict[tuple[Fill, Fill | None, bool], list[float]] = {}
        grounds: list[tuple[Rect, Fill | None]] = []
        for p in paint_events(page):
            if p.kind in ("image", "shading"):
                grounds.append((p.bbox or _UNBOUNDED, None))
            elif p.kind == "path" and p.filled and p.bbox is not None:
                grounds.append((p.bbox, p.fill if p.fill.family != "other" else None))
            elif p.kind == "text" and p.glyphs and p.bbox is not None:
                ground, on_paper = _ground_under(p.bbox, grounds, model.paper)
                found.setdefault((p.fill, ground, on_paper), []).append(p.size)
        for (text, ground, on_paper), sizes in found.items():
            delta_l, delta_e = _differences(model, text, ground)
            pairs.append(ContrastPair(number, text, ground, on_paper, len(sizes), min(sizes), delta_l, delta_e))
    return pairs


def _ground_under(bbox: Rect, grounds: list[tuple[Rect, Fill | None]], paper: Fill) -> tuple[Fill | None, bool]:
    cx, cy = (bbox[0] + bbox[2]) / 2, (bbox[1] + bbox[3]) / 2
    for (x0, y0, x1, y1), fill in reversed(grounds):  # the last painted is on top
        if x0 <= cx <= x1 and y0 <= cy <= y1:
            return fill, False
    return paper, True


def _differences(model: ColourModel, text: Fill, ground: Fill | None) -> tuple[float | None, float | None]:
    if ground is None:
        return None, None
    a, b = model.lab(text), model.lab(ground)
    if a is None or b is None:
        return None, None
    return abs(a[0] - b[0]), deltae.ciede2000(a, b)


def describe(fill: Fill | None) -> str:
    if fill is None:
        return "an image or shading"
    if fill.family == CMYK and len(fill.value) >= 4:
        c, m, y, k = (round(v * 100) for v in fill.value[:4])
        return f"C{c} M{m} Y{y} K{k}"
    if fill.family == GRAY and fill.value:
        return f"gray {round(fill.value[0] * 100)}%"
    if fill.family == RGB and len(fill.value) >= 3:
        r, g, b = (round(v * 255) for v in fill.value[:3])
        return f"RGB({r},{g},{b})"
    return "a spot or pattern colour"


def check_contrast(pdf: pikepdf.Pdf, min_delta_l: float, proof_profile: pathlib.Path | None = None) -> list[Finding]:
    """Flag text whose L* difference from its background is below *min_delta_l*.

    Text over an image or shading, or in a colour no profile describes, cannot be measured: that is a warning,
    not a pass. CMYK ink with no CMYK profile to measure it through is an error, because the check was asked for.
    """
    try:
        pairs = measure(pdf, ColourModel.for_pdf(pdf, proof_profile))
    except NoProfileError as exc:
        return [Finding("contrast", Severity.ERROR, f"cannot measure contrast: {exc}")]
    findings: list[Finding] = []
    unmeasured: set[int] = set()
    for pair in pairs:
        if pair.delta_l is None:
            unmeasured.add(pair.page)
        elif pair.delta_l < min_delta_l:
            ground = "the paper" if pair.on_paper else describe(pair.ground)
            findings.append(
                Finding(
                    "contrast",
                    Severity.ERROR,
                    f"text in {describe(pair.text)} on {ground} differs by only {pair.delta_l:.1f} L* "
                    f"(minimum {min_delta_l:g}; {pair.runs} run(s), smallest {pair.smallest_pt:.1f}pt)",
                    pair.page,
                )
            )
    findings += [
        Finding(
            "contrast",
            Severity.WARN,
            "text over an image, a shading or a spot colour: contrast not measured",
            page,
        )
        for page in sorted(unmeasured)
    ]
    return findings
