"""Run an external tool without blocking the event loop.

The one place that spawns processes, so veraPDF and Ghostscript are awaited the same way and a cancelled or
timed-out run never leaves an orphan behind.
"""

import asyncio
import collections.abc
import contextlib
import subprocess


class ProcTimeoutError(Exception):
    """The process ran past its timeout and was killed."""


async def run(argv: collections.abc.Sequence[str], *, timeout: float | None = None) -> subprocess.CompletedProcess[str]:
    """Run *argv*, capture its output as text, and kill it if *timeout* elapses or the task is cancelled."""
    proc = await asyncio.create_subprocess_exec(*argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout)
    except BaseException as exc:
        with contextlib.suppress(ProcessLookupError):
            proc.kill()
        await proc.wait()
        if isinstance(exc, TimeoutError):
            raise ProcTimeoutError(f"{argv[0]} timed out after {timeout:.0f}s") from exc
        raise
    assert proc.returncode is not None  # communicate() has returned, so the process has exited
    return subprocess.CompletedProcess(
        list(argv), proc.returncode, stdout.decode(errors="replace"), stderr.decode(errors="replace")
    )
