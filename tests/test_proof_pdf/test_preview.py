"""Rasterising proofs and drawing the page-box guides, through the Rust core's renderer."""

import pathlib
from typing import cast

import pytest
from PIL import Image

from proof_pdf import preview

from .conftest import BLEED, MEDIA_H, MEDIA_W, TRIM_H, TRIM_W


def _pixels(png: pathlib.Path) -> set[tuple[int, int, int]]:
    with Image.open(png) as img:
        return cast("set[tuple[int, int, int]]", set(img.convert("RGB").get_flattened_data()))


def _boxes(*, trim: bool = True, bleed: list[float] | None = None) -> dict:
    boxes: dict = {"MediaBox": [0, 0, MEDIA_W, MEDIA_H]}
    if trim:
        boxes["TrimBox"] = [BLEED, BLEED, BLEED + TRIM_W, BLEED + TRIM_H]
    if bleed is not None:
        boxes["BleedBox"] = bleed
    return boxes


class TestRenderPdf:
    def test_one_png_a_page_in_page_order(self, make_pdf, tmp_path):
        pngs = preview.render_pdf(make_pdf("deck.pdf", pages=3), tmp_path / "out", dpi=72, guides=False, safe_mm=None)
        assert [p.name for p in pngs] == ["deck-p1.png", "deck-p2.png", "deck-p3.png"]
        with Image.open(pngs[0]) as img:
            assert img.size == (round(MEDIA_W), round(MEDIA_H))

    def test_trim_bleed_and_safe_are_all_drawn(self, make_pdf, tmp_path):
        path = make_pdf(boxes=_boxes(bleed=[1, 1, MEDIA_W - 1, MEDIA_H - 1]))
        [png] = preview.render_pdf(path, tmp_path, dpi=72, guides=True, safe_mm=3.0)
        assert {preview._TRIM_RGB, preview._SAFE_RGB, preview._BLEED_RGB} <= _pixels(png)

    def test_bleed_equal_to_media_is_not_drawn(self, make_pdf, tmp_path):
        """No point outlining the sheet edge: it is already the image border."""
        [png] = preview.render_pdf(
            make_pdf(boxes=_boxes(bleed=[0, 0, MEDIA_W, MEDIA_H])), tmp_path, dpi=72, guides=True, safe_mm=None
        )
        assert preview._BLEED_RGB not in _pixels(png) and preview._TRIM_RGB in _pixels(png)

    def test_without_trim_only_bleed_is_drawn(self, make_pdf, tmp_path):
        path = make_pdf(boxes=_boxes(trim=False, bleed=[10, 10, 100, 100]))
        [png] = preview.render_pdf(path, tmp_path, dpi=72, guides=True, safe_mm=3.0)
        assert preview._BLEED_RGB in _pixels(png)
        assert preview._TRIM_RGB not in _pixels(png) and preview._SAFE_RGB not in _pixels(png)

    def test_no_guides_leaves_the_raster_plain(self, make_pdf, tmp_path):
        [png] = preview.render_pdf(make_pdf(), tmp_path, dpi=72, guides=False, safe_mm=3.0)
        assert _pixels(png) == {(255, 255, 255)}

    def test_the_guides_show_where_a_wrong_trimbox_actually_is(self, make_pdf, tmp_path):
        """The visual counterpart of the geometry finding: a trim guide inset 25pt."""
        boxes = {
            "MediaBox": [0, 0, 337.32198, 459.21146],
            "TrimBox": [25, 20, 312.32198, 439.21146],
            "BleedBox": [0, 0, 337.32198, 459.21146],
        }
        [png] = preview.render_pdf(make_pdf(boxes=boxes), tmp_path, dpi=72, guides=True, safe_mm=3.0)
        with Image.open(png) as img:
            flat = img.convert("RGB").get_flattened_data()
            row = flat[img.width * (img.height // 2) : img.width * (img.height // 2 + 1)]
        # 25pt at 72dpi is 25px from the left edge, not the 7mm (~20px) it should be.
        assert next(x for x, px in enumerate(row) if px == preview._TRIM_RGB) == 25

    def test_a_file_that_cannot_be_read_raises(self, tmp_path):
        bad = tmp_path / "bad.pdf"
        bad.write_bytes(b"not a pdf")
        with pytest.raises(preview.UnreadablePdfError):
            preview.render_pdf(bad, tmp_path / "out", dpi=72, guides=True, safe_mm=None)
        assert not (tmp_path / "out").exists()

    def test_a_page_too_large_for_the_resolution_raises(self, make_pdf, tmp_path):
        with pytest.raises(ValueError, match="65,535 pixels"):
            preview.render_pdf(make_pdf(), tmp_path, dpi=100_000, guides=False, safe_mm=None)

    async def test_the_async_wrapper_matches(self, make_pdf, tmp_path):
        pngs = await preview.preview_file(make_pdf(), tmp_path, dpi=72, guides=True, safe_mm=None)
        assert [p.name for p in pngs] == ["test-p1.png"]
