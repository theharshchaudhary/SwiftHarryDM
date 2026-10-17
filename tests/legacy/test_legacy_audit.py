"""Audit findings T1-T7 against the unmodified legacy UniversalDownloader.

Bugs that the legacy engine still has are strict xfails: they document the
behaviour v2 replaces and will start failing loudly if legacy changes.
"""
import os
import time

import pytest

from tests.support import legacy_loader
from tests.support.testserver import MiB, Fault, Payload, Resource


@pytest.fixture(scope="module")
def UD():
    ns = legacy_loader.load({"UniversalDownloader", "get_ffmpeg_path", "sanitize_filename",
                             "get_default_save_path", "clean_percent"})
    return ns["UniversalDownloader"]


def _sha(path):
    import hashlib
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def test_t1_direct_download_bytes_correct(UD, server, tmp_path):
    p = Payload("t1", 6 * MiB)
    url = server.add("/file.bin", Resource(p))
    out = UD(url, "best", str(tmp_path)).download()
    assert _sha(out) == p.sha256()


@pytest.mark.xfail(strict=True, reason="legacy names octet-stream downloads *.unknown_video")
def test_t1_keeps_file_extension(UD, server, tmp_path):
    url = server.add("/file.bin", Resource(Payload("t1", 1 * MiB)))
    out = UD(url, "best", str(tmp_path)).download()
    assert out.endswith("file.bin")


def test_t2_server_without_ranges(UD, server, tmp_path):
    p = Payload("t2", 3 * MiB)
    url = server.add("/norange.bin", Resource(p, ranges=False))
    assert _sha(UD(url, "best", str(tmp_path)).download()) == p.sha256()


def test_t3_in_session_resume_after_cut(UD, server, tmp_path):
    p = Payload("t3", 8 * MiB)
    res = Resource(p, faults=[Fault(after_bytes=3 * MiB, when=lambda n, rng: n == 3 and not rng)])
    url = server.add("/resume.bin", res)
    assert _sha(UD(url, "best", str(tmp_path)).download()) == p.sha256()
    assert any(rng for _, _, rng, _ in server.requests_for("/resume.bin"))


def test_t4_404_raises(UD, server, tmp_path):
    with pytest.raises(Exception):
        UD(server.base + "/missing.bin", "best", str(tmp_path)).download()


@pytest.mark.xfail(strict=True, reason="legacy silently returns the previous same-named file")
def test_t5b_same_name_different_content(UD, server, tmp_path):
    a, b = Payload("A", 1 * MiB), Payload("B", 1 * MiB)
    u1 = server.add("/v1/data.bin", Resource(a))
    u2 = server.add("/v2/data.bin", Resource(b))
    UD(u1, "best", str(tmp_path)).download()
    out = UD(u2, "best", str(tmp_path)).download()
    assert _sha(out) == b.sha256()


@pytest.mark.xfail(strict=True, reason="legacy reports a truncated .part file as success")
def test_t7_persistent_failure_is_reported(UD, server, tmp_path):
    (tmp_path / "someone_elses_video.mp4").write_bytes(b"decoy")
    res = Resource(Payload("dead", 8 * MiB), faults=[Fault(after_bytes=512 * 1024, when=lambda n, rng: n >= 3, times=-1)])
    url = server.add("/dead.bin", res)
    with pytest.raises(Exception):
        UD(url, "best", str(tmp_path)).download()
