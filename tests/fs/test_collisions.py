import getpass
import os
import subprocess
import threading

import pytest

from swiftdm.core.errors import ErrorCode, from_os_error
from swiftdm.fs.collisions import Reservations, finalize_move
from swiftdm.fs.names import long_path


def _write(p, data=b"x"):
    with open(long_path(str(p)), "wb") as f:
        f.write(data)


def test_reservations_unique_and_case_insensitive(tmp_path):
    r = Reservations()
    a = r.reserve("j1", str(tmp_path), "File.bin")
    b = r.reserve("j2", str(tmp_path), "file.BIN")
    c = r.reserve("j3", str(tmp_path), "file.bin")
    assert a == "File.bin" and b == "file (1).BIN" and c == "file (2).bin"
    assert r.reserve("j1", str(tmp_path), "File.bin") == "File.bin"     # idempotent for the owner
    r.release("j2")
    assert r.reserve("j4", str(tmp_path), "file.bin") == "file (1).bin"


def test_reservation_skips_existing_files(tmp_path):
    _write(tmp_path / "a.zip")
    _write(tmp_path / "a (1).zip")
    assert Reservations().reserve("j", str(tmp_path), "a.zip") == "a (2).zip"


def test_adopt_conflict(tmp_path):
    r = Reservations()
    r.reserve("j1", str(tmp_path), "a.bin")
    assert r.adopt("j2", str(tmp_path), "A.BIN") is False
    assert r.adopt("j1", str(tmp_path), "a.bin") is True


def test_parallel_reservations_never_collide(tmp_path):
    r = Reservations()
    names, lock = [], threading.Lock()

    def go(i):
        n = r.reserve(f"j{i}", str(tmp_path), "same.bin")
        with lock:
            names.append(n.casefold())

    ts = [threading.Thread(target=go, args=(i,)) for i in range(50)]
    [t.start() for t in ts]
    [t.join() for t in ts]
    assert len(set(names)) == 50


def test_finalize_race_target_created_after_reservation(tmp_path):
    r = Reservations()
    src = tmp_path / ".tmp.part"
    _write(src, b"new")
    r.reserve("j", str(tmp_path), "out.bin")
    _write(tmp_path / "out.bin", b"other process")          # appears between reserve and move
    final = finalize_move(str(src), str(tmp_path), "out.bin", "j", r)
    assert os.path.basename(final) == "out (1).bin"
    assert (tmp_path / "out.bin").read_bytes() == b"other process"
    assert (tmp_path / "out (1).bin").read_bytes() == b"new"


def test_finalize_overwrite_policy(tmp_path):
    src = tmp_path / ".t"
    _write(src, b"new")
    _write(tmp_path / "o.bin", b"old")
    final = finalize_move(str(src), str(tmp_path), "o.bin", "j", Reservations(), overwrite=True)
    assert final.endswith("o.bin") and (tmp_path / "o.bin").read_bytes() == b"new"


def test_finalize_never_overwrites_read_only_file(tmp_path):
    src = tmp_path / ".t"
    _write(src, b"new")
    ro = tmp_path / "ro.bin"
    _write(ro, b"keep")
    os.chmod(ro, 0o444)
    try:
        final = finalize_move(str(src), str(tmp_path), "ro.bin", "j", Reservations())
        assert final.endswith("ro (1).bin") and ro.read_bytes() == b"keep"
    finally:
        os.chmod(ro, 0o666)


def test_long_paths_work(tmp_path):
    deep = tmp_path
    for i in range(12):
        deep = deep / ("d" * 25 + str(i))
    os.makedirs(long_path(str(deep)), exist_ok=True)
    src = deep / ".t"
    _write(src, b"ok")
    final = finalize_move(str(src), str(deep), "f" * 60 + ".bin", "j", Reservations())
    assert len(final) > 300
    with open(long_path(final), "rb") as f:
        assert f.read() == b"ok"


@pytest.fixture
def denied_dir(tmp_path):
    d = tmp_path / "denied"
    d.mkdir()
    user = getpass.getuser()
    subprocess.run(["icacls", str(d), "/deny", f"{user}:(OI)(CI)(WD,AD)"], check=True, capture_output=True)
    yield d
    subprocess.run(["icacls", str(d), "/remove:d", user], capture_output=True)


def test_permission_denied_is_reported_with_path(tmp_path, denied_dir):
    src = tmp_path / ".t"
    _write(src)
    with pytest.raises(PermissionError) as ei:
        finalize_move(str(src), str(denied_dir), "x.bin", "j", Reservations())
    err = from_os_error(ei.value, str(denied_dir)).info
    assert err.code == ErrorCode.PERMISSION_DENIED and str(denied_dir) in err.message
    assert src.exists()                                       # nothing lost, no silent fallback folder


def test_manager_job_into_denied_folder_fails_without_fallback(tmp_path, denied_dir):
    from swiftdm.config.settings import Settings
    from swiftdm.core.models import JobSpec, JobState
    from swiftdm.store.db import Store
    from tests.support.fake_engine import FakeEngine, make_manager
    m = make_manager(Store(tmp_path / "s.db"), FakeEngine(), Settings().normalized())
    try:
        jid = m.submit(JobSpec(url="http://fake.test/a.bin?size=10", dest_dir=str(denied_dir)))
        snap = m.wait_for(jid, JobState.FAILED, 10)
        assert snap.state == JobState.FAILED
        assert snap.error.code == ErrorCode.PERMISSION_DENIED and str(denied_dir) in snap.error.message
        assert not os.listdir(os.getcwd()) or not any(f.startswith("a") and f.endswith(".bin") for f in os.listdir(os.getcwd()))
    finally:
        m.shutdown(2)
