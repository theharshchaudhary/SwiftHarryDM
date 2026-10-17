"""Full application E2E across two real launches: close mid-download, restart,
resume from the UI; filename collision; MP3 conversion from a direct link."""
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from swiftdm.engines.media import tools
from swiftdm.engines.media.validate import probe_streams
from swiftdm.store.db import Store
from tests.http.conftest import sha_file
from tests.media.conftest import RawPayload
from tests.support.testserver import MiB, Payload, Resource

ROOT = Path(__file__).resolve().parents[2]
pytestmark = [pytest.mark.gui, pytest.mark.skipif(tools.find_ffmpeg() is None, reason="needs FFmpeg")]


def run_phase(phase, cfg, home):
    env = dict(os.environ, SWIFTDM_HOME=str(home))
    r = subprocess.run([sys.executable, str(ROOT / "tests" / "e2e" / "app_scenario.py"), phase, json.dumps(cfg)],
                       capture_output=True, text=True, encoding="utf-8", env=env, timeout=180)
    lines = [l for l in r.stdout.splitlines() if l.startswith("RESULT ")]
    assert r.returncode == 0 and lines, f"rc={r.returncode}\n{r.stdout[-2000:]}\n{r.stderr[-3000:]}"
    return json.loads(lines[-1][7:])


def test_restart_recovery_collision_and_mp3_through_the_real_app(tmp_path, server):
    ff = tools.find_ffmpeg()
    clip = tmp_path / "clip.mp4"
    subprocess.run([ff, "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25",
                    "-f", "lavfi", "-i", "sine=frequency=440", "-t", "4", "-c:v", "libopenh264", "-c:a", "aac",
                    "-shortest", str(clip)], check=True)
    big = Payload("e2e-big", 48 * MiB)
    small = Payload("e2e-small", 200_000)
    cfg = {"dest": str(tmp_path / "Downloads"),
           "big_url": server.add("/big.iso", Resource(big, etag='"b1"', throttle_bps=2 * MiB)),
           "small_url": server.add("/notes.pdf", Resource(small, content_type="application/pdf")),
           "clip_url": server.add("/clip.mp4", Resource(RawPayload(clip.read_bytes()), content_type="video/mp4"))}
    home = tmp_path / "home"

    r1 = run_phase("1", cfg, home)
    assert r1["violations"] == []
    job_id = r1["job_id"]
    store = Store(home / "state.db")
    job = store.load_job(job_id)
    committed = sum(s.committed for s in store.load_segments(job_id))
    store.close()
    assert job.state.value == "PAUSED" and job.reason == "interrupted"
    assert committed > 0 and os.path.exists(job.temp_path)

    n_requests_before = len(server.requests_for("/big.iso"))
    r2 = run_phase("2", {**cfg, "job_id": job_id}, home)
    assert r2["violations"] == []
    assert r2["row_status_at_startup"] == "Interrupted (app closed)"
    assert r2["buttons_at_startup"][0] == "Resume"
    assert sha_file(r2["resumed_final_path"]) == big.sha256()
    resumed = server.requests_for("/big.iso")[n_requests_before:]
    starts = [int(rng.split("=")[1].split("-")[0]) for _, _, rng, _ in resumed if rng and rng != "bytes=0-0"]
    assert starts and min(starts) > 0                                    # resumed, not restarted
    assert [os.path.basename(p) for p in r2["small_paths"]] == ["notes (1).pdf", "notes.pdf"]
    assert all(sha_file(p) == small.sha256() for p in r2["small_paths"])
    assert r2["mp3"]["state"] == "COMPLETED" and r2["mp3"]["kind"] == "media", r2["mp3"]
    codecs = [s["codec_name"] for s in probe_streams(tools.find_ffprobe(), r2["mp3"]["path"])["streams"]]
    assert codecs == ["mp3"]
    assert "Total: 4" in r2["status_bar"]
    assert [f for f in os.listdir(cfg["dest"]) if f.startswith(".")] == []   # no temp leftovers
