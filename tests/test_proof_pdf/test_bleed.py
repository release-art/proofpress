"""The artwork-bleed check, and the clip tracking it relies on."""

import pathlib

import pikepdf
import pytest

from proof_pdf import paint, structure
from proof_pdf.models import Severity

from .conftest import BLEED, GOOD_BOXES, MEDIA_H, MEDIA_W, MM, TRIM_H, TRIM_W

T = BLEED  # the trim's lower-left corner, in points
TR, TT = BLEED + TRIM_W, BLEED + TRIM_H  # and its upper-right


def _pdf(tmp_path, stream: bytes, *, boxes=None, rotate: int = 0, make_form=None, xobjects=None) -> pathlib.Path:
    pdf = pikepdf.new()
    resources = pikepdf.Dictionary()
    if xobjects:
        resources["/XObject"] = pikepdf.Dictionary({f"/{k}": v(pdf) for k, v in xobjects.items()})
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page, Contents=pdf.make_stream(stream), Resources=resources, **(boxes or GOOD_BOXES)
    )
    if rotate:
        page["/Rotate"] = rotate
    pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
    path = tmp_path / "bleed.pdf"
    pdf.save(path)
    return path


def _check(path):
    with pikepdf.open(path) as pdf:
        return structure.check_artwork_bleed(pdf)


def _rect(x0, y0, x1, y1, op=b"f") -> bytes:
    return b"%f %f %f %f re %s " % (x0, y0, x1 - x0, y1 - y0, op)


FULL = _rect(0, 0, MEDIA_W, MEDIA_H)


class TestCoverage:
    def test_full_bleed_passes(self, tmp_path):
        assert _check(_pdf(tmp_path, b"0.9 g " + FULL)) == []

    @pytest.mark.parametrize(
        ("rect", "side"),
        [
            pytest.param((T, 0, MEDIA_W, MEDIA_H), "left", id="left"),
            pytest.param((0, T, MEDIA_W, MEDIA_H), "bottom", id="bottom"),
            pytest.param((0, 0, TR, MEDIA_H), "right", id="right"),
            pytest.param((0, 0, MEDIA_W, TT), "top", id="top"),
        ],
    )
    def test_a_fill_stopping_at_the_trim_fails_on_that_side(self, tmp_path, rect, side):
        [finding] = _check(_pdf(tmp_path, b"0.9 g " + _rect(*rect)))
        assert finding.check == "artwork-bleed" and finding.severity is Severity.ERROR and finding.page == 1
        assert f"{side}: 1 object(s)" in finding.message and "0.0 mm of the 3.0 mm" in finding.message
        assert "artwork must run to the bleed edge" in finding.message

    def test_partial_bleed_reports_how_far_it_got(self, tmp_path):
        [finding] = _check(_pdf(tmp_path, b"0.9 g " + _rect(T - 1.2 * MM, 0, MEDIA_W, MEDIA_H)))
        assert "left: 1 object(s)" in finding.message and "1.2 mm of the 3.0 mm" in finding.message

    def test_sides_collapse_into_one_finding_per_page(self, tmp_path):
        [finding] = _check(_pdf(tmp_path, b"0.9 g " + _rect(T, T, TR, TT)))
        assert all(side in finding.message for side in ("left", "bottom", "right", "top"))
        assert "1 object(s) stop short" in finding.message

    def test_an_inset_object_is_ignored(self, tmp_path):
        assert _check(_pdf(tmp_path, b"0.9 g " + FULL + b"0.2 g " + _rect(T + 20, T + 20, TR - 20, TT - 20))) == []

    def test_an_object_inside_the_bleed_zone_does_not_touch_the_page(self, tmp_path):
        assert _check(_pdf(tmp_path, _rect(0, 0, T - 1, MEDIA_H) + b"0.9 g " + FULL)) == []

    def test_a_panel_at_the_trim_over_a_full_bleed_background_passes(self, tmp_path):
        assert _check(_pdf(tmp_path, b"0.9 g " + FULL + b"1 g " + _rect(T, T, TR, TT))) == []

    def test_but_not_where_the_background_leaves_a_gap(self, tmp_path):
        half = _rect(0, 0, MEDIA_W, MEDIA_H / 2)  # the lower half only
        [finding] = _check(_pdf(tmp_path, b"0.9 g " + half + b"1 g " + _rect(T, T, TR, TT)))
        assert "top:" in finding.message and "bottom:" not in finding.message

    def test_a_strip_drawn_in_the_bleed_zone_alone_covers_it(self, tmp_path):
        strip = _rect(0, 0, T, MEDIA_H)
        assert _check(_pdf(tmp_path, strip + b"0.9 g " + _rect(T, 0, MEDIA_W, MEDIA_H))) == []

    def test_white_counts(self, tmp_path):
        assert len(_check(_pdf(tmp_path, b"1 g " + _rect(T, T, TR, TT)))) == 1

    def test_strokes_and_text_are_not_backgrounds(self, tmp_path):
        stream = b"0 G " + _rect(T, T, TR, TT, b"S") + b"BT /F1 12 Tf %f %f Td (x) Tj ET" % (T, T)
        assert _check(_pdf(tmp_path, stream)) == []


class TestClipping:
    def _image(self, make_image):
        return {"Im": lambda pdf: make_image(pdf)}

    def test_an_image_overshooting_the_bleed_but_clipped_to_the_trim_fails(self, tmp_path, make_image):
        stream = b"q " + _rect(T, T, TR, TT, b"W n") + b"%f 0 0 %f -50 -50 cm /Im Do Q" % (MEDIA_W + 100, MEDIA_H + 100)
        path = _pdf(tmp_path, stream, xobjects=self._image(make_image))
        assert len(_check(path)) == 1

    def test_an_image_clipped_to_the_bleed_box_passes(self, tmp_path, make_image):
        stream = (
            b"q "
            + _rect(0, 0, MEDIA_W, MEDIA_H, b"W n")
            + b"%f 0 0 %f -50 -50 cm /Im Do Q"
            % (
                MEDIA_W + 100,
                MEDIA_H + 100,
            )
        )
        assert _check(_pdf(tmp_path, stream, xobjects=self._image(make_image))) == []

    def test_a_clip_ends_with_its_q(self, tmp_path):
        stream = b"q " + _rect(T, T, TR, TT, b"W n") + b"Q 0.9 g " + FULL
        assert _check(_pdf(tmp_path, stream)) == []

    def test_a_clip_inside_a_form_ends_with_the_form(self, tmp_path, make_form):
        form = lambda pdf: make_form(pdf, _rect(T, T, TR, TT, b"W n"))  # noqa: E731
        assert _check(_pdf(tmp_path, b"/Fm Do 0.9 g " + FULL, xobjects={"Fm": form})) == []

    def test_a_clip_set_before_a_form_applies_inside_it(self, tmp_path, make_form):
        form = lambda pdf: make_form(pdf, b"0.9 g " + FULL)  # noqa: E731
        stream = b"q " + _rect(T, T, TR, TT, b"W n") + b"/Fm Do Q"
        assert len(_check(_pdf(tmp_path, stream, xobjects={"Fm": form}))) == 1

    def test_a_non_rectangular_clip_counts_as_its_extent(self, tmp_path):
        triangle = b"%f %f m %f %f l %f %f l h W n " % (T, T, TR, T, TR, TT)
        assert len(_check(_pdf(tmp_path, b"q " + triangle + b"0.9 g " + FULL + b"Q"))) == 1

    def test_visible_is_the_bbox_cut_to_the_clip_and_the_bbox_is_unchanged(self, tmp_path):
        path = _pdf(tmp_path, b"q 10 10 50 50 re W n 0 0 100 100 re f Q 0 0 100 100 re f")
        with pikepdf.open(path) as pdf:
            clipped, free = paint.paint_events(pdf.pages[0])
        assert clipped.bbox == (0, 0, 100, 100) and clipped.visible == (10, 10, 60, 60)
        assert free.visible == free.bbox

    def test_a_fully_clipped_object_has_nothing_visible(self, tmp_path):
        with pikepdf.open(_pdf(tmp_path, b"q 0 0 10 10 re W* n 50 50 5 5 re f Q")) as pdf:
            [only] = paint.paint_events(pdf.pages[0])
        assert only.visible is None and only.bbox is not None


class TestScope:
    def test_no_trimbox_is_skipped(self, tmp_path):
        boxes = {"MediaBox": GOOD_BOXES["MediaBox"], "BleedBox": GOOD_BOXES["BleedBox"]}
        assert _check(_pdf(tmp_path, b"0.9 g " + _rect(T, T, TR, TT), boxes=boxes)) == []

    def test_no_bleed_zone_is_skipped(self, tmp_path):
        box = [0, 0, TRIM_W, TRIM_H]
        boxes = {"MediaBox": box, "TrimBox": box, "BleedBox": box}
        assert _check(_pdf(tmp_path, b"0.9 g " + _rect(0, 0, TRIM_W, TRIM_H), boxes=boxes)) == []

    def test_a_missing_bleedbox_falls_back_to_the_mediabox(self, tmp_path):
        boxes = {"MediaBox": GOOD_BOXES["MediaBox"], "TrimBox": GOOD_BOXES["TrimBox"]}
        assert len(_check(_pdf(tmp_path, b"0.9 g " + _rect(T, T, TR, TT), boxes=boxes))) == 1

    def test_a_rotated_page_is_reported_as_not_checked(self, tmp_path):
        [finding] = _check(_pdf(tmp_path, b"0.9 g " + _rect(T, T, TR, TT), rotate=90))
        assert finding.severity is Severity.WARN and "not checked" in finding.message
