"""Media engine with real yt-dlp against a local server (offline), and process
lifecycle with a fake yt-dlp that keeps a grandchild alive."""
import json
import os
import subprocess
import sys
import time

import psutil
import pytest

from swiftdm.core.errors import ErrorCode
from swiftdm.core.models import JobSpec, JobState as S, MediaOptions
from swiftdm.engines.media.tools import find_ffprobe
from swiftdm.engines.media.validate import probe_streams
from tests.media.conftest import FAKE, MediaEnv, RawPayload, fake_engine, needs_ffmpeg
from tests.support.testserver import Resource

DONE = {S.COMPLETED, S.FAILED, S.CANCELLED}


def streams(path):
    info = probe_streams(find_ffprobe(), path)
    return {s["codec_type"]: s for s in info["streams"]}


def submit(env, url, preset, kind="media"):
    return env.m.submit(JobSpec(url=url, dest_dir=env.dest, kind=kind, media=MediaOptions(preset), format_label=preset))


@needs_ffmpeg
@pytest.mark.parametrize("preset,codecs", [("best", {"video": "h264", "audio": "aac"}), ("720", {"video": "h264"}),
                                           ("mp3", {"audio": "mp3"}), ("m4a", {"audio": "aac"})])
def test_real_ytdlp_presets_offline(server, menv, clips, preset, codecs):
    url = server.add("/clip.mp4", Resource(RawPayload(clips["av"].read_bytes()), content_type="video/mp4"))
    snap = menv.m.wait_for(submit(menv, url, preset), DONE, 120)
    assert snap.state == S.COMPLETED, snap.error
    got = streams(snap.final_path)
    for kind, codec in codecs.items():
        assert got[kind]["codec_name"] == codec
    if preset in ("mp3", "m4a"):
        assert "video" not in got or got["video"].get("disposition", {}).get("attached_pic")
    assert [f for f in os.listdir(menv.dest) if f.startswith(".")] == []      # temp folder cleaned up


@needs_ffmpeg
def test_auto_routing_sends_html_pages_to_media_and_files_to_http(server, menv, clips):
    url = server.add("/clip.mp4", Resource(RawPayload(clips["av"].read_bytes()), content_type="video/mp4"))
    snap = menv.m.wait_for(menv.m.submit(JobSpec(url=url, dest_dir=menv.dest)), DONE, 60)
    assert snap.state == S.COMPLETED and snap.kind == "http"                 # a direct file: native engine
    page = server.add("/page", Resource(RawPayload(b"<html><body>no media here</body></html>"),
                                        content_type="text/html"))
    snap = menv.m.wait_for(menv.m.submit(JobSpec(url=page, dest_dir=menv.dest)), DONE, 60)
    assert snap.state == S.FAILED and snap.kind == "media"                   # re-routed, then honest failure
    assert snap.error.code in (ErrorCode.UNSUPPORTED, ErrorCode.EXTRACTOR)


def test_router_recognises_media_sites():
    from swiftdm.engines.router import Router
    r = Router()
    assert r.is_media_url("https://www.youtube.com/watch?v=aqz-KE-bpKQ")
    assert r.is_media_url("https://vimeo.com/76979871")
    assert not r.is_media_url("https://example.com/files/setup.exe")


def _wait_pids(path, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if os.path.exists(path) and open(path).read().strip():
            return [int(x) for x in open(path).read().split()]
        time.sleep(0.05)
    raise AssertionError("fake yt-dlp did not start")


def _alive(pid):
    try:
        return psutil.Process(pid).status() != psutil.STATUS_ZOMBIE
    except psutil.NoSuchProcess:
        return False


def _gone(pids, timeout=3):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not any(_alive(p) for p in pids):
            return True
        time.sleep(0.05)
    return False


@pytest.mark.parametrize("action", ["cancel", "pause"])
def test_stop_kills_whole_process_tree(tmp_path, app_home, monkeypatch, action):
    pidfile = tmp_path / "pids"
    monkeypatch.setenv("FAKE_YTDLP_MODE", "hang")
    monkeypatch.setenv("FAKE_YTDLP_PIDS", str(pidfile))
    env = MediaEnv(tmp_path, fake_engine(ffmpeg="", ffprobe="", js_runtime=()))
    try:
        jid = submit(env, "https://media.example/watch?v=1", "best")
        env.m.wait_for(jid, S.DOWNLOADING, 20)
        pids = _wait_pids(pidfile)
        t0 = time.monotonic()
        getattr(env.m, action)(jid)
        snap = env.m.wait_for(jid, {S.CANCELLED} if action == "cancel" else {S.PAUSED}, 5)
        assert snap.state == (S.CANCELLED if action == "cancel" else S.PAUSED)
        assert time.monotonic() - t0 < 2.0
        assert _gone(pids), "yt-dlp or its child survived"
        tmpdirs = [d for d in os.listdir(env.dest) if d.endswith(".sdmtmp")]
        assert (tmpdirs == []) if action == "cancel" else (len(tmpdirs) == 1)   # pause keeps .part data
    finally:
        env.m.shutdown(3)


def test_app_crash_kills_ytdlp_tree(tmp_path):
    """KILL_ON_JOB_CLOSE: if the app process dies, yt-dlp and FFmpeg die with it."""
    pidfile = tmp_path / "pids"
    script = (
        "import sys; sys.path.insert(0, %r)\n"
        "from swiftdm.core.cancel import CancelToken\n"
        "from swiftdm.engines.media import runner\n"
        "runner.run([sys.executable, %r, '--paths', 'home:%s'], CancelToken(), lambda l: None)\n"
    ) % (str(os.path.dirname(os.path.dirname(FAKE)).rsplit(os.sep, 1)[0]), FAKE, str(tmp_path).replace("\\", "\\\\"))
    env = dict(os.environ, FAKE_YTDLP_MODE="hang", FAKE_YTDLP_PIDS=str(pidfile))
    app = subprocess.Popen([sys.executable, "-c", script], env=env)
    pids = _wait_pids(pidfile)
    app.kill()                       # TerminateProcess on the "app"
    app.wait(5)
    assert _gone(pids, timeout=5), "orphaned yt-dlp/ffmpeg after app crash"


@pytest.mark.parametrize("mode,code", [("age", ErrorCode.AUTH_REQUIRED), ("noresult", ErrorCode.POSTPROCESS)])
def test_failures_are_never_reported_as_success(tmp_path, app_home, monkeypatch, mode, code):
    monkeypatch.setenv("FAKE_YTDLP_MODE", mode)
    env = MediaEnv(tmp_path, fake_engine(ffmpeg="", ffprobe="", js_runtime=()))
    try:
        snap = env.m.wait_for(submit(env, "https://media.example/watch?v=2", "best"), DONE, 20)
        assert snap.state == S.FAILED and snap.error.code == code
    finally:
        env.m.shutdown(3)


def test_success_requires_existing_reported_file(tmp_path, app_home, monkeypatch):
    monkeypatch.setenv("FAKE_YTDLP_MODE", "success")
    env = MediaEnv(tmp_path, fake_engine(ffmpeg="", ffprobe="", js_runtime=()))
    try:
        snap = env.m.wait_for(submit(env, "https://media.example/watch?v=3", "best"), DONE, 20)
        assert snap.state == S.COMPLETED and snap.final_path.endswith("Fake.mp4")
    finally:
        env.m.shutdown(3)


@pytest.mark.parametrize("info,needle", [
    ({"_type": "playlist", "entries": []}, "Playlists"),
    ({"id": "l", "title": "Live", "format_id": "1", "is_live": True}, "Live streams"),
    ({"id": "d", "title": "DRM", "format_id": "1", "requested_formats": [{"format_id": "1", "has_drm": True}]}, "DRM"),
])
def test_probe_refuses_unsupported_content(tmp_path, app_home, monkeypatch, info, needle):
    monkeypatch.setenv("FAKE_YTDLP_JSON", json.dumps(info))
    env = MediaEnv(tmp_path, fake_engine(ffmpeg="", ffprobe="", js_runtime=()))
    try:
        snap = env.m.wait_for(submit(env, "https://media.example/watch?v=4", "best"), DONE, 20)
        assert snap.state == S.FAILED and needle in snap.error.message
    finally:
        env.m.shutdown(3)


def test_merge_without_ffmpeg_is_refused_not_downgraded(tmp_path, app_home, monkeypatch):
    info = {"id": "m", "title": "HD", "format_id": "137+140",
            "requested_formats": [{"format_id": "137", "height": 1080, "vcodec": "avc1", "acodec": "none"},
                                  {"format_id": "140", "vcodec": "none", "acodec": "mp4a"}]}
    monkeypatch.setenv("FAKE_YTDLP_JSON", json.dumps(info))
    env = MediaEnv(tmp_path, fake_engine(ffmpeg="", ffprobe="", js_runtime=()))
    try:
        snap = env.m.wait_for(submit(env, "https://media.example/watch?v=5", "1080"), DONE, 20)
        assert snap.state == S.FAILED and snap.error.code == ErrorCode.TOOL_MISSING
    finally:
        env.m.shutdown(3)
