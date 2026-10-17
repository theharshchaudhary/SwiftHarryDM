"""A scriptable engine for core tests. Behaviour comes from the URL query, e.g.

    http://fake.test/a.bin?size=1000&steps=5&delay=0.01
    http://fake.test/a.bin?fail=network&times=2      (retryable failure twice, then success)
    http://fake.test/a.bin?fail=forbidden            (non-retryable)
    http://fake.test/a.bin?fail=diskfull
    http://fake.test/a.bin?hang=1                    (runs until stopped)
"""
import os
import threading
import time
from urllib.parse import parse_qs, urlsplit

from swiftdm.core.cancel import StopRequested
from swiftdm.core.errors import ErrorCode, SdmError
from swiftdm.core.models import ProgressSnapshot
from swiftdm.engines.base import Completed, ProbeResult


class FakeEngine:
    name = "http"

    def __init__(self):
        self.lock = threading.Lock()
        self.failures = {}
        self.concurrent = 0
        self.max_concurrent_seen = 0
        self.runs = 0

    @staticmethod
    def opts(job):
        q = parse_qs(urlsplit(job.spec.url).query)
        return {k: v[0] for k, v in q.items()}

    def probe(self, job, ctx):
        o = self.opts(job)
        name = os.path.basename(urlsplit(job.spec.url).path) or "file.bin"
        return ProbeResult(updates={"total_bytes": int(o.get("size", 1000)), "final_url": job.spec.url},
                           desired_name=name)

    def _temp(self, job):
        return os.path.join(job.dest_dir, f".{job.reserved_name}.{job.id[:8]}.sdmpart")

    def run(self, job, ctx):
        o = self.opts(job)
        with self.lock:
            self.concurrent += 1
            self.runs += 1
            self.max_concurrent_seen = max(self.max_concurrent_seen, self.concurrent)
        try:
            fail = o.get("fail")
            if fail:
                times = int(o.get("times", 10**9))
                with self.lock:
                    n = self.failures.get(job.id, 0)
                    self.failures[job.id] = n + 1
                if n < times:
                    if fail == "network":
                        raise SdmError(ErrorCode.NETWORK, "network down", retryable=True)
                    if fail == "diskfull":
                        raise SdmError(ErrorCode.DISK_FULL, "disk full")
                    raise SdmError(ErrorCode.FORBIDDEN, "forbidden")
            size = int(o.get("size", 1000))
            steps = int(o.get("steps", 3))
            delay = float(o.get("delay", 0.0))
            temp = self._temp(job)
            ctx.update(temp_path=temp)
            os.makedirs(job.dest_dir, exist_ok=True)
            with open(temp, "wb") as f:
                i = 0
                while True:
                    ctx.cancel.raise_if_set()
                    if o.get("hang"):
                        ctx.cancel.wait(0.01)
                        continue
                    if i >= steps:
                        break
                    f.write(b"x" * (size // steps))
                    i += 1
                    ctx.report(ProgressSnapshot(job.id, f.tell(), size, None, None))
                    if delay:
                        ctx.cancel.wait(delay)
                f.write(b"x" * (size - f.tell()))
            return Completed(temp, size)
        finally:
            with self.lock:
                self.concurrent -= 1

    def discard(self, job):
        if job.temp_path and os.path.exists(job.temp_path):
            os.remove(job.temp_path)

    def cleanup(self, job):
        pass


def make_manager(store, engine, settings_obj, **kw):
    from swiftdm.core.manager import JobManager
    return JobManager(store, {"http": engine}, router=lambda spec: "http", settings=lambda: settings_obj, **kw)
