"""Live tests against CC-licensed media (nightly; need internet).

Test video: "Big Buck Bunny", Blender Foundation, CC-BY 3.0 (youtube aqz-KE-bpKQ).
"""
import os
import time

import pytest

from swiftdm.core.models import JobSpec, JobState as S, MediaOptions
from swiftdm.engines.media.tools import find_ffprobe
from swiftdm.engines.media.validate import probe_streams
from tests.media.conftest import needs_ffmpeg

pytestmark = [pytest.mark.live, needs_ffmpeg]
CC_VIDEO = "https://www.youtube.com/watch?v=aqz-KE-bpKQ"
DONE = {S.COMPLETED, S.FAILED, S.CANCELLED}


def submit(env, preset):
    return env.m.submit(JobSpec(url=CC_VIDEO, dest_dir=env.dest, media=MediaOptions(preset), format_label=preset))


def test_1080p_has_video_1080_and_audio(menv):
    snap = menv.m.wait_for(submit(menv, "1080"), DONE, 1800)
    assert snap.state == S.COMPLETED, snap.error
    info = probe_streams(find_ffprobe(), snap.final_path)
    video = [s for s in info["streams"] if s["codec_type"] == "video"]
    audio = [s for s in info["streams"] if s["codec_type"] == "audio"]
    assert video and int(video[0]["height"]) == 1080
    assert audio
    assert abs(float(info["format"]["duration"]) - 635) < 5
    assert snap.kind == "media"


def test_mp3_is_valid_mp3(menv):
    snap = menv.m.wait_for(submit(menv, "mp3"), DONE, 900)
    assert snap.state == S.COMPLETED, snap.error
    info = probe_streams(find_ffprobe(), snap.final_path)
    assert snap.final_path.endswith(".mp3")
    assert [s["codec_name"] for s in info["streams"] if s["codec_type"] == "audio"] == ["mp3"]


def test_pause_resume_keeps_pinned_format(menv):
    jid = submit(menv, "720")
    menv.m.wait_for(jid, S.DOWNLOADING, 120)
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        p = menv.m.progress(jid)
        if p and p.downloaded > 2_000_000:
            break
        time.sleep(0.2)
    pinned = menv.store.load_job(jid).media["format_id"]
    menv.m.pause(jid)
    assert menv.m.wait_for(jid, S.PAUSED, 10).state == S.PAUSED
    tmp = [d for d in os.listdir(menv.dest) if d.endswith(".sdmtmp")]
    assert tmp and any(f.endswith(".part") for f in os.listdir(os.path.join(menv.dest, tmp[0])))
    menv.m.resume(jid)
    snap = menv.m.wait_for(jid, DONE, 1800)
    assert snap.state == S.COMPLETED, snap.error
    assert menv.store.load_job(jid).media["format_id"] == pinned
    info = probe_streams(find_ffprobe(), snap.final_path)
    assert max(int(s.get("height") or 0) for s in info["streams"] if s["codec_type"] == "video") == 720
