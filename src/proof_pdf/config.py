"""Read a ``proof-pdf.toml`` the way ``proof-pdf verify`` does, for library callers.

One file can then carry a printer's numbers for both ``proof-pdf verify`` and Python code that calls
:func:`proof_pdf.core.verify_files`.
"""

import dataclasses
import os
import pathlib
import tomllib
import typing

from ._internal import (
    _core,
    config as _config,
)
from ._internal.config import ConfigError
from .core import DEFAULT_JOBS
from .models import DEFAULT_MAX_TAC, DEFAULT_MIN_DPI, DEFAULT_SMALL_TEXT_PT, ColourMode, PageGeometry, Profile

__all__ = ["ConfigError", "LoadedConfig", "load_profile"]


@dataclasses.dataclass(frozen=True)
class LoadedConfig:
    """What ``verify`` would run with: the checks and the file selection, from the file plus the option defaults."""

    profile: Profile
    exclude: tuple[str, ...]  # globs for :func:`proof_pdf.core.expand_targets`
    exempt: dict[str, str]  # glob -> reason, for ``verify_files(exemptions=...)``
    jobs: int
    timeout: float
    verapdf_bin: str
    gs_bin: str
    path: pathlib.Path | None  # the file that was read; None when none was found


def load_profile(
    path: pathlib.Path | None = None, *, profile: str | None = None, command: str = "verify"
) -> LoadedConfig:
    """The settings ``proof-pdf verify`` would use, read from the config file.

    *path* defaults to ``PROOF_PDF_CONFIG``, else the nearest ``proof-pdf.toml`` (or ``[tool.proof-pdf]`` in
    ``pyproject.toml``) from the working directory; *profile* defaults to ``PROOF_PDF_PROFILE``. With no file, every
    option has its default. The file is read by the command itself (``verify --show-profile``), so it is held to the
    same rules: raises :class:`ConfigError` for an unreadable file, an unknown key or table, a profile the file lacks,
    or a value the command would reject.
    """
    if command != "verify":
        raise ConfigError(f"only the verify command makes a profile, not {command!r}")
    path = path or (pathlib.Path(env) if (env := os.environ.get("PROOF_PDF_CONFIG")) else None)
    profile = profile or os.environ.get("PROOF_PDF_PROFILE") or None
    path = path or _config.find_config(pathlib.Path.cwd())
    if path is None and profile:
        raise ConfigError("a profile needs a config file, and none was found")
    table: dict[str, typing.Any] = {}
    if path is not None:
        argv = ["proof-pdf", "--config", str(path), *(["--profile", profile] if profile else []), "verify"]
        code, out, err = _core.run([*argv, "--show-profile"])
        if code != 0:
            raise ConfigError(err.strip().removeprefix("error: "))
        table = {key.replace("_", "-"): value for key, value in tomllib.loads(out).items()}
    return _loaded(table, path)


def _loaded(table: dict[str, typing.Any], path: pathlib.Path | None) -> LoadedConfig:
    """The :class:`LoadedConfig` a validated ``[verify]`` table describes."""

    def get(*keys: str, default: typing.Any = None) -> typing.Any:
        return next((table[k] for k in keys if k in table), default)

    def on(key: str, default: bool) -> bool:
        return bool(get(key, default=default)) and not get(f"no-{key}", default=False)

    def listed(key: str) -> tuple[str, ...]:
        value = get(key, default=())
        return (value,) if isinstance(value, str) else tuple(value)

    geometry = None
    if (trim := get("trim-mm")) is not None:
        width, height = (float(p) for p in str(trim).lower().replace("*", "x").split("x"))
        geometry = PageGeometry(
            trim_w_mm=width,
            trim_h_mm=height,
            bleed_mm=float(get("bleed-mm", default=3.0)),
            tolerance_mm=float(get("tolerance-mm", default=0.25)),
            binding_bleed_mm=get("binding-bleed-mm"),
            binding=get("binding", default="left"),
        )
    pdfx = get("pdfx", default=True)
    proof = get("proof-profile")
    exempt = get("exempt", default={})
    built = Profile(
        flavour=get("flavour", default="2b"),
        pdfa=bool(get("pdfa", default=False)),
        pdfx=pdfx is not False and not get("no-pdfx", default=False),
        colour=on("colour", True),
        tac=bool(get("tac", default=False)),
        geometry=geometry,
        max_tac=float(get("max-tac", default=DEFAULT_MAX_TAC)),
        warn_tac=get("warn-tac"),
        small_text_pt=float(get("small-text-pt", default=DEFAULT_SMALL_TEXT_PT)),
        min_dpi=float(get("min-dpi", default=DEFAULT_MIN_DPI)),
        warn_dpi=get("warn-dpi"),
        max_dpi=get("max-dpi"),
        max_stretch_pct=get("max-stretch-pct"),
        strict_cmyk=bool(get("strict-cmyk", default=False)),
        colour_mode=ColourMode(get("colour-mode", default="cmyk").lower()),
        allow_k_only=bool(get("allow-k-only", default=False)),
        k_only_text=bool(get("k-only-text", default=False)),
        missing_glyphs=on("missing-glyphs", True),
        min_text_pt=get("min-text-pt"),
        min_stroke_pt=get("min-stroke-pt"),
        safe_mm=get("safe-mm"),
        min_delta_l=get("min-delta-l"),
        proof_profile=None if proof is None or path is None else (path.parent / proof).resolve(),
        fonts=on("fonts", True),
        allow_spots=listed("allow-spot") or None if "allow-spot" in table else None,
        output_condition=get("output-condition"),
        uniform_pages=bool(get("uniform-pages", default=False)),
        pages_multiple_of=get("pages-multiple-of"),
        require_bleed=bool(get("require-bleed", default=False)),
        no_marks=bool(get("no-marks", default=False)),
    )
    exempt_items = exempt.items() if isinstance(exempt, dict) else (e.split("=", 1) for e in exempt)
    return LoadedConfig(
        profile=built,
        exclude=listed("exclude"),
        exempt={glob.strip(): reason.strip() for glob, reason in exempt_items},
        jobs=int(get("jobs", "j", default=DEFAULT_JOBS)),
        timeout=float(get("timeout", default=900.0)),
        verapdf_bin=get("verapdf", "verapdf-bin", default="verapdf"),
        gs_bin=get("gs", "gs-bin", default="gs"),
        path=path,
    )
