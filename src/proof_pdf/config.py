"""Per-project and per-printer defaults for the command line.

A job's numbers (trim, bleed, size floors, the output condition, allowed spot inks, exemptions) come from its printer's
spec, so they belong in a file that is committed with the job, not in ten flags repeated in CI. The file is
``proof-pdf.toml`` or a ``[tool.proof-pdf]`` table in ``pyproject.toml``, found by walking up from the working
directory (or named with ``--config`` / ``PROOF_PDF_CONFIG``)::

    [verify]
    trim-mm = "105x148"
    bleed-mm = 3
    min-text-pt = 6
    output-condition = "FOGRA51"
    allow-spot = ["Dieline"]
    proof-profile = "profiles/PSOcoated_v3.icc"      # relative to this file

    [verify.exempt]
    "manual.pdf" = "internal documentation, never printed"

    [profiles.deluxe.verify]                          # chosen with --profile deluxe; overrides [verify]
    bleed-mm = 5

Each table is a command (``verify``, ``inspect``, ``preview``, ``texlog``) and each key an option of that command,
spelled with dashes or underscores. An option given on the command line always wins. Unknown tables and keys are
errors, so a typo cannot silently turn a check off.
"""

import pathlib
import tomllib
import typing

CONFIG_NAME = "proof-pdf.toml"
_PYPROJECT = "pyproject.toml"


class ConfigError(Exception):
    """The configuration file is unreadable or says something this tool does not understand."""


def find_config(start: pathlib.Path) -> pathlib.Path | None:
    """The nearest ``proof-pdf.toml``, or ``pyproject.toml`` with a ``[tool.proof-pdf]`` table, from *start* upwards."""
    for directory in (start, *start.parents):
        if (directory / CONFIG_NAME).is_file():
            return directory / CONFIG_NAME
        pyproject = directory / _PYPROJECT
        if pyproject.is_file() and _tool_table(pyproject) is not None:
            return pyproject
    return None


def default_map(
    path: pathlib.Path, profile: str | None, commands: typing.Mapping[str, typing.Any]
) -> dict[str, dict[str, typing.Any]]:
    """The ``default_map`` the file (and the chosen *profile*) describes, validated against the CLI's *commands*.

    *commands* are the CLI framework's command objects; only their ``params`` are read (name, kind and type name),
    so this module does not import the framework.
    """
    table = _tool_table(path) if path.name == _PYPROJECT else _read(path)
    if table is None:
        raise ConfigError(f"{path} has no [tool.proof-pdf] table")
    profiles = table.pop("profiles", {})
    selected = _select(profiles, profile, path)
    result: dict[str, dict[str, typing.Any]] = {}
    for name in {*table, *selected}:
        command = commands.get(name)
        if command is None:
            raise ConfigError(f"{path}: [{name}] is not a command; the commands are {', '.join(sorted(commands))}")
        merged = {**table.get(name, {}), **selected.get(name, {})}
        result[name] = _options(merged, command, name, path)
    return result


def _read(path: pathlib.Path) -> dict[str, typing.Any]:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise ConfigError(f"cannot read {path}: {exc}") from exc


def _tool_table(pyproject: pathlib.Path) -> dict[str, typing.Any] | None:
    try:
        table = _read(pyproject).get("tool", {}).get("proof-pdf")
    except ConfigError:
        return None
    return dict(table) if isinstance(table, dict) else None


def _select(profiles: object, name: str | None, path: pathlib.Path) -> dict[str, dict[str, typing.Any]]:
    if name is None:
        return {}
    if not isinstance(profiles, dict) or name not in profiles:
        known = ", ".join(sorted(profiles)) if isinstance(profiles, dict) and profiles else "none"
        raise ConfigError(f"{path} has no profile {name!r} (profiles: {known})")
    return profiles[name]


def _options(raw: dict[str, typing.Any], command: typing.Any, name: str, path: pathlib.Path) -> dict[str, typing.Any]:
    options = [p for p in command.params if p.param_type_name == "option" and p.name]
    # A key may be the option's parameter name or its long flag, with dashes or underscores: ``verapdf`` and
    # ``verapdf-bin`` both reach ``--verapdf``.
    by_key = {_key(p.name): p for p in options}
    by_key.update({_key(flag): p for p in options for flag in p.opts if flag.startswith("--")})
    resolved: dict[str, typing.Any] = {}
    for key, value in raw.items():
        param = by_key.get(_key(key))
        if param is None:
            valid = ", ".join(sorted(_key(p.opts[0]).replace("_", "-") for p in options if p.opts))
            raise ConfigError(f"{path}: [{name}] has no option {key!r} (options: {valid})")
        resolved[param.name] = _value(value, param, path)
    return resolved


def _key(name: str) -> str:
    return name.lstrip("-").replace("-", "_")


def _value(value: typing.Any, param: typing.Any, path: pathlib.Path) -> typing.Any:
    if param.name == "exempt" and isinstance(value, dict):
        return [f"{glob}={reason}" for glob, reason in value.items()]
    if param.type.name == "path":
        items = value if isinstance(value, list) else [value]
        resolved = [str(_relative_to(path, str(item))) for item in items]
        return resolved if isinstance(value, list) else resolved[0]
    return value


def _relative_to(config: pathlib.Path, item: str) -> pathlib.Path:
    candidate = pathlib.Path(item)
    return candidate if candidate.is_absolute() else (config.parent / candidate).resolve()
