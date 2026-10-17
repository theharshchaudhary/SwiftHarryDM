"""Repeatable download benchmark for the native HTTP engine.

1. Throughput: a local server throttled per connection (default 2 MiB/s, like a
   per-connection-limited CDN) serves a file; compare 1 segment vs N segments.
2. Memory: download a large deterministic virtual file (default 4 GiB) and
   record peak RSS of this process (server + client) while it runs.

Results are written as JSON. Usage:
    python bench/bench_segments.py --out bench/results.json [--size-gib 4] [--throttle-mib 2]
"""
import argparse
import json
import os
import platform
import shutil
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import psutil  # noqa: E402  (dev dependency)

from swiftdm.config.settings import Settings  # noqa: E402
from swiftdm.core.manager import JobManager  # noqa: E402
from swiftdm.core.models import JobSpec, JobState as S  # noqa: E402
from swiftdm.engines.http.transfer import HttpEngine  # noqa: E402
from swiftdm.store.db import Store  # noqa: E402
from tests.support.testserver import MiB, Payload, Resource, TestServer  # noqa: E402


def download(url, segments, workdir, timeout=3600):
    settings = Settings(max_concurrent=1, max_segments=segments).normalized()
    store = Store(Path(workdir) / f"state-{segments}-{time.time_ns()}.db")
    m = JobManager(store, {"http": HttpEngine(store)}, router=lambda s: "http", settings=lambda: settings)
    dest = Path(workdir) / f"dl-{segments}-{time.time_ns()}"
    dest.mkdir()
    t0 = time.perf_counter()
    jid = m.submit(JobSpec(url=url, dest_dir=str(dest)))
    snap = m.wait_for(jid, {S.COMPLETED, S.FAILED}, timeout)
    elapsed = time.perf_counter() - t0
    m.shutdown(3)
    store.close()
    return snap, elapsed


def sha(path):
    import hashlib
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(8 << 20), b""):
            h.update(b)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="bench/results.json")
    ap.add_argument("--throttle-mib", type=float, default=2.0)
    ap.add_argument("--throughput-mib", type=int, default=64)
    ap.add_argument("--segments", type=int, default=8)
    ap.add_argument("--size-gib", type=float, default=4.0)
    args = ap.parse_args()
    srv = TestServer()
    work = tempfile.mkdtemp(prefix="sdmbench")
    results = {"machine": {"python": sys.version.split()[0], "platform": platform.platform(),
                           "cpus": os.cpu_count()}, "runs": []}
    try:
        p = Payload("bench", args.throughput_mib * MiB)
        url = srv.add("/t.bin", Resource(p, etag='"b"', throttle_bps=int(args.throttle_mib * MiB)))
        timings = {}
        for segs in (1, args.segments):
            snap, secs = download(url, segs, work)
            assert snap.state == S.COMPLETED, snap
            ok = sha(snap.final_path) == p.sha256()
            timings[segs] = secs
            results["runs"].append({"test": "throughput", "segments": segs, "bytes": p.size, "seconds": round(secs, 2),
                                    "mib_per_s": round(p.size / MiB / secs, 2), "sha_ok": ok})
            os.remove(snap.final_path)
        results["speedup"] = round(timings[1] / timings[args.segments], 2)

        size = int(args.size_gib * 1024 * MiB)
        big = Payload("big", size)
        url = srv.add("/big.bin", Resource(big, etag='"big"'))
        proc = psutil.Process()
        peak = {"rss": proc.memory_info().rss}
        stop = threading.Event()

        def sample():
            while not stop.wait(0.2):
                peak["rss"] = max(peak["rss"], proc.memory_info().rss)

        threading.Thread(target=sample, daemon=True).start()
        snap, secs = download(url, args.segments, work, timeout=7200)
        stop.set()
        assert snap.state == S.COMPLETED, snap
        got = os.path.getsize(snap.final_path)
        ok = sha(snap.final_path) == big.sha256()
        results["runs"].append({"test": "large_file_memory", "segments": args.segments, "bytes": got,
                                "seconds": round(secs, 1), "mib_per_s": round(got / MiB / secs, 1),
                                "peak_rss_mib": round(peak["rss"] / MiB, 1), "sha_ok": ok})
    finally:
        srv.close()
        shutil.rmtree(work, ignore_errors=True)
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
