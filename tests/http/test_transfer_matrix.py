"""P3/P4 fault matrix: every case ends with the right bytes or the specified error."""
import time

import pytest

from swiftdm.core.errors import ErrorCode, SdmError
from swiftdm.core.models import JobState as S
from swiftdm.engines.http.probe import next_redirect
from tests.http.conftest import sha_file
from tests.support.testserver import MiB, Fault, Payload, Resource


def ok(snap, payload):
    assert snap.state == S.COMPLETED, (snap.state, snap.error)
    assert sha_file(snap.final_path) == payload.sha256()
    return snap


def test_small_file_keeps_name_and_bytes(server, henv):
    p = Payload("small", 1 * MiB)
    snap = ok(henv.run(server.add("/file.bin", Resource(p))), p)
    assert snap.final_path.endswith("file.bin")          # T1: no ".unknown_video"
    assert henv.files() == ["file.bin"]


def test_segmented_download_uses_parallel_ranges(server, henv):
    p = Payload("seg", 24 * MiB)
    ok(henv.run(server.add("/big.bin", Resource(p, etag='"v1"'))), p)
    ranges = {rng for _, _, rng, _ in server.requests_for("/big.bin") if rng and rng != "bytes=0-0"}
    assert len(ranges) >= 8


def test_server_without_range_support(server, henv):
    p = Payload("norange", 9 * MiB)
    ok(henv.run(server.add("/nr.bin", Resource(p, ranges=False))), p)
    assert all(rng in (None, "bytes=0-0") for _, _, rng, _ in server.requests_for("/nr.bin"))


def test_server_ignoring_range_header(server, henv):
    p = Payload("ign", 6 * MiB)
    ok(henv.run(server.add("/ign.bin", Resource(p, range_bug="ignore"))), p)


def test_cut_mid_transfer_resumes_with_range(server, henv):
    p = Payload("cut", 10 * MiB)
    res = Resource(p, etag='"e"', faults=[Fault(3 * MiB, when=lambda n, rng: rng and rng.startswith("bytes=0-"), times=2)])
    ok(henv.run(server.add("/cut.bin", res)), p)


def test_stalled_connection_times_out_and_resumes(server, henv):
    p = Payload("stall", 6 * MiB)
    res = Resource(p, etag='"e"', faults=[Fault(1 * MiB, kind="stall", stall_seconds=6, times=1)])
    ok(henv.run(server.add("/stall.bin", res), timeout=60), p)


def test_5xx_burst_then_success(server, henv):
    p = Payload("5xx", 2 * MiB)
    ok(henv.run(server.add("/5xx.bin", Resource(p, statuses=[503, 502]))), p)


def test_429_honours_retry_after(server, henv):
    p = Payload("429", 1 * MiB)
    t0 = time.monotonic()
    ok(henv.run(server.add("/429.bin", Resource(p, statuses=[429], retry_after="1"))), p)
    assert time.monotonic() - t0 >= 1.0


def test_wrong_content_range_start_falls_back_safely(server, henv):
    p = Payload("wrong", 6 * MiB)
    ok(henv.run(server.add("/wr.bin", Resource(p, range_bug="wrong_start"))), p)


def test_wrong_content_range_total_never_completes_wrongly(server, henv):
    p = Payload("wt", 6 * MiB)
    snap = henv.run(server.add("/wt.bin", Resource(p, range_bug="wrong_total")), timeout=90)
    if snap.state == S.COMPLETED:
        assert sha_file(snap.final_path) == p.sha256()
    else:
        assert snap.state == S.FAILED and "wt.bin" not in henv.files()


def test_etag_change_mid_download_restarts_with_new_content(server, henv):
    old, new = Payload("old", 12 * MiB), Payload("new", 12 * MiB)
    res = Resource(old, etag='"old"', throttle_bps=8 * MiB,
                   replace_after=3, replacement=Resource(new, etag='"new"'))
    snap = ok(henv.run(server.add("/chg.bin", res), timeout=90), new)
    assert any(ir == '"old"' for _, _, _, ir in server.requests_for("/chg.bin"))


@pytest.mark.parametrize("validators", [{"etag": '"strong"'}, {"last_modified": "Wed, 21 Oct 2025 07:28:00 GMT"},
                                        {"etag": 'W/"weak"'}, {}])
def test_pause_resume_with_each_validator_kind(server, henv, validators):
    p = Payload("pr", 10 * MiB)
    url = server.add("/pr.bin", Resource(p, throttle_bps=384 * 1024, **validators))
    jid = henv.submit(url)
    henv.m.wait_for(jid, S.DOWNLOADING, 10)
    time.sleep(1.2)
    henv.m.pause(jid)
    assert henv.m.wait_for(jid, S.PAUSED, 5).state == S.PAUSED
    committed = sum(s.committed for s in henv.store.load_segments(jid))
    assert committed > 0
    n_before = len(server.requests_for("/pr.bin"))
    henv.m.resume(jid)
    snap = ok(henv.m.wait_for(jid, {S.COMPLETED, S.FAILED}, 60), p)
    resumed = server.requests_for("/pr.bin")[n_before:]
    starts = [int(r.split("=")[1].split("-")[0]) for _, _, r, _ in resumed if r and r != "bytes=0-0"]
    assert starts and min(starts) > 0                    # nothing re-downloaded from byte 0
    expected_if_range = validators.get("etag") if validators.get("etag", "").startswith('"') else validators.get("last_modified")
    assert all(ir == expected_if_range for _, _, r, ir in resumed if r and r != "bytes=0-0")


def test_no_validator_content_change_detected_by_overlap(server, henv):
    a, b = Payload("same-size-a", 8 * MiB), Payload("same-size-b", 8 * MiB)
    url = server.add("/nv.bin", Resource(a, throttle_bps=384 * 1024))
    jid = henv.submit(url)
    henv.m.wait_for(jid, S.DOWNLOADING, 10)
    time.sleep(1.2)
    henv.m.pause(jid)
    henv.m.wait_for(jid, S.PAUSED, 5)
    server.add("/nv.bin", Resource(b))                  # same size, no validators, different bytes
    henv.m.resume(jid)
    ok(henv.m.wait_for(jid, {S.COMPLETED, S.FAILED}, 60), b)


def test_chunked_unknown_length(server, henv):
    p = Payload("chunk", 3 * MiB)
    snap = ok(henv.run(server.add("/ch.bin", Resource(p, chunked=True, ranges=False))), p)
    assert henv.store.load_job(snap.id).size_verified is False


def test_gzip_content_encoding_is_decoded(server, henv):
    p = Payload("gz", 2 * MiB)
    ok(henv.run(server.add("/gz.bin", Resource(p, gzip=True))), p)


def test_redirect_chain_followed(server, henv):
    p = Payload("redir", 2 * MiB)
    server.add("/final.bin", Resource(p))
    server.add("/r2", Resource(p, redirect_to="/final.bin", redirect_status=307))
    server.add("/r1", Resource(p, redirect_to=server.base + "/r2", redirect_status=301))
    snap = ok(henv.run(server.base + "/r1"), p)
    assert snap.final_path.endswith("final.bin")


def test_redirect_loop_fails(server, henv):
    p = Payload("x", 10)
    server.add("/la", Resource(p, redirect_to="/lb"))
    server.add("/lb", Resource(p, redirect_to="/la"))
    snap = henv.run(server.base + "/la")
    assert snap.state == S.FAILED and "redirect" in snap.error.message.lower()


@pytest.mark.parametrize("cur,loc,ok_", [
    ("https://a/x", "http://b/y", False), ("https://a/x", "https://b/y", True), ("http://a/x", "https://b/y", True),
    ("http://a/x", "/rel", True), ("http://a/x", "file:///C:/x", False), ("http://a/x", "ftp://h/f", False),
])
def test_redirect_policy(cur, loc, ok_):
    if ok_:
        assert next_redirect(cur, loc)
    else:
        with pytest.raises(SdmError) as e:
            next_redirect(cur, loc)
        assert e.value.info.code == ErrorCode.REDIRECT_REFUSED


@pytest.mark.parametrize("status,code", [(404, ErrorCode.HTTP_STATUS), (403, ErrorCode.FORBIDDEN),
                                         (401, ErrorCode.FORBIDDEN)])
def test_client_errors_fail_without_retry(server, henv, status, code):
    url = server.add(f"/e{status}", Resource(Payload("e", 10), statuses=[status] * 10))
    snap = henv.run(url)
    assert snap.state == S.FAILED and snap.error.code == code and snap.attempts == 1


def test_html_page_is_not_saved_as_a_file(server, henv):
    url = server.add("/page", Resource(Payload("html", 100), content_type="text/html; charset=utf-8"))
    snap = henv.run(url)
    assert snap.state == S.FAILED and snap.error.code == ErrorCode.NOT_A_FILE and henv.files() == []


def test_repeated_cuts_with_progress_still_complete_correctly(server, henv):
    p = Payload("cuts", 8 * MiB)
    res = Resource(p, etag='"c"', faults=[Fault(256 * 1024, when=lambda n, rng: rng != "bytes=0-0", times=-1)])
    ok(henv.run(server.add("/cuts.bin", res), timeout=120), p)


def test_persistent_failure_never_completes(server, henv):
    """T7: a transfer that keeps breaking without progress must end FAILED, never COMPLETED."""
    p = Payload("dead", 8 * MiB)
    res = Resource(p, etag='"d"', faults=[Fault(0, when=lambda n, rng: rng != "bytes=0-0", times=-1)])
    snap = henv.run(server.add("/dead.bin", res), timeout=120)
    assert snap.state == S.FAILED and snap.error.retryable
    assert "dead.bin" not in henv.files()


def test_same_name_different_content(server, henv):
    """T5b: the second file gets its own name and its own bytes."""
    a, b = Payload("A", 1 * MiB), Payload("B", 1 * MiB)
    s1 = ok(henv.run(server.add("/v1/data.bin", Resource(a))), a)
    s2 = ok(henv.run(server.add("/v2/data.bin", Resource(b))), b)
    assert s1.final_path.endswith("data.bin") and s2.final_path.endswith("data (1).bin")


def test_content_disposition_traversal_stays_in_folder(server, henv):
    p = Payload("cd", 1000)
    snap = ok(henv.run(server.add("/get", Resource(p, content_disposition='attachment; filename="..\\\\..\\\\evil.exe"'))), p)
    assert snap.final_path == henv.dest + "\\evil.exe"


def test_zero_byte_file(server, henv):
    p = Payload("zero", 0)
    snap = henv.run(server.add("/empty.txt", Resource(p)))
    assert snap.state == S.COMPLETED and snap.total == 0


def test_pause_stops_server_bytes_quickly(server, henv):
    p = Payload("throttle", 40 * MiB)
    res = Resource(p, etag='"t"', throttle_bps=2 * MiB)
    jid = henv.submit(server.add("/t.bin", res))
    henv.m.wait_for(jid, S.DOWNLOADING, 10)
    time.sleep(1.0)
    t0 = time.monotonic()
    henv.m.pause(jid)
    henv.m.wait_for(jid, S.PAUSED, 5)
    assert time.monotonic() - t0 < 2.0
    sent, n_req = res.bytes_sent, len(server.requests_for("/t.bin"))
    time.sleep(1.5)
    assert len(server.requests_for("/t.bin")) == n_req       # no new requests after pausing
    # The server may still push a couple of blocks into socket buffers before it sees the close.
    assert res.bytes_sent - sent <= 8 * 2 * 64 * 1024
    sent = res.bytes_sent
    time.sleep(1.0)
    assert res.bytes_sent == sent                           # and then nothing at all


def test_cancel_removes_partial_file(server, henv):
    p = Payload("cancel", 30 * MiB)
    jid = henv.submit(server.add("/c.bin", Resource(p, throttle_bps=2 * MiB)))
    henv.m.wait_for(jid, S.DOWNLOADING, 10)
    time.sleep(0.8)
    henv.m.cancel(jid)
    assert henv.m.wait_for(jid, S.CANCELLED, 5).state == S.CANCELLED
    assert henv.files() == [] and henv.store.load_segments(jid) == []


def test_insufficient_disk_space_pauses(server, henv, monkeypatch):
    import shutil as _sh
    from collections import namedtuple
    Usage = namedtuple("Usage", "total used free")
    monkeypatch.setattr("swiftdm.engines.http.transfer.shutil.disk_usage", lambda p: Usage(10 * MiB, 9 * MiB, 1 * MiB))
    snap_id = henv.submit(server.add("/huge.bin", Resource(Payload("h", 20 * MiB))))
    snap = henv.m.wait_for(snap_id, S.PAUSED, 10)
    assert snap.reason == "disk_full" and snap.error.code == ErrorCode.DISK_FULL


def test_disk_full_during_write_pauses_then_resumes(server, tmp_path):
    import errno
    from tests.http.conftest import HttpEnv
    state = {"limit": 3 * MiB, "written": 0}

    class Full(Exception):
        pass

    def opener(path, mode):
        f = open(path, mode)
        real_write = f.write

        def write(b):
            if state["limit"] is not None and state["written"] + len(b) > state["limit"]:
                raise OSError(errno.ENOSPC, "No space left on device")
            state["written"] += len(b)
            return real_write(b)

        class Proxy:
            def __getattr__(self, n):
                return getattr(f, n)

            def __enter__(self):
                return self

            def __exit__(self, *a):
                f.close()

        px = Proxy()
        px.write = write
        return px

    env = HttpEnv(tmp_path, opener=opener)
    try:
        p = Payload("df", 10 * MiB)
        jid = env.submit(server.add("/df.bin", Resource(p, etag='"df"')))
        snap = env.m.wait_for(jid, S.PAUSED, 20)
        assert snap.reason == "disk_full", snap
        state["limit"] = None                      # "user freed space"
        env.m.resume(jid)
        snap = env.m.wait_for(jid, {S.COMPLETED, S.FAILED}, 30)
        assert snap.state == S.COMPLETED and sha_file(snap.final_path) == p.sha256()
    finally:
        env.m.shutdown(3)
