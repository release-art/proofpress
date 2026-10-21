"""Ink coverage, overprint, colour space and rich-black checks."""

import pathlib
import shutil

import pikepdf
import pytest
from PIL import Image

from proof_pdf import colour, proc
from proof_pdf.models import Severity


def _checks(findings) -> set[str]:
    return {f.check for f in findings}


def _page_with(pdf: pikepdf.Pdf, stream: bytes, resources: pikepdf.Dictionary) -> None:
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page,
        MediaBox=[0, 0, 500, 500],
        Contents=pdf.make_stream(stream),
        Resources=resources,
    )
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))


class TestTotalAreaCoverage:
    async def test_peak_over_limit_is_an_error(self, mocker):
        mocker.patch.object(colour, "_page_tac_peaks", return_value=[250.0, 340.0])
        findings = await colour.check_tac("gs", "x.pdf", 300.0)
        assert [f.page for f in findings] == [2]
        assert findings[0].severity is Severity.ERROR

    async def test_all_under_limit_is_clean(self, mocker):
        mocker.patch.object(colour, "_page_tac_peaks", return_value=[250.0, 299.9])
        assert await colour.check_tac("gs", "x.pdf", 300.0) == []

    def test_peak_tac_reads_a_real_cmyk_raster(self, tmp_path):
        tif = tmp_path / "page.tif"
        Image.new("CMYK", (4, 4), (255, 255, 0, 0)).save(tif)
        assert colour._peak_tac(tif) == pytest.approx(200.0)

    async def test_rasters_are_deleted_as_they_are_read(self, tmp_path, fake_run):
        """A 50-page deck must not keep every TIFF on disk at once."""
        seen: list[int] = []

        def _write_pages(argv, **kwargs):
            out = next(a for a in argv if a.startswith("-sOutputFile=")).removeprefix("-sOutputFile=")
            for i in (1, 2, 3):
                Image.new("CMYK", (2, 2), (0, 0, 0, 0)).save(out % i)
            seen.append(len(list(pathlib.Path(out).parent.glob("*.tif"))))

        fake_run(side_effect=_write_pages)
        peaks = await colour._page_tac_peaks("gs", str(tmp_path / "in.pdf"))
        assert len(peaks) == 3
        assert seen == [3]  # all written by gs...
        # ...and none left behind once each peak was read.
        assert not list(tmp_path.glob("**/page-*.tif"))

    async def test_pages_sort_numerically_not_lexicographically(self, tmp_path, fake_run):
        """`page-1000.tif` must not sort before `page-999.tif`."""

        def _write(argv, **kwargs):
            out = next(a for a in argv if a.startswith("-sOutputFile=")).removeprefix("-sOutputFile=")
            base = pathlib.Path(out).parent
            for i, ink in ((999, 0), (1000, 255)):
                Image.new("CMYK", (2, 2), (0, 0, 0, ink)).save(base / f"page-{i}.tif")

        fake_run(side_effect=_write)
        peaks = await colour._page_tac_peaks("gs", "in.pdf")
        assert peaks == [pytest.approx(0.0), pytest.approx(100.0)]

    @pytest.mark.parametrize(
        ("stderr", "expected"), [("boom", "boom"), ("", "ghostscript failed")], ids=["detail", "default"]
    )
    async def test_ghostscript_failure_raises(self, fake_run, stderr, expected):
        fake_run(returncode=1, stderr=stderr)
        with pytest.raises(colour.GhostscriptError, match=expected):
            await colour._page_tac_peaks("gs", "in.pdf")

    async def test_timeout_raises(self, mocker):
        mocker.patch.object(proc, "run", side_effect=proc.ProcTimeoutError("timed out"))
        with pytest.raises(colour.GhostscriptError, match="timed out"):
            await colour._page_tac_peaks("gs", "in.pdf", timeout=5)

    def test_gs_available(self, mocker):
        mocker.patch.object(colour.shutil, "which", side_effect=lambda b: "/usr/bin/gs" if b == "gs" else None)
        assert colour.gs_available("gs")
        assert not colour.gs_available("nope")


class TestOverprint:
    GS = {"/GS1": pikepdf.Dictionary(OP=True, op=True)}
    FILL = b"0 0 10 10 re f"

    def _findings(self, make_content_pdf, stream, **kwargs):
        with pikepdf.open(make_content_pdf(stream, extgstates=self.GS, **kwargs)) as pdf:
            return colour.check_overprint(pdf)

    def test_painting_under_overprint_is_flagged(self, make_content_pdf):
        findings = self._findings(make_content_pdf, b"/GS1 gs " + self.FILL, font=False)
        assert [f.severity for f in findings] == [Severity.WARN]

    def test_applied_but_never_painted_under_is_not_flagged(self, make_content_pdf):
        """Overprint with nothing painted while it is on changes nothing on press."""
        assert self._findings(make_content_pdf, b"/GS1 gs", font=False) == []

    def test_declared_but_never_applied_is_not_flagged(self, make_content_pdf):
        """An ExtGState nobody invokes changes nothing on press."""
        assert self._findings(make_content_pdf, self.FILL, font=False) == []

    def test_overprint_off_is_clean(self, make_content_pdf):
        off = {"/GS1": pikepdf.Dictionary(OP=False)}
        with pikepdf.open(make_content_pdf(b"/GS1 gs " + self.FILL, font=False, extgstates=off)) as pdf:
            assert colour.check_overprint(pdf) == []

    @pytest.mark.parametrize(
        "text",
        [
            pytest.param(b"0 0 0 1 k", id="cmyk-100k"),
            pytest.param(b"0 g", id="gray-black"),
        ],
    )
    def test_overprinting_100_percent_k_text_is_fine(self, make_content_pdf, text):
        assert self._findings(make_content_pdf, b"/GS1 gs BT /F1 8 Tf " + text + b" (x) Tj ET") == []

    @pytest.mark.parametrize(
        "text",
        [
            pytest.param(b"0 0 0 0.6 k", id="k-tint"),
            pytest.param(b"0.6 0.5 0.4 0.9 k", id="rich-black"),
            pytest.param(b"1 0 0 0 k", id="colour"),
        ],
    )
    def test_overprinting_other_text_is_flagged(self, make_content_pdf, text):
        findings = self._findings(make_content_pdf, b"/GS1 gs BT /F1 8 Tf " + text + b" (x) Tj ET")
        assert _checks(findings) == {"overprint"}

    def test_a_q_restores_overprint(self, make_content_pdf):
        assert self._findings(make_content_pdf, b"q /GS1 gs Q " + self.FILL, font=False) == []

    def test_overprint_inside_a_form_is_found(self, tmp_path, make_form):
        """The page-only scan this replaces could not see nested content."""
        path = tmp_path / "nested.pdf"
        pdf = pikepdf.new()
        form = pdf.make_indirect(
            make_form(
                pdf,
                b"/GS1 gs " + self.FILL,
                resources=pikepdf.Dictionary(ExtGState=pikepdf.Dictionary(GS1=pikepdf.Dictionary(OP=True))),
            )
        )
        _page_with(pdf, b"/Fm1 Do", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert _checks(colour.check_overprint(opened)) == {"overprint"}

    def test_overprint_set_in_a_form_does_not_leak_out_of_it(self, tmp_path, make_form):
        path = tmp_path / "leak.pdf"
        pdf = pikepdf.new()
        form = pdf.make_indirect(
            make_form(
                pdf,
                b"/GS1 gs",
                resources=pikepdf.Dictionary(ExtGState=pikepdf.Dictionary(GS1=pikepdf.Dictionary(OP=True))),
            )
        )
        _page_with(pdf, b"/Fm1 Do " + self.FILL, pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert colour.check_overprint(opened) == []


class TestColourSpaces:
    @pytest.mark.parametrize(
        ("space", "expected"),
        [
            pytest.param(pikepdf.Name.DeviceRGB, "rgb", id="device-rgb"),
            pytest.param(pikepdf.Name.DeviceCMYK, "cmyk", id="device-cmyk"),
            pytest.param(pikepdf.Name.DeviceGray, "gray", id="device-gray"),
            pytest.param(pikepdf.Name.CalRGB, "rgb", id="cal-rgb"),
        ],
    )
    def test_device_spaces(self, space, expected):
        assert colour.classify_colour_space(space, None) == expected

    @pytest.mark.parametrize(("components", "expected"), [(1, "gray"), (3, "rgb"), (4, "cmyk"), (7, "other")])
    def test_icc_based_classified_by_component_count(self, components, expected):
        pdf = pikepdf.new()
        icc = pdf.make_stream(b"")
        icc["/N"] = components
        space = pikepdf.Array([pikepdf.Name("/ICCBased"), pdf.make_indirect(icc)])
        assert colour.classify_colour_space(space, None) == expected

    @pytest.mark.parametrize(("signature", "expected"), [(b"RGB ", "rgb"), (b"Lab ", "other"), (b"CMYK", "cmyk")])
    def test_icc_based_trusts_the_profile_header_over_the_component_count(self, signature, expected):
        """A Lab profile has three components too (the Altona suite has them); it is not RGB."""
        pdf = pikepdf.new()
        icc = pdf.make_stream(bytes(16) + signature + bytes(8))
        icc["/N"] = 4 if signature == b"CMYK" else 3
        space = pikepdf.Array([pikepdf.Name("/ICCBased"), pdf.make_indirect(icc)])
        assert colour.classify_colour_space(space, None) == expected

    def test_separation_alternate_is_not_the_artwork_space(self):
        """A spot colour whose alternate is RGB is a correct print construct.

        This is the exact shape in www.release.art's business card (its /&Scodix
        spot ink); classifying the alternate would fail a known-good file.
        """
        space = pikepdf.Array(
            [pikepdf.Name("/Separation"), pikepdf.Name("/&Scodix"), pikepdf.Name.DeviceRGB, pikepdf.Dictionary()]
        )
        assert colour.classify_colour_space(space, None) == "other"

    def test_devicen_alternate_is_also_ignored(self):
        space = pikepdf.Array([pikepdf.Name("/DeviceN"), pikepdf.Array([]), pikepdf.Name.DeviceRGB])
        assert colour.classify_colour_space(space, None) == "other"

    def test_indexed_looks_through_to_its_base(self):
        space = pikepdf.Array([pikepdf.Name("/Indexed"), pikepdf.Name.DeviceRGB, 255, b"\x00"])
        assert colour.classify_colour_space(space, None) == "rgb"

    def test_named_space_resolves_through_resources(self, make_content_pdf):
        path = make_content_pdf(b"", font=False, colorspaces={"/CS0": pikepdf.Name.DeviceRGB})
        with pikepdf.open(path) as pdf:
            resources = pdf.pages[0].obj.get("/Resources")
            assert colour.classify_colour_space(pikepdf.Name("/CS0"), resources) == "rgb"

    def test_unknown_space_is_other(self):
        assert colour.classify_colour_space(None, None) == "other"
        assert colour.classify_colour_space(pikepdf.Name("/Nonsense"), None) == "other"

    def test_rgb_image_is_an_error(self, tmp_path, make_image):
        path = tmp_path / "rgb.pdf"
        pdf = pikepdf.new()
        image = pdf.make_indirect(make_image(pdf, colorspace=pikepdf.Name.DeviceRGB))
        _page_with(pdf, b"q 100 0 0 100 0 0 cm /Im1 Do Q", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=image)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            findings = colour.check_colour_spaces(opened, strict=False)
        assert [f.severity for f in findings] == [Severity.ERROR]
        assert "/Im1" in findings[0].message

    def test_cmyk_image_is_clean(self, tmp_path, make_image):
        path = tmp_path / "cmyk.pdf"
        pdf = pikepdf.new()
        image = pdf.make_indirect(make_image(pdf, colorspace=pikepdf.Name.DeviceCMYK))
        _page_with(pdf, b"q 100 0 0 100 0 0 cm /Im1 Do Q", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=image)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert colour.check_colour_spaces(opened, strict=False) == []

    @pytest.mark.parametrize(
        ("strict", "expected"), [(False, Severity.WARN), (True, Severity.ERROR)], ids=["advisory", "strict"]
    )
    def test_vector_rgb_severity_follows_strict(self, make_content_pdf, strict, expected):
        path = make_content_pdf(b"1 0 0 rg 0 0 10 10 re f", font=False)
        with pikepdf.open(path) as pdf:
            findings = colour.check_colour_spaces(pdf, strict=strict)
        assert [f.severity for f in findings] == [expected]

    def test_repeated_image_reports_once_per_page(self, tmp_path, make_image):
        path = tmp_path / "repeat.pdf"
        pdf = pikepdf.new()
        image = pdf.make_indirect(make_image(pdf, colorspace=pikepdf.Name.DeviceRGB))
        stream = b"q 100 0 0 100 0 0 cm /Im1 Do Q " * 5
        _page_with(pdf, stream, pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im1=image)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert len(colour.check_colour_spaces(opened, strict=False)) == 1


class TestRichBlack:
    def test_small_rich_black_text_is_flagged(self, make_content_pdf):
        path = make_content_pdf(b"BT /F1 8 Tf 0.6 0.5 0.4 0.9 k (hello) Tj ET")
        with pikepdf.open(path) as pdf:
            findings = colour.check_rich_black(pdf, 12.0)
        assert [f.severity for f in findings] == [Severity.WARN]
        assert "8.0pt" in findings[0].message

    @pytest.mark.parametrize(
        "stream",
        [
            pytest.param(b"BT /F1 8 Tf 0 0 0 1 k (x) Tj ET", id="pure-process-black"),
            pytest.param(b"BT /F1 24 Tf 0.6 0.5 0.4 0.9 k (x) Tj ET", id="large-text"),
            pytest.param(b"BT /F1 8 Tf 0.6 0.5 0.4 0.1 k (x) Tj ET", id="light-k-not-black"),
            pytest.param(b"BT /F1 8 Tf 0 g (x) Tj ET", id="grayscale"),
            pytest.param(b"BT /F1 8 Tf 0 0 0 rg (x) Tj ET", id="rgb"),
        ],
    )
    def test_clean_cases(self, make_content_pdf, stream):
        with pikepdf.open(make_content_pdf(stream)) as pdf:
            assert colour.check_rich_black(pdf, 12.0) == []

    def test_a_q_restores_the_fill_before_the_text(self, make_content_pdf):
        """The fill set inside q...Q is gone by the time the text is shown."""
        stream = b"q 0.6 0.5 0.4 0.9 k Q BT /F1 8 Tf (x) Tj ET"
        with pikepdf.open(make_content_pdf(stream)) as pdf:
            assert colour.check_rich_black(pdf, 12.0) == []

    def test_text_matrix_scaling_counts_towards_the_size(self, make_content_pdf):
        """4pt text scaled 4x is 16pt on the page, and must not be flagged."""
        path = make_content_pdf(b"BT 4 0 0 4 0 0 Tm /F1 4 Tf 0.6 0.5 0.4 0.9 k (x) Tj ET")
        with pikepdf.open(path) as pdf:
            assert colour.check_rich_black(pdf, 12.0) == []

    def test_ctm_scaling_counts_too(self, make_content_pdf):
        path = make_content_pdf(b"0.25 0 0 0.25 0 0 cm BT /F1 16 Tf 0.6 0.5 0.4 0.9 k (x) Tj ET")
        with pikepdf.open(path) as pdf:
            findings = colour.check_rich_black(pdf, 12.0)
        assert findings and "4.0pt" in findings[0].message

    def test_smallest_run_on_the_page_is_reported(self, make_content_pdf):
        stream = b"BT 0.6 0.5 0.4 0.9 k /F1 10 Tf (a) Tj /F1 5 Tf (b) Tj ET"
        with pikepdf.open(make_content_pdf(stream)) as pdf:
            findings = colour.check_rich_black(pdf, 12.0)
        assert "5.0pt" in findings[0].message

    def test_rich_black_inside_a_form_is_found(self, tmp_path, make_form):
        path = tmp_path / "nested.pdf"
        pdf = pikepdf.new()
        font = pdf.make_indirect(
            pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name("/Helvetica"))
        )
        form = pdf.make_indirect(
            make_form(
                pdf,
                b"BT /F1 6 Tf 0.6 0.5 0.4 0.9 k (x) Tj ET",
                resources=pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font)),
            )
        )
        _page_with(pdf, b"/Fm1 Do", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            findings = colour.check_rich_black(opened, 12.0)
        assert findings and "6.0pt" in findings[0].message


class TestKOnlyText:
    def _findings(self, make_content_pdf, stream, **kwargs):
        with pikepdf.open(make_content_pdf(stream, **kwargs)) as pdf:
            return colour.check_k_only_text(pdf)

    @pytest.mark.parametrize(
        "stream",
        [
            pytest.param(b"BT /F1 8 Tf 0 0 0 1 k (x) Tj ET", id="process-black"),
            pytest.param(b"BT /F1 8 Tf 0 0 0 0.4 k (x) Tj ET", id="k-tint"),
            pytest.param(b"BT /F1 8 Tf 0 0 0 0 k (x) Tj ET", id="white"),
            pytest.param(b"BT /F1 8 Tf 0 g (x) Tj ET", id="gray-black"),
            pytest.param(b"BT /F1 8 Tf 0.4 g (x) Tj ET", id="gray-tint"),
            pytest.param(b"BT /F1 8 Tf (x) Tj ET", id="no-colour-set"),
            pytest.param(b"/DeviceCMYK cs 0 0 0 1 scn BT /F1 8 Tf (x) Tj ET", id="scn-k-only"),
        ],
    )
    def test_k_only_text_is_clean(self, make_content_pdf, stream):
        assert self._findings(make_content_pdf, stream) == []

    def test_a_four_colour_build_is_an_error_naming_the_ink(self, make_content_pdf):
        findings = self._findings(make_content_pdf, b"BT /F1 8 Tf 1 0.5 0 0.5 k (x) Tj ET")
        assert [f.severity for f in findings] == [Severity.ERROR]
        assert "C100 M50 Y0 K50" in findings[0].message

    def test_rich_black_is_not_k_only(self, make_content_pdf):
        assert _checks(self._findings(make_content_pdf, b"BT /F1 8 Tf 0.6 0.5 0.4 0.9 k (x) Tj ET")) == {"k-only-text"}

    def test_rgb_text_is_flagged(self, make_content_pdf):
        findings = self._findings(make_content_pdf, b"BT /F1 8 Tf 0 0 0 rg (x) Tj ET")
        assert "RGB" in findings[0].message

    def test_scn_in_a_colour_family_is_read(self, make_content_pdf):
        findings = self._findings(make_content_pdf, b"/DeviceCMYK cs 1 0 0 0 scn BT /F1 8 Tf (x) Tj ET")
        assert "C100" in findings[0].message

    def test_a_spot_colour_is_flagged(self, make_content_pdf):
        spot = pikepdf.Array([pikepdf.Name.Separation, pikepdf.Name("/Spot"), pikepdf.Name.DeviceCMYK])
        findings = self._findings(make_content_pdf, b"/CS1 cs 1 scn BT /F1 8 Tf (x) Tj ET", colorspaces={"/CS1": spot})
        assert "spot" in findings[0].message

    def test_a_pattern_fill_is_flagged(self, make_content_pdf):
        findings = self._findings(make_content_pdf, b"/Pattern cs /P1 scn BT /F1 8 Tf (x) Tj ET")
        assert _checks(findings) == {"k-only-text"}

    def test_a_q_restores_the_fill_before_the_text(self, make_content_pdf):
        stream = b"0 0 0 1 k q 1 0 0 0 k Q BT /F1 8 Tf (x) Tj ET"
        assert self._findings(make_content_pdf, stream) == []

    def test_only_text_is_judged_not_graphics(self, make_content_pdf):
        assert self._findings(make_content_pdf, b"1 0 0 0 k 0 0 10 10 re f") == []

    def test_one_finding_per_page_counts_the_runs(self, make_content_pdf):
        stream = b"BT /F1 8 Tf 1 0 0 0 k (a) Tj (b) Tj (c) Tj ET"
        findings = self._findings(make_content_pdf, stream)
        assert len(findings) == 1 and "and 2 more runs" in findings[0].message

    def test_a_form_runs_in_its_own_state(self, tmp_path, make_form):
        """Cyan set inside a form does not colour the black text the page shows afterwards."""
        path = tmp_path / "form.pdf"
        pdf = pikepdf.new()
        font = pdf.make_indirect(
            pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name("/Helvetica"))
        )
        resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font))
        form = pdf.make_indirect(make_form(pdf, b"1 0 0 0 k", resources=resources))
        page_resources = pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form), Font=pikepdf.Dictionary(F1=font))
        _page_with(pdf, b"/Fm1 Do BT /F1 8 Tf (x) Tj ET", page_resources)
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert colour.check_k_only_text(opened) == []

    def test_cyan_text_inside_a_form_is_found(self, tmp_path, make_form):
        path = tmp_path / "form.pdf"
        pdf = pikepdf.new()
        font = pdf.make_indirect(
            pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name("/Helvetica"))
        )
        form = pdf.make_indirect(
            make_form(
                pdf,
                b"BT /F1 8 Tf 1 0 0 0 k (x) Tj ET",
                resources=pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font)),
            )
        )
        _page_with(pdf, b"/Fm1 Do", pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form)))
        pdf.save(path)
        with pikepdf.open(path) as opened:
            assert _checks(colour.check_k_only_text(opened)) == {"k-only-text"}


@pytest.mark.skipif(shutil.which("gs") is None, reason="Ghostscript not installed")
async def test_page_tac_peaks_against_real_ghostscript(make_content_pdf):
    """One end-to-end pass through the real binary, on a deliberately tiny file."""
    path = make_content_pdf(b"0 0 0 0 k 0 0 50 50 re f", font=False)
    peaks = await colour._page_tac_peaks("gs", str(path), timeout=10)
    assert len(peaks) == 1
    assert 0.0 <= peaks[0] <= 400.0


GS_OP = {"/GS1": pikepdf.Dictionary(OP=True, op=True)}


class TestWhiteOverprint:
    @pytest.mark.parametrize(
        "white",
        [
            pytest.param(b"1 g", id="gray"),
            pytest.param(b"1 1 1 rg", id="rgb"),
            pytest.param(b"0 0 0 0 k", id="cmyk"),
        ],
    )
    def test_white_text_set_to_overprint_is_an_error(self, make_content_pdf, white):
        path = make_content_pdf(b"/GS1 gs BT /F1 12 Tf " + white + b" (x) Tj ET", extgstates=GS_OP)
        with pikepdf.open(path) as pdf:
            [finding] = colour.check_white_overprint(pdf)
        assert finding.severity is Severity.ERROR and finding.check == "white-overprint" and finding.page == 1
        assert "knock out" in finding.message

    def test_a_white_shape_under_overprint_is_flagged_too(self, make_content_pdf):
        path = make_content_pdf(b"/GS1 gs 1 g 0 0 10 10 re f", font=False, extgstates=GS_OP)
        with pikepdf.open(path) as pdf:
            assert _checks(colour.check_white_overprint(pdf)) == {"white-overprint"}

    def test_white_that_knocks_out_is_fine(self, make_content_pdf):
        path = make_content_pdf(b"BT /F1 12 Tf 1 g (x) Tj ET", extgstates=GS_OP)
        with pikepdf.open(path) as pdf:
            assert colour.check_white_overprint(pdf) == []

    def test_overprinting_black_is_not_white(self, make_content_pdf):
        path = make_content_pdf(b"/GS1 gs BT /F1 12 Tf 0 g (x) Tj ET", extgstates=GS_OP)
        with pikepdf.open(path) as pdf:
            assert colour.check_white_overprint(pdf) == []

    def test_a_q_restores_a_white_fill(self, make_content_pdf):
        path = make_content_pdf(b"/GS1 gs q 1 g Q BT /F1 12 Tf (x) Tj ET", extgstates=GS_OP)
        with pikepdf.open(path) as pdf:
            assert colour.check_white_overprint(pdf) == []


class TestSpotInks:
    SEP = {"/CS1": pikepdf.Array([pikepdf.Name.Separation, pikepdf.Name("/PANTONE 185 C"), pikepdf.Name.DeviceCMYK])}
    ALL = {"/CS1": pikepdf.Array([pikepdf.Name.Separation, pikepdf.Name.All, pikepdf.Name.DeviceCMYK])}
    STREAM = b"/CS1 cs 1 scn 0 0 5 5 re f"

    def _findings(self, make_content_pdf, spaces, allowed):
        with pikepdf.open(make_content_pdf(self.STREAM, font=False, colorspaces=spaces)) as pdf:
            return colour.check_spot_inks(pdf, allowed)

    def test_a_spot_not_on_the_list_is_an_error_naming_it(self, make_content_pdf):
        [finding] = self._findings(make_content_pdf, self.SEP, ["Dieline"])
        assert finding.check == "spot-ink" and finding.severity is Severity.ERROR
        assert "PANTONE 185 C" in finding.message and finding.page == 1

    def test_a_listed_spot_passes_and_names_match_exactly(self, make_content_pdf):
        assert self._findings(make_content_pdf, self.SEP, ["PANTONE 185 C"]) == []
        assert len(self._findings(make_content_pdf, self.SEP, ["pantone 185 c"])) == 1

    def test_the_registration_colour_is_called_out(self, make_content_pdf):
        [finding] = self._findings(make_content_pdf, self.ALL, [])
        assert "registration colour" in finding.message
        assert self._findings(make_content_pdf, self.ALL, ["All"]) == []

    def test_no_spots_means_nothing_to_report(self, make_content_pdf):
        with pikepdf.open(make_content_pdf(b"0 g 0 0 5 5 re f", font=False)) as pdf:
            assert colour.check_spot_inks(pdf, []) == []


def _transparent(tmp_path, *, group_cs=None, intent_n=None, extgstate=None, stream=b"/GS1 gs 0.5 g 0 0 50 50 re f"):
    pdf = pikepdf.new()
    gs = {"/GS1": pikepdf.Dictionary(ca=0.5)} if extgstate is None else extgstate
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page,
        MediaBox=[0, 0, 100, 100],
        Contents=pdf.make_stream(stream),
        Resources=pikepdf.Dictionary(ExtGState=pikepdf.Dictionary(gs)),
    )
    if group_cs is not None:
        page["/Group"] = pikepdf.Dictionary(S=pikepdf.Name.Transparency, CS=pikepdf.Name(group_cs))
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
    if intent_n is not None:
        profile = pdf.make_stream(b"x")
        profile["/N"] = intent_n
        pdf.Root["/OutputIntents"] = pikepdf.Array([pikepdf.Dictionary(DestOutputProfile=pdf.make_indirect(profile))])
    path = tmp_path / "t.pdf"
    pdf.save(path)
    return pikepdf.open(path)


class TestTransparency:
    def test_a_blend_space_that_disagrees_with_the_output_intent_is_an_error(self, tmp_path):
        with _transparent(tmp_path, group_cs="/DeviceRGB", intent_n=4) as pdf:
            [finding] = colour.check_transparency(pdf)
        assert finding.severity is Severity.ERROR and finding.check == "blend-space"
        assert "RGB" in finding.message and "CMYK" in finding.message and "opacity" in finding.message

    def test_a_matching_blend_space_is_clean(self, tmp_path):
        with _transparent(tmp_path, group_cs="/DeviceCMYK", intent_n=4) as pdf:
            assert colour.check_transparency(pdf) == []

    def test_an_undeclared_blend_space_is_a_warning(self, tmp_path):
        with _transparent(tmp_path, intent_n=4) as pdf:
            [finding] = colour.check_transparency(pdf)
        assert finding.severity is Severity.WARN and "declares no blend colour space" in finding.message

    def test_without_transparency_there_is_nothing_to_say(self, tmp_path):
        with _transparent(tmp_path, extgstate={"/GS1": pikepdf.Dictionary(ca=1.0)}, intent_n=4) as pdf:
            assert colour.check_transparency(pdf) == []

    @pytest.mark.parametrize(
        ("extgstate", "reason"),
        [
            pytest.param({"/GS1": pikepdf.Dictionary(CA=0.3)}, "opacity", id="stroke-opacity"),
            pytest.param({"/GS1": pikepdf.Dictionary(BM=pikepdf.Name.Multiply)}, "blend mode", id="blend-mode"),
            pytest.param({"/GS1": pikepdf.Dictionary(SMask=pikepdf.Dictionary())}, "soft mask", id="soft-mask"),
        ],
    )
    def test_each_kind_of_transparency_is_recognised(self, tmp_path, extgstate, reason):
        with _transparent(tmp_path, group_cs="/DeviceRGB", intent_n=4, extgstate=extgstate) as pdf:
            [finding] = colour.check_transparency(pdf)
        assert reason in finding.message

    @pytest.mark.parametrize("mode", ["/Normal", "/Compatible"])
    def test_a_normal_blend_mode_is_not_transparency(self, tmp_path, mode):
        gs = {"/GS1": pikepdf.Dictionary(BM=pikepdf.Name(mode), SMask=pikepdf.Name("/None"))}
        with _transparent(tmp_path, group_cs="/DeviceRGB", intent_n=4, extgstate=gs) as pdf:
            assert colour.check_transparency(pdf) == []

    def test_no_output_intent_means_no_mismatch_to_report(self, tmp_path):
        with _transparent(tmp_path, group_cs="/DeviceRGB") as pdf:
            assert colour.check_transparency(pdf) == []
