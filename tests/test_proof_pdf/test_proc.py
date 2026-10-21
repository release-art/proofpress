"""``proc.run`` against real processes: output, exit codes, timeouts and cancellation."""

import asyncio
import pathlib
import sys

import pytest

from proof_pdf import proc


async def test_captures_output_and_exit_code():
    result = await proc.run(
        [sys.executable, "-c", "import sys; print('out'); print('err', file=sys.stderr); sys.exit(3)"]
    )
    assert (result.returncode, result.stdout.strip(), result.stderr.strip()) == (3, "out", "err")


async def test_timeout_kills_the_process(tmp_path: pathlib.Path):
    pidfile = tmp_path / "pid"
    code = f"import os, pathlib, time; pathlib.Path({str(pidfile)!r}).write_text(str(os.getpid())); time.sleep(30)"
    with pytest.raises(proc.ProcTimeoutError, match="timed out"):
        await proc.run([sys.executable, "-c", code], timeout=1)
    pid = int(pidfile.read_text())
    with pytest.raises(ProcessLookupError):
        import os

        os.kill(pid, 0)


async def test_cancellation_kills_the_process(tmp_path: pathlib.Path):
    pidfile = tmp_path / "pid"
    code = f"import os, pathlib, time; pathlib.Path({str(pidfile)!r}).write_text(str(os.getpid())); time.sleep(30)"
    task = asyncio.create_task(proc.run([sys.executable, "-c", code]))
    while not pidfile.exists():
        await asyncio.sleep(0.05)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    with pytest.raises(ProcessLookupError):
        import os

        os.kill(int(pidfile.read_text()), 0)


async def test_runs_do_not_block_each_other():
    """Two one-second sleeps overlap: the event loop is free while a process runs."""
    loop = asyncio.get_running_loop()
    start = loop.time()
    await asyncio.gather(*(proc.run([sys.executable, "-c", "import time; time.sleep(1)"]) for _ in range(3)))
    assert loop.time() - start < 2.5
