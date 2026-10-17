import os
import time

import pytest

from swiftdm.config.settings import Settings
from swiftdm.core.errors import ErrorCode
from swiftdm.core.models import Job, JobSpec, JobState as S
from swiftdm.store.db import Store
from tests.support.fake_engine import FakeEngine, make_manager


@pytest.fixture
def env(tmp_path):
    settings = Settings(max_concurrent=2).normalized()
    store = Store(tmp_path / "state.db")
    eng = FakeEngine()
    dest = tmp_path / "dl"
    dest.mkdir()
    managers = []

    def mk(**kw):
        m = make_manager(store, eng, settings, retry_base=0.05, **kw)
        managers.append(m)
        return m

    yield type("E", (), {"settings": settings, "store": store, "eng": eng, "dest": str(dest), "mk": mk,
                         "tmp": tmp_path})
    for m in managers:
        m.shutdown(1)


def spec(env, q="", name="a.bin"):
    return JobSpec(url=f"http://fake.test/{name}?{q}", dest_dir=env.dest)


def test_happy_path_completes_with_file_in_place(env):
    m = env.mk()
    jid = m.submit(spec(env, "size=999&steps=3"))
    snap = m.wait_for(jid, S.COMPLETED, 10)
    assert snap.state == S.COMPLETED
    assert snap.final_path == os.path.join(env.dest, "a.bin")
    assert os.path.getsize(snap.final_path) == 999
    assert [f for f in os.listdir(env.dest)] == ["a.bin"]          # no temp leftovers
    assert env.store.load_job(jid).state == S.COMPLETED


def test_existing_file_is_never_overwritten(env):
    open(os.path.join(env.dest, "a.bin"), "wb").write(b"keep me")
    m = env.mk()
    jid = m.submit(spec(env, "size=10"))
    snap = m.wait_for(jid, S.COMPLETED, 10)
    assert snap.final_path.endswith("a (1).bin")
    assert open(os.path.join(env.dest, "a.bin"), "rb").read() == b"keep me"


def test_concurrent_same_name_jobs_get_distinct_files(env):
    m = env.mk()
    ids = [m.submit(spec(env, "size=100&steps=4&delay=0.02")) for _ in range(4)]
    finals = {m.wait_for(i, S.COMPLETED, 15).final_path for i in ids}
    assert len(finals) == 4


def test_pause_resume(env):
    m = env.mk()
    jid = m.submit(spec(env, "hang=1"))
    m.wait_for(jid, S.DOWNLOADING, 5)
    m.pause(jid)
    assert m.wait_for(jid, S.PAUSED, 5).reason == "paused"
    m.resume(jid)
    assert m.wait_for(jid, S.DOWNLOADING, 5).state == S.DOWNLOADING


def test_cancel_deletes_partial_data(env):
    m = env.mk()
    jid = m.submit(spec(env, "hang=1"))
    m.wait_for(jid, S.DOWNLOADING, 5)
    m.cancel(jid)
    assert m.wait_for(jid, S.CANCELLED, 5).state == S.CANCELLED
    assert os.listdir(env.dest) == []


def test_retryable_failure_retries_then_succeeds(env):
    m = env.mk()
    jid = m.submit(spec(env, "fail=network&times=2&size=10"))
    snap = m.wait_for(jid, S.COMPLETED, 10)
    assert snap.state == S.COMPLETED and env.eng.failures[jid] == 3


def test_retry_budget_exhausted_fails(env):
    m = env.mk()
    jid = m.submit(spec(env, "fail=network"))
    snap = m.wait_for(jid, S.FAILED, 10)
    assert snap.state == S.FAILED and snap.error.code == ErrorCode.NETWORK and snap.attempts == 3


def test_non_retryable_fails_immediately_and_user_can_retry(env):
    m = env.mk()
    jid = m.submit(spec(env, "fail=forbidden&times=1&size=5"))
    snap = m.wait_for(jid, S.FAILED, 5)
    assert snap.attempts == 1 and env.eng.failures[jid] == 1
    m.retry(jid)
    assert m.wait_for(jid, S.COMPLETED, 5).state == S.COMPLETED


def test_disk_full_pauses_resumably(env):
    m = env.mk()
    jid = m.submit(spec(env, "fail=diskfull&times=1&size=5"))
    snap = m.wait_for(jid, S.PAUSED, 5)
    assert snap.reason == "disk_full" and snap.error.code == ErrorCode.DISK_FULL
    m.resume(jid)
    assert m.wait_for(jid, S.COMPLETED, 5).state == S.COMPLETED


def test_concurrency_limit_respected(env):
    m = env.mk()
    ids = [m.submit(spec(env, "size=100&steps=10&delay=0.03", f"f{i}.bin")) for i in range(7)]
    peak = 0
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        peak = max(peak, m.running_count())
        if all(m.get(i) and m.get(i).state == S.COMPLETED for i in ids):
            break
        time.sleep(0.005)
    assert all(m.get(i).state == S.COMPLETED for i in ids)
    assert env.eng.max_concurrent_seen <= 2 and peak <= 2


def test_priority_order(env):
    env.settings.max_concurrent = 1
    m = env.mk()
    blocker = m.submit(spec(env, "hang=1", "block.bin"))
    m.wait_for(blocker, S.DOWNLOADING, 5)
    low = m.submit(spec(env, "size=1", "low.bin"))
    high = m.submit(JobSpec(url="http://fake.test/high.bin?size=1", dest_dir=env.dest, priority=5))
    m.sync()
    m.cancel(blocker)
    m.wait_for(low, S.COMPLETED, 5)
    assert m.get(high).state == S.COMPLETED
    assert env.store.load_job(high).completed_at <= env.store.load_job(low).completed_at


def test_remove_running_job_cancels_then_removes(env):
    m = env.mk()
    jid = m.submit(spec(env, "hang=1"))
    m.wait_for(jid, S.DOWNLOADING, 5)
    m.remove(jid)
    deadline = time.monotonic() + 5
    while m.get(jid) is not None and time.monotonic() < deadline:
        time.sleep(0.02)
    assert m.get(jid) is None and env.store.load_job(jid) is None and os.listdir(env.dest) == []


def test_submit_rejects_bad_urls(env):
    m = env.mk()
    for bad in ("ftp://x/y", "file:///C:/Windows/win.ini", "javascript:alert(1)", "not a url", ""):
        with pytest.raises(ValueError):
            m.submit(JobSpec(url=bad, dest_dir=env.dest))


def test_shutdown_marks_running_jobs_interrupted(env):
    m = env.mk()
    jid = m.submit(spec(env, "hang=1"))
    m.wait_for(jid, S.DOWNLOADING, 5)
    m.shutdown(3)
    job = env.store.load_job(jid)
    assert job.state == S.PAUSED and job.reason == "interrupted"


@pytest.mark.parametrize("persisted,auto,expected", [
    (S.STARTING, False, S.PAUSED), (S.DOWNLOADING, False, S.PAUSED), (S.POSTPROCESSING, False, S.PAUSED),
    (S.RETRY_WAIT, False, S.PAUSED), (S.PAUSING, False, S.PAUSED), (S.DOWNLOADING, True, S.QUEUED),
    (S.CANCELLING, False, S.CANCELLED), (S.QUEUED, False, S.QUEUED), (S.COMPLETED, False, S.COMPLETED),
])
def test_recovery_table(env, persisted, auto, expected):
    env.settings.auto_resume = auto
    env.settings.max_concurrent = 0 if expected == S.QUEUED else 2
    job = Job(id="f" * 32, spec=spec(env, "hang=1"), state=persisted, kind="http")
    env.store.save_job(job)
    m = env.mk()
    snap = m.get(job.id)
    if expected == S.QUEUED:
        assert snap.state == S.QUEUED
    else:
        assert snap.state == expected, snap


def test_recovery_refinalizes_interrupted_finalize(env):
    artifact = os.path.join(env.dest, ".a.bin.ffffffff.sdmpart")
    open(artifact, "wb").write(b"z" * 42)
    job = Job(id="f" * 32, spec=spec(env), state=S.FINALIZING, kind="http", reserved_name="a.bin",
              artifact_path=artifact, total_bytes=42, size_verified=True)
    env.store.save_job(job)
    m = env.mk()
    snap = m.get(job.id)
    assert snap.state == S.COMPLETED and os.path.getsize(snap.final_path) == 42


def test_recovery_finalize_with_missing_artifact_fails_honestly(env):
    job = Job(id="f" * 32, spec=spec(env), state=S.FINALIZING, kind="http", reserved_name="a.bin",
              artifact_path=os.path.join(env.dest, "gone.sdmpart"))
    env.store.save_job(job)
    assert env.mk().get(job.id).state == S.FAILED
