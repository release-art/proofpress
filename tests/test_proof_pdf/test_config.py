"""The config file: discovery, validation, profiles, and how it turns into CLI defaults."""

import pathlib

import pytest

from proof_pdf import config


class _Param:
    def __init__(self, name: str, flag: str | None = None, *, kind: str = "option", type_name: str = "text") -> None:
        self.name = name
        self.opts = [flag or f"--{name.replace('_', '-')}"]
        self.param_type_name = kind
        self.type = type("T", (), {"name": type_name})()


class _Command:
    def __init__(self, *params: _Param) -> None:
        self.params = list(params)


COMMANDS = {
    "verify": _Command(
        _Param("files", kind="argument"),
        _Param("min_text_pt", type_name="float"),
        _Param("colour"),
        _Param("exempt"),
        _Param("allow_spot"),
        _Param("verapdf_bin", "--verapdf"),
        _Param("proof_profile", type_name="path"),
    ),
    "texlog": _Command(_Param("max_overfull_pt")),
}


def _write(directory: pathlib.Path, text: str, name: str = config.CONFIG_NAME) -> pathlib.Path:
    path = directory / name
    path.write_text(text)
    return path


class TestFind:
    def test_walks_up_from_the_working_directory(self, tmp_path):
        found = _write(tmp_path, "[verify]\n")
        deep = tmp_path / "a" / "b"
        deep.mkdir(parents=True)
        assert config.find_config(deep) == found

    def test_the_nearest_file_wins(self, tmp_path):
        _write(tmp_path, "[verify]\n")
        (tmp_path / "job").mkdir()
        near = _write(tmp_path / "job", "[verify]\n")
        assert config.find_config(tmp_path / "job") == near

    def test_pyproject_counts_only_with_a_tool_table(self, tmp_path):
        _write(tmp_path, "[project]\nname = 'x'\n", "pyproject.toml")
        assert config.find_config(tmp_path) is None
        found = _write(tmp_path, "[tool.proof-pdf.verify]\nmin-text-pt = 6\n", "pyproject.toml")
        assert config.find_config(tmp_path) == found

    def test_the_dedicated_file_beats_pyproject_in_the_same_directory(self, tmp_path):
        _write(tmp_path, "[tool.proof-pdf.verify]\n", "pyproject.toml")
        dedicated = _write(tmp_path, "[verify]\n")
        assert config.find_config(tmp_path) == dedicated

    def test_none_when_nothing_is_there(self, tmp_path):
        assert config.find_config(tmp_path) is None


class TestDefaultMap:
    def test_keys_may_use_dashes_underscores_or_the_flag_name(self, tmp_path):
        path = _write(tmp_path, '[verify]\nmin-text-pt = 6\ncolour = false\nverapdf = "/opt/verapdf"\n')
        assert config.default_map(path, None, COMMANDS) == {
            "verify": {"min_text_pt": 6, "colour": False, "verapdf_bin": "/opt/verapdf"}
        }
        again = _write(tmp_path, '[verify]\nmin_text_pt = 6\nverapdf_bin = "/opt/verapdf"\n')
        assert config.default_map(again, None, COMMANDS)["verify"]["min_text_pt"] == 6

    def test_an_exempt_table_becomes_glob_equals_reason_options(self, tmp_path):
        path = _write(tmp_path, '[verify.exempt]\n"manual.pdf" = "internal docs"\n"res/**" = "inputs"\n')
        assert config.default_map(path, None, COMMANDS)["verify"]["exempt"] == [
            "manual.pdf=internal docs",
            "res/**=inputs",
        ]

    def test_lists_pass_through(self, tmp_path):
        path = _write(tmp_path, '[verify]\nallow-spot = ["Dieline", "Foil"]\n')
        assert config.default_map(path, None, COMMANDS)["verify"]["allow_spot"] == ["Dieline", "Foil"]

    def test_paths_are_relative_to_the_config_file_not_the_working_directory(self, tmp_path):
        (tmp_path / "icc").mkdir()
        path = _write(tmp_path, '[verify]\nproof-profile = "icc/fogra51.icc"\n')
        assert config.default_map(path, None, COMMANDS)["verify"]["proof_profile"] == str(
            (tmp_path / "icc" / "fogra51.icc").resolve()
        )

    def test_an_absolute_path_is_left_alone(self, tmp_path):
        path = _write(tmp_path, '[verify]\nproof-profile = "/abs/p.icc"\n')
        assert config.default_map(path, None, COMMANDS)["verify"]["proof_profile"] == "/abs/p.icc"

    def test_pyproject_uses_the_tool_table(self, tmp_path):
        path = _write(tmp_path, "[tool.proof-pdf.texlog]\nmax-overfull-pt = 10\n", "pyproject.toml")
        assert config.default_map(path, None, COMMANDS) == {"texlog": {"max_overfull_pt": 10}}


class TestProfiles:
    TEXT = (
        "[verify]\nmin-text-pt = 6\ncolour = true\n"
        "[profiles.deluxe.verify]\nmin-text-pt = 8\n"
        "[profiles.deluxe.texlog]\nmax-overfull-pt = 5\n"
    )

    def test_a_profile_overrides_the_base_per_option(self, tmp_path):
        path = _write(tmp_path, self.TEXT)
        result = config.default_map(path, "deluxe", COMMANDS)
        assert result["verify"] == {"min_text_pt": 8, "colour": True}
        assert result["texlog"] == {"max_overfull_pt": 5}

    def test_without_a_profile_only_the_base_applies(self, tmp_path):
        assert config.default_map(_write(tmp_path, self.TEXT), None, COMMANDS) == {
            "verify": {"min_text_pt": 6, "colour": True}
        }

    def test_an_unknown_profile_lists_the_known_ones(self, tmp_path):
        with pytest.raises(config.ConfigError, match=r"no profile 'nope'.*deluxe"):
            config.default_map(_write(tmp_path, self.TEXT), "nope", COMMANDS)

    def test_a_profile_with_no_profiles_table_says_none(self, tmp_path):
        with pytest.raises(config.ConfigError, match="profiles: none"):
            config.default_map(_write(tmp_path, "[verify]\n"), "x", COMMANDS)


class TestErrors:
    def test_a_misspelled_option_is_rejected_with_the_valid_ones(self, tmp_path):
        with pytest.raises(config.ConfigError, match=r"\[verify\] has no option 'min-txt-pt'.*min-text-pt"):
            config.default_map(_write(tmp_path, "[verify]\nmin-txt-pt = 6\n"), None, COMMANDS)

    def test_a_positional_argument_cannot_be_set(self, tmp_path):
        with pytest.raises(config.ConfigError, match="no option 'files'"):
            config.default_map(_write(tmp_path, '[verify]\nfiles = ["a.pdf"]\n'), None, COMMANDS)

    def test_an_unknown_command_table_is_rejected(self, tmp_path):
        with pytest.raises(config.ConfigError, match=r"\[nope\] is not a command.*texlog, verify"):
            config.default_map(_write(tmp_path, "[nope]\nx = 1\n"), None, COMMANDS)

    def test_invalid_toml_is_a_config_error_not_a_traceback(self, tmp_path):
        with pytest.raises(config.ConfigError, match="cannot read"):
            config.default_map(_write(tmp_path, "[verify\n"), None, COMMANDS)

    def test_a_pyproject_without_the_table_is_an_error_if_named_explicitly(self, tmp_path):
        with pytest.raises(config.ConfigError, match=r"no \[tool.proof-pdf\]"):
            config.default_map(_write(tmp_path, "[project]\n", "pyproject.toml"), None, COMMANDS)
