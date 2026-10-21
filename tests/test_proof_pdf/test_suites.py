"""The Ghent and Altona test suites: nothing crashes, and each file raises exactly the checks in ``expected.toml``.

The suites are valid PDF/X-4 files built to exercise a RIP, so every finding is either the file doing its job or a
false positive of ours. They are not redistributable: ``bin/suites/fetch.sh`` downloads them into a git-ignored
cache and these tests skip without it.
"""

import pathlib
import shutil
import tomllib

import pytest

from proof_pdf import core, inventory
from proof_pdf.models import Profile

SUITES = pathlib.Path(__file__).parents[1] / "fixtures" / "suites"
EXPECTED = tomllib.loads((SUITES / "expected.toml").read_text())

pytestmark = [
    pytest.mark.container,
    pytest.mark.skipif(shutil.which("gs") is None, reason="needs Ghostscript on PATH"),
    pytest.mark.timeout(600),
]


@pytest.fixture(params=sorted(EXPECTED))
def suite_file(request) -> pathlib.Path:
    path = SUITES / ".cache" / request.param
    if not path.is_file():
        pytest.skip(f"{request.param} not fetched (bin/suites/fetch.sh)")
    return path


async def test_the_checks_raised_are_the_expected_ones(suite_file):
    report = await core.verify_file(suite_file, Profile(pdfa=False))
    assert not any(f.check == "structure" for f in report.findings), "the file must open"
    assert {f.check for f in report.findings} == set(EXPECTED[suite_file.name])


def test_inspect_reads_the_file(suite_file):
    assert inventory.inspect_pdf(suite_file).pages
