"""Rasterising proofs and drawing the page-box guides."""

import pathlib
import subprocess
from typing import cast

import pikepdf
import pytest
from PIL import Image

from proof_pdf import preview

from .conftest import BLEED, MEDIA_H, MEDIA_W, TRIM_H, TRIM_W


def _white_png(path: pathlib.Path, size=(72, 72)) -> pathlib.Path:
    Image.new("RGB", size, (255, 255, 255)).save(path)
    return path


def _pixels(png: pathlib.Path) -> set[tuple[int, int, int]]:
    with Image.open(png) as img:
        return cast("set[tuple[int, int, int]]", set(img.convert("RGB").get_flattened_data()))


class TestPageBoxes:
    def test_every_box_is_read(self, make_pdf):
        with pikepdf.open(make_pdf()) as pdf:
            boxes = preview.page_boxes(pdf)
        assert len(boxes) == 1
        assert boxes[0].trim is not None and boxes[0].bleed is not None
        assert boxes[0].media == (0, 0, pytest.approx(MEDIA_W), pytest.approx(MEDIA_H))

    def test_optional_boxes_are_none_when_absent(self, make_pdf):
        with pikepdf.open(make_pdf(boxes={"MediaBox": [0, 0, MEDIA_W, MEDIA_H]})) as pdf:
            boxes = preview.page_boxes(pdf)
        assert boxes[0].trim is None and boxes[0].bleed is None


class TestRenderPages:
    def test_pages_come_back_in_numeric_order(self, tmp_path, fake_run):
        def _write(argv, **kwargs):
            out = next(a for a in argv if a.startswith("-sOutputFile=")).removeprefix("-sOutputFile=")
            for i in (1, 2, 10):
                _white_png(pathlib.Path(out.replace("%d", str(i))))

        calls = fake_run(preview, side_effect=_write)
        pngs = preview._render_pages("gs", tmp_path / "deck.pdf", tmp_path / "out", 150)
        assert [p.name for p in pngs] == ["deck-p1.png", "deck-p2.png", "deck-p10.png"]
        assert "-r150" in calls[0]

    @pytest.mark.parametrize(
        ("stderr", "expected"), [("bad page", "bad page"), ("", "ghostscript failed")], ids=["detail", "default"]
    )
    def test_failure_raises(self, tmp_path, fake_run, stderr, expected):
        fake_run(preview, returncode=1, stderr=stderr)
        with pytest.raises(preview.GhostscriptError, match=expected):
            preview._render_pages("gs", tmp_path / "a.pdf", tmp_path / "out", 150)

    def test_timeout_raises(self, tmp_path, monkeypatch):
        def _timeout(*a, **k):
            raise subprocess.TimeoutExpired(cmd="gs", timeout=5)

        monkeypatch.setattr(preview.subprocess, "run", _timeout)
        with pytest.raises(preview.GhostscriptError, match="timed out"):
            preview._render_pages("gs", tmp_path / "a.pdf", tmp_path / "out", 150, timeout=5)


class TestGuideOverlay:
    def _boxes(self, *, media=True, trim=True, bleed=None) -> preview.PageBoxes:
        return preview.PageBoxes(
            media=(0, 0, MEDIA_W, MEDIA_H) if media else None,
            trim=(BLEED, BLEED, BLEED + TRIM_W, BLEED + TRIM_H) if trim else None,
            bleed=bleed,
        )

    def test_trim_bleed_and_safe_are_all_drawn(self, tmp_path):
        png = _white_png(tmp_path / "p.png", size=(300, 400))
        preview._overlay_guides(png, self._boxes(bleed=(0, 0, MEDIA_W, MEDIA_H - 1)), 72, 3.0)
        colours = _pixels(png)
        assert preview._TRIM_RGB in colours
        assert preview._SAFE_RGB in colours
        assert preview._BLEED_RGB in colours

    def test_bleed_equal_to_media_is_not_drawn(self, tmp_path):
        """No point outlining the sheet edge -- it is already the image border."""
        png = _white_png(tmp_path / "p.png", size=(300, 400))
        preview._overlay_guides(png, self._boxes(bleed=(0, 0, MEDIA_W, MEDIA_H)), 72, 3.0)
        assert preview._BLEED_RGB not in _pixels(png)

    def test_without_trim_only_bleed_is_drawn(self, tmp_path):
        png = _white_png(tmp_path / "p.png", size=(300, 400))
        preview._overlay_guides(png, self._boxes(trim=False, bleed=(10, 10, 100, 100)), 72, 3.0)
        colours = _pixels(png)
        assert preview._BLEED_RGB in colours
        assert preview._TRIM_RGB not in colours

    def test_without_media_it_is_a_no_op(self, tmp_path):
        """Points cannot be mapped to pixels without the media origin."""
        png = _white_png(tmp_path / "p.png")
        before = _pixels(png)
        preview._overlay_guides(png, self._boxes(media=False), 72, 3.0)
        assert _pixels(png) == before


class TestPreviewFile:
    def _fake_render(self, monkeypatch, tmp_path, count=1):
        def _render(gs_bin, pdf_path, out_dir, dpi, *, timeout=None):
            out_dir.mkdir(parents=True, exist_ok=True)
            return [_white_png(out_dir / f"{pdf_path.stem}-p{i}.png", size=(340, 462)) for i in range(1, count + 1)]

        monkeypatch.setattr(preview, "_render_pages", _render)

    def test_guides_are_overlaid_when_asked(self, make_pdf, tmp_path, monkeypatch):
        self._fake_render(monkeypatch, tmp_path)
        path = make_pdf()
        pngs = preview.preview_file(path, tmp_path / "out", dpi=72, guides=True, safe_mm=3.0, gs_bin="gs")
        assert preview._TRIM_RGB in _pixels(pngs[0])

    def test_no_guides_leaves_the_raster_untouched(self, make_pdf, tmp_path, monkeypatch):
        self._fake_render(monkeypatch, tmp_path)
        pngs = preview.preview_file(make_pdf(), tmp_path / "out", dpi=72, guides=False, safe_mm=3.0, gs_bin="gs")
        assert _pixels(pngs[0]) == {(255, 255, 255)}

    def test_the_guides_show_where_a_wrong_trimbox_actually_is(self, make_pdf, tmp_path, monkeypatch):
        """The visual counterpart of the geometry finding: a trim guide inset 25pt."""
        self._fake_render(monkeypatch, tmp_path)
        boxes = {
            "MediaBox": [0, 0, 337.32198, 459.21146],
            "TrimBox": [25, 20, 312.32198, 439.21146],
            "BleedBox": [0, 0, 337.32198, 459.21146],
        }
        pngs = preview.preview_file(
            make_pdf(boxes=boxes), tmp_path / "out", dpi=72, guides=True, safe_mm=3.0, gs_bin="gs"
        )
        with Image.open(pngs[0]) as img:
            flat = img.convert("RGB").get_flattened_data()
            row = flat[img.width * (img.height // 2) : img.width * (img.height // 2 + 1)]
        first_trim_x = next(x for x, px in enumerate(row) if px == preview._TRIM_RGB)
        # 25pt at 72dpi is 25px from the left edge, not the 7mm (~20px) it should be.
        assert first_trim_x == pytest.approx(25, abs=2)
