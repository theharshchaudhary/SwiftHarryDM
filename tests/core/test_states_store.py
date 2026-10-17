import sqlite3

import pytest

from swiftdm.core import states
from swiftdm.core.errors import ErrorCode, ErrorInfo
from swiftdm.core.models import ACTIVE_STATES, TERMINAL_STATES, Job, JobSpec, JobState as S, MediaOptions, Segment
from swiftdm.store.db import ReservationConflict, Store


def test_terminal_states_have_no_exits():
    for s in TERMINAL_STATES:
        assert states.TRANSITIONS[s] == set()


def test_every_state_reachable_from_queued():
    seen, todo = {S.QUEUED}, [S.QUEUED]
    while todo:
        for nxt in states.TRANSITIONS[todo.pop()]:
            if nxt not in seen:
                seen.add(nxt)
                todo.append(nxt)
    assert seen == set(S)


@pytest.mark.parametrize("src,dst,ok", [
    (S.QUEUED, S.STARTING, True), (S.STARTING, S.DOWNLOADING, True), (S.DOWNLOADING, S.FINALIZING, True),
    (S.FINALIZING, S.COMPLETED, True), (S.COMPLETED, S.QUEUED, False), (S.CANCELLED, S.QUEUED, False),
    (S.QUEUED, S.COMPLETED, False), (S.DOWNLOADING, S.COMPLETED, False), (S.PAUSED, S.DOWNLOADING, False),
    (S.FINALIZING, S.CANCELLING, False), (S.FAILED, S.QUEUED, True), (S.PAUSING, S.CANCELLING, True),
])
def test_specific_transitions(src, dst, ok):
    assert states.can(src, dst) is ok
    if not ok:
        with pytest.raises(states.InvalidTransition):
            states.check(src, dst)


def test_completed_only_via_finalizing():
    for src, dsts in states.TRANSITIONS.items():
        if S.COMPLETED in dsts:
            assert src == S.FINALIZING


def _job(i="a" * 32, dest="C:/d", name=None, state=S.QUEUED):
    return Job(id=i, spec=JobSpec(url="https://x.example/f.bin", dest_dir=dest, media=MediaOptions("720")),
               state=state, reserved_name=name)


def test_store_roundtrip(tmp_path):
    st = Store(tmp_path / "s.db")
    j = _job(name="f.bin")
    j.error = ErrorInfo(ErrorCode.NETWORK, "down", retryable=True)
    j.media = {"format_id": "137+140"}
    j.accept_ranges = True
    st.save_job(j)
    back = st.load_jobs()[0]
    assert back == j
    assert st.integrity_check() == "ok"


def test_reservation_unique_among_unfinished_jobs(tmp_path):
    st = Store(tmp_path / "s.db")
    st.save_job(_job("a" * 32, name="F.bin"))
    with pytest.raises(ReservationConflict):
        st.save_job(_job("b" * 32, name="f.BIN"))          # case-insensitive, same folder
    st.save_job(_job("c" * 32, dest="C:/other", name="f.bin"))
    done = _job("d" * 32, name="f.bin", state=S.COMPLETED)
    st.save_job(done)                                         # completed jobs don't hold reservations


def test_segment_split_is_atomic(tmp_path):
    st = Store(tmp_path / "s.db")
    st.save_job(_job())
    st.replace_segments("a" * 32, [Segment(0, 0, 999, 100)])
    st.split_segment("a" * 32, 0, 499, Segment(1, 500, 999, 0))
    assert [(s.start, s.end, s.committed) for s in st.load_segments("a" * 32)] == [(0, 499, 100), (500, 999, 0)]
    with pytest.raises(sqlite3.IntegrityError):
        st.split_segment("a" * 32, 0, 249, Segment(1, 250, 499, 0))  # duplicate idx -> rolled back
    assert [(s.start, s.end) for s in st.load_segments("a" * 32)] == [(0, 499), (500, 999)]


def test_refuses_newer_schema(tmp_path):
    p = tmp_path / "s.db"
    Store(p).close()
    con = sqlite3.connect(p)
    con.execute("UPDATE schema_version SET v=99")
    con.commit()
    con.close()
    with pytest.raises(RuntimeError, match="newer"):
        Store(p)


def test_backup_copy(tmp_path):
    st = Store(tmp_path / "s.db")
    st.save_job(_job())
    st.backup(tmp_path / "b.db")
    assert len(Store(tmp_path / "b.db").load_jobs()) == 1
