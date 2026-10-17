import os
import subprocess
import sys
from pathlib import Path

import pytest

from swiftdm.config.settings import Settings
from swiftdm.core.manager import JobManager
from swiftdm.engines.http.transfer import HttpEngine
from swiftdm.engines.media import tools
from swiftdm.engines.media.engine import MediaEngine
from swiftdm.engines.router import Router
from swiftdm.store.db import Store
from tests.support.testserver import Payload

ROOT = Path(__file__).resolve().parents[2]
FAKE = str(ROOT / "tests" / "support" / "fake_ytdlp.py")

needs_ffmpeg = pytest.mark.skipif(tools.find_ffmpeg() is None, reason="FFmpeg not available (run packaging/fetch_tools.py)")


class RawPayload(Payload):
    def __init__(self, data: bytes):
        self.data = data
        self.size = len(data)

    def read(self, start, length):
        return self.data[start:start + length]

    def all(self):
        return self.data


@pytest.fixture(scope="session")
def clips(tmp_path_factory):
    ff = tools.find_ffmpeg()
    if not ff:
        pytest.skip("FFmpeg not available")
    d = tmp_path_factory.mktemp("clips")

    def make(name, *args):
        out = d / name
        subprocess.run([ff, "-hide_banner", "-loglevel", "error", "-y", *args, str(out)], check=True)
        return out

    return {
        "av": make("av.mp4", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25", "-f", "lavfi", "-i",
                   "sine=frequency=440", "-t", "6", "-c:v", "libopenh264", "-b:v", "300k", "-c:a", "aac", "-shortest"),
        "video_only": make("v.mp4", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25", "-t", "3", "-c:v",
                           "libopenh264"),
        "audio": make("a.m4a", "-f", "lavfi", "-i", "sine=frequency=330", "-t", "4", "-c:a", "aac"),
    }


class MediaEnv:
    def __init__(self, tmp_path, media_engine=None):
        self.settings = Settings(max_concurrent=2).normalized()
        self.store = Store(tmp_path / "state.db")
        self.media = media_engine or MediaEngine()
        self.m = JobManager(self.store, {"http": HttpEngine(self.store), "media": self.media}, Router(),
                            lambda: self.settings, retry_base=0.1)
        self.dest = str(tmp_path / "dl")
        os.makedirs(self.dest)


@pytest.fixture
def menv(tmp_path, app_home):
    env = MediaEnv(tmp_path)
    yield env
    env.m.shutdown(3)


def fake_engine(**kw):
    return MediaEngine(ytdlp_cmd=[sys.executable, FAKE], **kw)
