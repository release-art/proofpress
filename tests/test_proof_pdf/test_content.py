"""The shared content-stream walker, and the Form XObject blindness it fixes."""

import pikepdf
import pytest

from proof_pdf._internal import content


def _ops(path, **kwargs) -> list[content.Operation]:
    with pikepdf.open(path) as pdf:
        return list(content.iter_operations(pdf.pages[0], **kwargs))


class TestMatrixMath:
    def test_identity_is_neutral(self):
        m = (2.0, 0.0, 0.0, 3.0, 4.0, 5.0)
        assert content.mul(m, content.IDENTITY) == m
        assert content.mul(content.IDENTITY, m) == m

    @pytest.mark.parametrize(
        ("m", "horizontal", "vertical"),
        [
            ((1.0, 0.0, 0.0, 1.0, 0.0, 0.0), 1.0, 1.0),
            ((3.0, 0.0, 0.0, 7.0, 0.0, 0.0), 3.0, 7.0),
            # A 90-degree rotation swaps which axis each scale lands on.
            ((0.0, 2.0, -5.0, 0.0, 0.0, 0.0), 2.0, 5.0),
        ],
    )
    def test_scales(self, m, horizontal, vertical):
        assert content.horizontal_scale(m) == pytest.approx(horizontal)
        assert content.vertical_scale(m) == pytest.approx(vertical)

    @pytest.mark.parametrize("operands", [[], [1, 2, 3], ["a", "b", "c", "d", "e", "f"]])
    def test_malformed_matrix_falls_back_to_identity(self, operands):
        assert content.matrix(operands) == content.IDENTITY


class TestGraphicsState:
    def test_cm_accumulates_and_q_restores(self, make_content_pdf):
        path = make_content_pdf(b"2 0 0 2 0 0 cm q 3 0 0 3 0 0 cm Q 1 0 0 1 0 0 cm", font=False)
        by_op = [(op.operator, op.ctm) for op in _ops(path)]
        inside = next(ctm for name, ctm in by_op if name == "cm" and ctm[0] == 6.0)
        assert inside[0] == 6.0  # 2 x 3, concatenated
        after_restore = by_op[-1][1]
        assert after_restore[0] == pytest.approx(2.0)  # Q undid the inner cm

    def test_unbalanced_q_does_not_desync(self, make_content_pdf):
        """A stray Q restores to the stream's base rather than corrupting the CTM."""
        path = make_content_pdf(b"5 0 0 5 0 0 cm Q Q 1 0 0 1 0 0 cm", font=False)
        assert _ops(path)[-1].ctm == content.IDENTITY


class TestFormRecursion:
    def _pdf_with_form(self, tmp_path, make_form, inner: bytes, *, matrix=None, outer=b"/Fm1 Do"):
        path = tmp_path / "form.pdf"
        pdf = pikepdf.new()
        form = make_form(pdf, inner, matrix=matrix)
        page = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            MediaBox=[0, 0, 500, 500],
            Contents=pdf.make_stream(outer),
            Resources=pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=pdf.make_indirect(form))),
        )
        pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        pdf.save(path)
        return path

    def test_operators_inside_a_form_are_yielded(self, tmp_path, make_form):
        path = self._pdf_with_form(tmp_path, make_form, b"BT /F1 8 Tf (hi) Tj ET")
        names = [op.operator for op in _ops(path)]
        assert "Do" in names
        assert "Tj" in names, "content inside the Form XObject was not walked"
        assert max(op.depth for op in _ops(path)) == 1

    def test_form_matrix_folds_into_the_ctm(self, tmp_path, make_form):
        """A form scaled 4x reports its inner operators at 4x, not 1x."""
        path = self._pdf_with_form(tmp_path, make_form, b"1 0 0 1 0 0 cm", matrix=[4, 0, 0, 4, 0, 0])
        inner = [op for op in _ops(path) if op.depth == 1]
        assert inner and content.horizontal_scale(inner[-1].ctm) == pytest.approx(4.0)

    def test_enclosing_ctm_multiplies_with_the_form_matrix(self, tmp_path, make_form):
        path = self._pdf_with_form(
            tmp_path, make_form, b"1 0 0 1 0 0 cm", matrix=[2, 0, 0, 2, 0, 0], outer=b"3 0 0 3 0 0 cm /Fm1 Do"
        )
        inner = [op for op in _ops(path) if op.depth == 1]
        assert content.horizontal_scale(inner[-1].ctm) == pytest.approx(6.0)

    def test_form_without_resources_inherits_the_page_scope(self, tmp_path, make_form):
        path = self._pdf_with_form(tmp_path, make_form, b"/Fm1 Do")
        # The form draws itself; the cycle guard must stop it rather than recurse away.
        depths = {op.depth for op in _ops(path)}
        assert depths == {0, 1}

    def test_max_depth_caps_the_walk(self, tmp_path, make_form):
        """Two mutually nested forms stop at the configured depth."""
        path = tmp_path / "deep.pdf"
        pdf = pikepdf.new()
        inner = pdf.make_indirect(make_form(pdf, b"1 0 0 1 0 0 cm"))
        outer = pdf.make_indirect(
            make_form(pdf, b"/Fm2 Do", resources=pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm2=inner)))
        )
        page = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            MediaBox=[0, 0, 500, 500],
            Contents=pdf.make_stream(b"/Fm1 Do"),
            Resources=pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=outer)),
        )
        pdf.pages.append(pikepdf.Page(pdf.make_indirect(page)))
        pdf.save(path)
        assert max(op.depth for op in _ops(path)) == 2
        assert max(op.depth for op in _ops(path, max_depth=1)) == 1


class TestLookup:
    @pytest.mark.parametrize("operands", [[], [pikepdf.Name("/Missing")]])
    def test_missing_names_resolve_to_none(self, make_content_pdf, operands):
        with pikepdf.open(make_content_pdf(b"", font=False)) as pdf:
            resources = pdf.pages[0].obj.get("/Resources")
            assert content.lookup_xobject(resources, operands) is None

    def test_no_resources_resolves_to_none(self):
        assert content.lookup_xobject(None, [pikepdf.Name("/Fm1")]) is None
