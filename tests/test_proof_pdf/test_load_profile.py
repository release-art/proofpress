"""``proof_pdf.load_profile``: the CLI's config reading, for library callers."""

import pytest

import proof_pdf
from proof_pdf.models import PageGeometry, Profile

CARD = """
[verify]
trim-mm = "105x148"
bleed-mm = 3
exclude = ["res/**", "_views/**", "*_preview.pdf"]
safe-mm = 3
require-bleed = true

[verify.exempt]
"manual.pdf" = "internal"

[profiles.deluxe.verify]
bleed-mm = 5
"""


@pytest.fixture
def card(tmp_path, monkeypatch):
    monkeypatch.delenv("PROOF_PDF_CONFIG", raising=False)
    monkeypatch.delenv("PROOF_PDF_PROFILE", raising=False)
    path = tmp_path / "proof-pdf.toml"
    path.write_text(CARD)
    return path


def test_the_file_gives_the_profile_the_globs_and_the_exemptions(card):
    loaded = proof_pdf.load_profile(card)
    assert loaded.profile.geometry == PageGeometry(trim_w_mm=105, trim_h_mm=148, bleed_mm=3)
    assert loaded.profile.safe_mm == 3 and loaded.profile.require_bleed is True
    assert loaded.exclude == ("res/**", "_views/**", "*_preview.pdf")
    assert loaded.exempt == {"manual.pdf": "internal"} and loaded.path == card


def test_a_named_profile_overrides_the_base(card):
    assert proof_pdf.load_profile(card, profile="deluxe").profile.geometry.bleed_mm == 5  # type: ignore[union-attr]


def test_the_nearest_file_is_found_from_the_working_directory(card, monkeypatch):
    monkeypatch.chdir(card.parent)
    assert proof_pdf.load_profile().path == card


def test_the_environment_names_the_file_and_profile(card, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path / "..")
    monkeypatch.setenv("PROOF_PDF_CONFIG", str(card))
    monkeypatch.setenv("PROOF_PDF_PROFILE", "deluxe")
    assert proof_pdf.load_profile().profile.geometry.bleed_mm == 5  # type: ignore[union-attr]


def test_no_file_means_the_cli_defaults(tmp_path, monkeypatch):
    monkeypatch.delenv("PROOF_PDF_CONFIG", raising=False)
    monkeypatch.chdir(tmp_path)
    loaded = proof_pdf.load_profile()
    assert loaded.profile == Profile() and loaded.path is None and loaded.exclude == () and loaded.exempt == {}


@pytest.mark.parametrize(
    "text",
    [
        pytest.param("[verify]\nnot-an-option = 1\n", id="unknown-key"),
        pytest.param('[verify]\ntrim-mm = "wide"\n', id="bad-trim"),
        pytest.param('[verify]\nmin-dpi = "high"\n', id="bad-type"),
        pytest.param("[nope]\nx = 1\n", id="unknown-table"),
    ],
)
def test_a_file_the_cli_would_reject_is_a_config_error(tmp_path, monkeypatch, text):
    monkeypatch.delenv("PROOF_PDF_CONFIG", raising=False)
    path = tmp_path / "proof-pdf.toml"
    path.write_text(text)
    with pytest.raises(proof_pdf.ConfigError):
        proof_pdf.load_profile(path)


def test_a_profile_without_a_file_is_a_config_error(tmp_path, monkeypatch):
    monkeypatch.delenv("PROOF_PDF_CONFIG", raising=False)
    monkeypatch.chdir(tmp_path)
    with pytest.raises(proof_pdf.ConfigError):
        proof_pdf.load_profile(profile="deluxe")


def test_only_verify_makes_a_profile():
    with pytest.raises(proof_pdf.ConfigError):
        proof_pdf.load_profile(command="inspect")
