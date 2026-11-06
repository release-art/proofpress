"""The sample builder (tests/fixtures/samples/build.py) loaded by path, as that directory is not a package."""

import importlib.util
import pathlib

_PATH = pathlib.Path(__file__).parents[1] / "fixtures" / "samples" / "build.py"
_spec = importlib.util.spec_from_file_location("samples_build", _PATH)
assert _spec is not None and _spec.loader is not None
build = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(build)
