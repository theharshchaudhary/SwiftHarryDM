"""Run a tool as a cancellable process tree, streaming its output line by line."""
from __future__ import annotations

import collections
import os
import subprocess
import threading
import time
from dataclasses import dataclass, field
from typing import Callable, Optional

from ...core.cancel import CancelToken
from . import jobobject


@dataclass
class ProcResult:
    returncode: Optional[int]
    stderr_tail: list = field(default_factory=list)
    stopped: bool = False
    timed_out: bool = False


def _pump(stream, sink: Callable[[str], None]):
    try:
        for raw in iter(stream.readline, b""):
            sink(raw.decode("utf-8", "replace").rstrip("\r\n"))
    except (OSError, ValueError):
        pass
    finally:
        try:
            stream.close()
        except OSError:
            pass


def run(cmd: list, cancel: CancelToken, on_stdout: Callable[[str], None],
        on_stderr: Optional[Callable[[str], None]] = None, timeout: Optional[float] = None,
        env_extra: Optional[dict] = None) -> ProcResult:
    env = dict(os.environ)
    env.update({"PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"})
    env.update(env_extra or {})
    tail: collections.deque = collections.deque(maxlen=200)

    def err_sink(line):
        tail.append(line)
        if on_stderr:
            on_stderr(line)

    tree = jobobject.spawn(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    readers = [threading.Thread(target=_pump, args=(tree.popen.stdout, on_stdout), daemon=True),
               threading.Thread(target=_pump, args=(tree.popen.stderr, err_sink), daemon=True)]
    for t in readers:
        t.start()
    stopped = timed_out = False
    deadline = time.monotonic() + timeout if timeout else None
    try:
        while tree.popen.poll() is None:
            if cancel.wait(0.1):
                stopped = True
                tree.kill_tree()
                break
            if deadline and time.monotonic() > deadline:
                timed_out = True
                tree.kill_tree()
                break
        for t in readers:
            t.join(5)
    finally:
        tree.close()
    return ProcResult(tree.popen.returncode, list(tail), stopped, timed_out)
