"""P1 acceptance: forced process kills (TerminateProcess) mid-transition never
corrupt the database, and recovery always yields valid, worker-free states."""
import os
import random
import subprocess
import sys
import time
from pathlib import Path

import pytest

from swiftdm.config.settings import Settings
from swiftdm.core.models import JobState as S
from swiftdm.store.db import Store
from tests.support.fake_engine import FakeEngine, make_manager

ROOT = Path(__file__).resolve().parents[2]
KILLS = int(os.environ.get("SWIFTDM_KILLS", "200"))


@pytest.mark.slow
def test_forced_kills_never_corrupt_state(tmp_path):
    db = tmp_path / "state.db"
    dest = tmp_path / "dl"
    dest.mkdir()
    rng = random.Random(1234)
    seen_states = set()
    for i in range(KILLS):
        p = subprocess.Popen([sys.executable, str(ROOT / "tests" / "support" / "kill_child.py"), str(db), str(dest),
                              str(i)], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        assert p.stdout.readline().strip() == b"ready"
        time.sleep(rng.uniform(0.02, 0.4))
        p.kill()                     # TerminateProcess: no cleanup, no atexit
        p.wait(10)
        store = Store(db)
        assert store.integrity_check() == "ok", f"corrupt after kill {i}"
        jobs = store.load_jobs()
        seen_states.update(j.state for j in jobs)
        m = make_manager(store, FakeEngine(), Settings(max_concurrent=0).normalized(), autostart=False)
        for snap in m.snapshot():
            assert snap.state not in (S.STARTING, S.DOWNLOADING, S.POSTPROCESSING, S.PAUSING, S.CANCELLING,
                                      S.FINALIZING), f"kill {i}: {snap.state} survived recovery"
            if snap.state == S.COMPLETED:
                assert os.path.isfile(snap.final_path)
        store.close()
    # The kills must actually have landed while jobs were active.
    assert seen_states & {S.DOWNLOADING, S.STARTING}, seen_states
