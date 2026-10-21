"""Read a ``proof-pdf.toml`` the way the CLI does, for library callers.

One file can then carry a printer's numbers for both ``proof-pdf verify`` and Python code that calls
:func:`proof_pdf.core.verify_files`.
"""

import dataclasses
import os
import pathlib
import typing

import typer
import typer.main

from ._internal import config as _config
from ._internal.config import ConfigError
from .models import Profile

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
    """The settings the CLI's *command* would use, read from the config file.

    *path* defaults to ``PROOF_PDF_CONFIG``, else the nearest ``proof-pdf.toml`` (or ``[tool.proof-pdf]`` in
    ``pyproject.toml``) from the working directory; *profile* defaults to ``PROOF_PDF_PROFILE``. With no file, every
    option has its CLI default. Raises :class:`ConfigError` for an unreadable file, an unknown key or table, a profile
    the file lacks, or a value the CLI would reject.
    """
    from . import cli  # deferred: the CLI framework is only loaded when a caller wants its behaviour

    if command != "verify":
        raise ConfigError(f"only the verify command makes a profile, not {command!r}")
    path = path or (pathlib.Path(env) if (env := os.environ.get("PROOF_PDF_CONFIG")) else None)
    profile = profile or os.environ.get("PROOF_PDF_PROFILE") or None
    path = path or _config.find_config(pathlib.Path.cwd())
    if path is None and profile:
        raise ConfigError("a profile needs a config file, and none was found")
    commands = _click_commands()
    defaults = _config.default_map(path, profile, commands) if path else {}
    try:
        with commands[command].make_context(command, [], default_map=defaults.get(command, {})) as ctx:
            values = ctx.params
        built = cli.build_profile(values)
        exempt = cli._parse_exemptions(values["exempt"] or [])
    except typer.BadParameter as exc:  # a bad value is the file's fault, not a CLI usage error
        raise ConfigError(f"{path or 'defaults'}: {exc.format_message()}") from exc
    return LoadedConfig(
        profile=built,
        exclude=tuple(values["exclude"] or ()),
        exempt=exempt,
        jobs=values["jobs"],
        timeout=values["timeout"],
        verapdf_bin=values["verapdf_bin"],
        gs_bin=values["gs_bin"],
        path=path,
    )


def _click_commands() -> dict[str, typing.Any]:
    from . import cli

    return dict(getattr(typer.main.get_command(cli.pdf_app), "commands", {}))
