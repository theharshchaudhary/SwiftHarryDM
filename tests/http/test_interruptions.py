"""P4 acceptance: randomized interruptions (server faults + TerminateProcess of the
downloading process, then restart and resume) always produce SHA-correct files."""
import os
import random
import subprocess
import sys
import time
from pathlib import Path

import pytest

from swiftdm.core.models import Segment
from swiftdm.engines.http import planner
from tests.http.conftest import sha_file
from tests.support.testserver import MiB, Fault, Payload, Resource

ROOT = Path(__file__).resolve().parents[2]
RUNS = int(os.environ.get("SWIFTDM_INTERRUPT_RUNS", "100"))


def test_plan_tiles_exactly():
    for total in (1, 4 * MiB - 1, 4 * MiB, 10 * MiB + 7, 1 << 32):
        for n in (1, 3, 8, 16):
            segs = planner.plan(total, True, n)
            assert segs[0].start == 0 and segs[-1].end == total - 1
            assert all(a.end + 1 == b.start for a, b in zip(segs, segs[1:]))
    assert planner.plan(0, True, 8) == []
    assert planner.plan(None, False, 8) == [Segment(0, 0, -1)]


def test_split_point_bounds():
    assert planner.split_point(0, MiB) is None
    k = planner.split_point(1000, 1000 + 10 * MiB - 1)
    assert 1000 < k < 1000 + 10 * MiB - 1


def test_dynamic_split_happens_and_bytes_are_right(server, tmp_path):
    from tests.http.conftest import HttpEnv
    from swiftdm.core.models import JobState as S
    env = HttpEnv(tmp_path)
    env.settings.max_segments = 4
    try:
        p = Payload("split", 16 * MiB)
        # Segment 0 stalls after 512 KiB, so the other segments finish first and it must be split.
        stall = Fault(512 * 1024, kind="stall", stall_seconds=4,
                      when=lambda n, r: bool(r) and r.startswith("bytes=0-") and r != "bytes=0-0")
        res = Resource(p, etag='"s"', throttle_bps=4 * MiB, faults=[stall])
        jid = env.submit(server.add("/split.bin", res))
        seen_more = False
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            prog = env.m.progress(jid)
            if prog and len(prog.segments) > 4:
                seen_more = True
            snap = env.m.get(jid)
            if snap and snap.state in (S.COMPLETED, S.FAILED):
                break
            time.sleep(0.05)
        assert snap.state == S.COMPLETED and sha_file(snap.final_path) == p.sha256()
        assert seen_more, "no dynamic split observed"
    finally:
        env.m.shutdown(3)


def _run_child(db, dest, url, segs):
    return subprocess.Popen([sys.executable, str(ROOT / "tests" / "support" / "http_child.py"), str(db), str(dest),
                             url, str(segs)], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)


@pytest.mark.slow
def test_randomized_kill_and_fault_runs(server, tmp_path):
    rng = random.Random(20261017)
    kills_total = 0
    for run in range(RUNS):
        size = rng.choice([3 * MiB, 9 * MiB, 17 * MiB]) + rng.randint(0, 99999)
        p = Payload(f"run{run}", size)
        validators = rng.choice([{"etag": f'"r{run}"'}, {"last_modified": "Wed, 21 Oct 2025 07:28:00 GMT"}, {}])
        faults = []
        if rng.random() < 0.5:
            faults.append(Fault(rng.randint(64 * 1024, 2 * MiB), times=rng.randint(1, 4),
                                kind=rng.choice(["cut", "cut", "stall"]), stall_seconds=3,
                                when=lambda n, r: r != "bytes=0-0"))
        res = Resource(p, ranges=rng.random() > 0.1, throttle_bps=rng.choice([4 * MiB, 8 * MiB]),
                       faults=faults, **validators)
        url = server.add(f"/run{run}.bin", res)
        d = tmp_path / f"run{run}"
        d.mkdir()
        db = d / "state.db"
        dest = d / "dl"
        dest.mkdir()
        segs = rng.choice([1, 4, 8])
        kills = rng.randint(1, 3)
        result = None
        for attempt in range(kills + 1):
            child = _run_child(db, dest, url, segs)
            assert child.stdout.readline().strip() == "ready"
            if attempt < kills:
                time.sleep(rng.uniform(0.1, 1.2))
                if child.poll() is None:
                    child.kill()
                    kills_total += 1
                child.wait(10)
                if child.returncode == 0:
                    result = child.stdout.read()
                    break
                continue
            out, _ = child.communicate(timeout=120)
            result = out
        assert result and "result COMPLETED" in result, f"run {run}: {result!r}"
        final = result.split("result COMPLETED ", 1)[1].strip()
        assert sha_file(final) == p.sha256(), f"run {run}: corrupted file"
        assert [f for f in os.listdir(dest) if f.endswith(".sdmpart")] == []
    assert kills_total >= RUNS  # most runs were actually killed mid-download
