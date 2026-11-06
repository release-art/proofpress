"""The gray job mode (``--colour-mode gray``) and its ``--allow-k-only`` opt-in."""

import io
import zlib

import pikepdf
import pytest
from PIL import Image

from proof_pdf._internal import colour
from proof_pdf.models import ColourMode, Severity

from ._samples import build
from .test_colour import _icc_space, _page_with, _pdf_with_pattern, _pdf_with_profiles, _shading, _tiling

FAKE_CMYK = bytes(16) + b"CMYK" + bytes(180)  # only the header signature matters to the colour family
K_ONLY = b"0 0 0 0.7 k"
RICH = b"0.1 0 0 1 k"
PATTERN = b"/Pattern cs /P1 scn 0 0 10 10 re f"


def _check(pdf, *, k_only=False):
    return colour.check_gray_only(pdf, allow_k_only=k_only)


def _flagged(findings, severity=Severity.ERROR) -> bool:
    return any(f.severity is severity for f in findings)


def _paint(make_content_pdf, stream, **kw):
    with pikepdf.open(make_content_pdf(stream, **kw)) as pdf:
        return _check(pdf, k_only=False), _check(pdf, k_only=True)


class TestVector:
    @pytest.mark.parametrize(
        ("stream", "strict", "allowed"),
        [
            (b"0.5 g 0 0 10 10 re f", False, False),
            (b"0.5 G 0 0 10 10 re S", False, False),
            (b"/CS0 cs 0.5 scn 0 0 10 10 re f", False, False),  # CalGray-style named gray
            (b"1 0 0 rg 0 0 10 10 re f", True, True),
            (b"1 0 0 RG 0 0 10 10 re S", True, True),
            (b"1 0 0 RG 0 0 10 10 re f", False, False),  # colour set, never stroked
            (K_ONLY + b" 0 0 10 10 re f", True, False),  # strict: K-only is not gray
            (K_ONLY.upper() + b" 0 0 10 10 re S", True, False),
            (b"BT /F1 12 Tf " + K_ONLY + b" (a) Tj ET", True, False),
            (b"BT /F1 12 Tf 1 Tr 0.5 g " + K_ONLY.upper() + b" (a) Tj ET", True, False),  # the stroke is K-only
            (RICH + b" 0 0 10 10 re f", True, True),  # any C, M or Y fails both
            (b"0 0 0 0 k 0 0 10 10 re f", True, False),  # paper white in CMYK is K-only too
            (b"/CS1 cs 0 0 0 0.7 scn 0 0 10 10 re f", True, True),  # ICC CMYK is never K-only
            (b"/CS2 cs 0 0 0 0.7 scn 0 0 10 10 re f", True, False),  # a named DeviceCMYK is plain CMYK
            (b"/CS3 cs 0.5 0.5 0 scn 0 0 10 10 re f", True, True),  # Lab
        ],
    )
    def test_each_painted_colour(self, make_content_pdf, stream, strict, allowed):
        path = make_content_pdf(
            stream,
            colorspaces={
                "/CS0": pikepdf.Name.DeviceGray,
                "/CS2": pikepdf.Name.DeviceCMYK,
                "/CS3": pikepdf.Array([pikepdf.Name.Lab, pikepdf.Dictionary(WhitePoint=[0.96, 1, 0.82])]),
            },
        )
        with pikepdf.open(path) as pdf:
            pdf.pages[0].Resources.ColorSpace["/CS1"] = pikepdf.Array(
                [pikepdf.Name.ICCBased, pdf.make_stream(FAKE_CMYK, N=4)]
            )
            assert _flagged(_check(pdf)) is strict
            assert _flagged(_check(pdf, k_only=True)) is allowed

    def test_the_message_says_what_it_found(self, make_content_pdf):
        strict, _ = _paint(make_content_pdf, b"1 0 0 rg 0 0 10 10 re f")
        assert strict[0].check == "gray-only" and "RGB" in strict[0].message and strict[0].page == 1

    def test_icc_cmyk_says_it_is_not_treated_as_k_only(self, make_content_pdf):
        path = make_content_pdf(b"/CS1 cs 0 0 0 0.7 scn 0 0 10 10 re f")
        with pikepdf.open(path) as pdf:
            pdf.pages[0].Resources.ColorSpace = pikepdf.Dictionary(
                CS1=pikepdf.Array([pikepdf.Name.ICCBased, pdf.make_stream(FAKE_CMYK, N=4)])
            )
            [finding] = _check(pdf, k_only=True)
        assert "ICC-based" in finding.message and "not treated as K-only" in finding.message

    def test_a_spot_is_a_warning_even_one_named_black(self, make_content_pdf):
        space = pikepdf.Array(
            [
                pikepdf.Name.Separation,
                pikepdf.Name("/Black"),
                pikepdf.Name.DeviceCMYK,
                pikepdf.Dictionary(FunctionType=2, Domain=[0, 1], C0=[0, 0, 0, 0], C1=[0, 0, 0, 1], N=1),
            ]
        )
        path = make_content_pdf(b"/CS0 cs 1 scn 0 0 10 10 re f", colorspaces={"/CS0": space})
        with pikepdf.open(path) as pdf:
            for k_only in (False, True):
                [finding] = _check(pdf, k_only=k_only)
                assert finding.severity is Severity.WARN and "Black" in finding.message

    def test_form_content_is_walked(self, make_content_pdf, make_form):
        pdf = pikepdf.new()
        form = pdf.make_indirect(make_form(pdf, b"1 0 0 rg 0 0 10 10 re f"))
        _page_with(pdf, b"/Fm1 Do", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        assert _flagged(_check(pdf))


def _cmyk_image(pdf, data: bytes, *, space: pikepdf.Object = pikepdf.Name.DeviceCMYK, decode=None, filter_=True, bpc=8):
    image = pdf.make_stream(
        zlib.compress(data) if filter_ else data,
        Type=pikepdf.Name.XObject,
        Subtype=pikepdf.Name.Image,
        Width=len(data) // 4,
        Height=1,
    )
    image.BitsPerComponent = bpc
    image.ColorSpace = space
    if filter_:
        image.Filter = pikepdf.Name.FlateDecode
    if decode is not None:
        image.Decode = decode
    return pdf.make_indirect(image)


def _broken_jpeg(pdf):
    image = _cmyk_image(pdf, b"junk", filter_=False)
    image.Filter = pikepdf.Name.DCTDecode
    return image


def _pdf_with_image(image_maker, stream=b"q 10 0 0 10 0 0 cm /Im1 Do Q") -> pikepdf.Pdf:
    pdf = pikepdf.new()
    _page_with(pdf, stream, pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=image_maker(pdf))))
    return pdf


class TestImages:
    @pytest.mark.parametrize(
        ("space", "strict"),
        [(pikepdf.Name.DeviceGray, False), (pikepdf.Name.DeviceRGB, True), (pikepdf.Name.DeviceCMYK, True)],
    )
    def test_image_by_colour_space(self, make_image, space, strict):
        pdf = _pdf_with_image(lambda p: p.make_indirect(make_image(p, colorspace=space)))
        assert _flagged(_check(pdf)) is strict

    def test_k_only_cmyk_image_passes_only_when_allowed(self):
        pdf = _pdf_with_image(lambda p: _cmyk_image(p, b"\x00\x00\x00\x80" * 4))
        assert _flagged(_check(pdf)) and not _check(pdf, k_only=True)

    @pytest.mark.parametrize("pixel", [b"\x01\x00\x00\x80", b"\x00\x01\x00\x80", b"\x00\x00\x01\x80"])
    def test_any_c_m_or_y_ink_fails_even_when_allowed(self, pixel):
        pdf = _pdf_with_image(lambda p: _cmyk_image(p, b"\x00\x00\x00\x80" * 3 + pixel))
        [finding] = _check(pdf, k_only=True)
        assert "has C, M or Y ink" in finding.message and "/Im1" in finding.message

    def test_an_inverted_decode_array_is_honoured(self):
        # /Decode [1 0] on C, M and Y: a sample of 255 is no ink, 0 is full ink.
        decode = [1, 0, 1, 0, 1, 0, 0, 1]
        pdf = _pdf_with_image(lambda p: _cmyk_image(p, b"\xff\xff\xff\x80" * 2, decode=decode))
        assert _check(pdf, k_only=True) == []
        pdf = _pdf_with_image(lambda p: _cmyk_image(p, b"\xff\xff\xff\x80" + b"\xfe\xff\xff\x80", decode=decode))
        assert _flagged(_check(pdf, k_only=True))

    @pytest.mark.parametrize(
        "make",
        [
            lambda p: _cmyk_image(p, b"\x00" * 16, bpc=4),  # not 8 bits
            lambda p: _cmyk_image(p, b"\x00" * 16, decode=[0, 0.5] * 4),  # a decode we do not read
            lambda p: _broken_jpeg(p),
        ],
        ids=["bpc", "decode", "broken-jpeg"],
    )
    def test_an_image_that_cannot_be_read_fails(self, make):
        [finding] = _check(_pdf_with_image(make), k_only=True)
        assert finding.severity is Severity.ERROR and "could not be decoded" in finding.message

    def test_a_real_jpeg_is_decoded(self):
        def jpeg(channels):
            buf = io.BytesIO()
            Image.new("CMYK", (8, 8), channels).save(buf, "JPEG", quality=95)
            return buf.getvalue()

        def image(data):
            def make(p):
                made = p.make_stream(
                    data,
                    Type=pikepdf.Name.XObject,
                    Subtype=pikepdf.Name.Image,
                    Width=8,
                    Height=8,
                    BitsPerComponent=8,
                    ColorSpace=pikepdf.Name.DeviceCMYK,
                    Filter=pikepdf.Name.DCTDecode,
                )
                made.Decode = [1, 0, 1, 0, 1, 0, 1, 0]  # Pillow writes Adobe (inverted) CMYK, as LaTeX includes it
                return p.make_indirect(made)

            return make

        assert _check(_pdf_with_image(image(jpeg((0, 0, 0, 180)))), k_only=True) == []
        assert _flagged(_check(_pdf_with_image(image(jpeg((0, 150, 220, 0)))), k_only=True))

    def test_icc_and_indexed_cmyk_images_are_never_k_only(self):
        icc = lambda p: _cmyk_image(p, b"\x00" * 16, space=_icc_space(p, FAKE_CMYK, 4))  # noqa: E731
        indexed = lambda p: _cmyk_image(  # noqa: E731
            p, b"\x00" * 4, space=pikepdf.Array([pikepdf.Name.Indexed, pikepdf.Name.DeviceCMYK, 0, b"\x00\x00\x00\x80"])
        )
        for make in (icc, indexed):
            [finding] = _check(_pdf_with_image(make), k_only=True)
            assert "not treated as K-only" in finding.message

    def test_an_image_is_decoded_once_however_often_it_is_placed(self, mocker):
        spy = mocker.spy(colour, "_cmy_problem")
        pdf = _pdf_with_image(lambda p: _cmyk_image(p, b"\x00\x00\x00\x80" * 4), b"q /Im1 Do Q q /Im1 Do Q q /Im1 Do Q")
        pdf.pages.append(pdf.pages[0])  # the same image on a second page
        assert _check(pdf, k_only=True) == []
        assert spy.call_count == 1

    def test_strict_mode_never_decodes(self, mocker):
        spy = mocker.spy(colour, "_cmy_problem")
        _check(_pdf_with_image(lambda p: _cmyk_image(p, b"\x00\x00\x00\x80" * 4)))
        assert spy.call_count == 0

    @pytest.mark.parametrize(
        ("space", "data", "strict", "allowed"),
        [
            (b"/G", b"\x80", False, False),
            (b"/RGB", b"\x00\x00\x00", True, True),
            (b"/CMYK", b"\x00\x00\x00\x80", True, True),  # inline CMYK is never decoded: cannot be confirmed
        ],
    )
    def test_inline_images(self, make_content_pdf, space, data, strict, allowed):
        stream = b"q 10 0 0 10 0 0 cm BI /W 1 /H 1 /CS " + space + b" /BPC 8 ID " + data + b" EI Q"
        with pikepdf.open(make_content_pdf(stream, font=False)) as pdf:
            assert _flagged(_check(pdf)) is strict
            findings = _check(pdf, k_only=True)
            assert _flagged(findings) is allowed
            if space == b"/CMYK":
                assert "inline" in findings[0].message

    @pytest.mark.parametrize(
        ("fill", "strict", "allowed"),
        [(b"0.5 g", False, False), (b"1 0 0 rg", True, True), (K_ONLY, True, False), (RICH, True, True)],
    )
    @pytest.mark.parametrize("kind", ["inline", "xobject"])
    def test_stencil_mask_is_judged_by_its_fill(self, fill, strict, allowed, kind):
        pdf = pikepdf.new()
        mask = pdf.make_stream(
            b"\x00",
            Type=pikepdf.Name.XObject,
            Subtype=pikepdf.Name.Image,
            Width=1,
            Height=1,
            ImageMask=True,
            BitsPerComponent=1,
        )
        inline = b"BI /W 1 /H 1 /IM true /BPC 1 ID \x00 EI"
        _page_with(
            pdf,
            fill + b" q 10 0 0 10 0 0 cm " + (inline if kind == "inline" else b"/Im1 Do") + b" Q",
            pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=pdf.make_indirect(mask))),
        )
        assert _flagged(_check(pdf)) is strict
        assert _flagged(_check(pdf, k_only=True)) is allowed

    def test_a_spot_image_is_a_warning(self, make_image):
        space = pikepdf.Array(
            [
                pikepdf.Name.Separation,
                pikepdf.Name("/Gold"),
                pikepdf.Name.DeviceGray,
                pikepdf.Dictionary(FunctionType=2, Domain=[0, 1], C0=[1], C1=[0], N=1),
            ]
        )
        pdf = _pdf_with_image(lambda p: p.make_indirect(make_image(p, colorspace=space)))
        [finding] = _check(pdf)
        assert finding.severity is Severity.WARN and "Gold" in finding.message


def _build_shading_pdf(space, stream=b"/Sh1 sh"):
    return _pdf_with_pattern(
        stream, lambda p: (pikepdf.Dictionary(Sh1=_shading(p, space)), pikepdf.Dictionary(), pikepdf.Dictionary())
    )


class TestShadingsAndPatterns:
    @pytest.mark.parametrize(
        ("space", "flagged"),
        [(pikepdf.Name.DeviceGray, False), (pikepdf.Name.DeviceRGB, True), (pikepdf.Name.DeviceCMYK, True)],
    )
    def test_sh_is_judged_by_its_colour_space(self, space, flagged):
        # a shading's colours come from a function, so CMYK is not confirmed K-only even when allowed
        assert _flagged(_check(_build_shading_pdf(space))) is flagged
        assert _flagged(_check(_build_shading_pdf(space), k_only=True)) is flagged

    @pytest.mark.parametrize(
        ("space", "flagged"),
        [(pikepdf.Name.DeviceGray, False), (pikepdf.Name.DeviceRGB, True), (pikepdf.Name.DeviceCMYK, True)],
    )
    def test_shading_pattern(self, space, flagged):
        def build(p):
            pattern = pikepdf.Dictionary(PatternType=2, Shading=_shading(p, space))
            return pikepdf.Dictionary(), pikepdf.Dictionary(P1=p.make_indirect(pattern)), pikepdf.Dictionary()

        assert _flagged(_check(_pdf_with_pattern(PATTERN, build), k_only=True)) is flagged

    @pytest.mark.parametrize(
        ("content", "flagged"),
        [(b"0.5 g 0 0 1 1 re f", False), (b"1 0 0 rg 0 0 1 1 re f", True), (K_ONLY + b" 0 0 1 1 re f", True)],
    )
    def test_tiling_pattern_is_judged_by_its_content(self, content, flagged):
        def build(p):
            return pikepdf.Dictionary(), pikepdf.Dictionary(P1=_tiling(p, content)), pikepdf.Dictionary()

        assert _flagged(_check(_pdf_with_pattern(PATTERN, build), k_only=True)) is flagged

    def test_a_missing_pattern_cannot_be_confirmed(self):
        pdf = _pdf_with_pattern(PATTERN, lambda p: (pikepdf.Dictionary(), pikepdf.Dictionary(), pikepdf.Dictionary()))
        assert _flagged(_check(pdf))


class TestOutputIntent:
    @staticmethod
    def _with_intent(components: int):
        pdf = pikepdf.new()
        _page_with(pdf, b"0.5 g 0 0 10 10 re f", pikepdf.Dictionary())
        intent = pikepdf.Dictionary(
            Type=pikepdf.Name.OutputIntent,
            S=pikepdf.Name.GTS_PDFX,
            DestOutputProfile=pdf.make_stream(b"x", N=components),
        )
        pdf.Root.OutputIntents = pikepdf.Array([pdf.make_indirect(intent)])
        return pdf

    @pytest.mark.parametrize(
        ("components", "strict", "allowed"), [(1, False, False), (3, True, True), (4, True, False)]
    )
    def test_the_intent_family(self, components, strict, allowed):
        pdf = self._with_intent(components)
        assert _flagged(_check(pdf)) is strict
        assert _flagged(_check(pdf, k_only=True)) is allowed

    def test_no_intent_is_fine(self, make_content_pdf):
        assert _paint(make_content_pdf, b"0.5 g 0 0 10 10 re f") == ([], [])


class TestGrayProfiles:
    def test_two_different_gray_profiles_are_an_error_in_a_gray_job_only(self):
        pdf = _pdf_with_profiles((build.gray_profile("One gray"), 1), (build.gray_profile("Two gray"), 1))
        [finding] = colour.check_icc_profiles(pdf, ColourMode.GRAY)
        assert finding.severity is Severity.ERROR and "mixes 2 gray profiles" in finding.message
        assert colour.check_icc_profiles(pdf, ColourMode.CMYK) == []

    def test_the_same_gray_profile_twice_is_one_profile(self):
        pdf = _pdf_with_profiles((build.gray_profile("One gray"), 1), (build.gray_profile("One gray"), 1))
        assert colour.check_icc_profiles(pdf, ColourMode.GRAY) == []

    def test_the_intent_counts_as_a_gray_profile(self):
        pdf = _pdf_with_profiles((build.gray_profile("One gray"), 1))
        intent = pikepdf.Dictionary(
            Type=pikepdf.Name.OutputIntent,
            S=pikepdf.Name.GTS_PDFX,
            DestOutputProfile=pdf.make_stream(build.gray_profile("Two gray"), N=1),
        )
        pdf.Root.OutputIntents = pikepdf.Array([pdf.make_indirect(intent)])
        assert colour.check_icc_profiles(pdf, ColourMode.GRAY)

    def test_rgb_mode_wording_is_unchanged(self):
        from .test_colour import _SRGB_NAME, _renamed, _srgb

        other = _renamed(_srgb(), _SRGB_NAME, "Odd RGB built".encode("utf-16-be"))
        [finding] = colour.check_icc_profiles(_pdf_with_profiles((_srgb(), 3), (other, 3)), ColourMode.RGB)
        assert finding.message.startswith("the RGB job mixes 2 RGB profiles")
