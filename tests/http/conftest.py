import hashlib
import os

import pytest

from swiftdm.config.settings import Settings
from swiftdm.core.manager import JobManager
from swiftdm.core.models import JobSpec, JobState as S
from swiftdm.engines.http.transfer import HttpEngine
from swiftdm.store.db import Store

DONE = {S.COMPLETED, S.FAILED, S.CANCELLED}


def sha_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(4 << 20), b""):
            h.update(b)
    return h.hexdigest()


class HttpEnv:
    def __init__(self, tmp_path, **engine_kw):
        self.settings = Settings(max_concurrent=3, max_segments=8).normalized()
        self.store = Store(tmp_path / "state.db")
        kw = dict(timeout=(3.0, 2.0), backoff_max=0.3, checkpoint_secs=0.2)
        kw.update(engine_kw)
        self.engine = HttpEngine(self.store, **kw)
        self.dest = str(tmp_path / "dl")
        os.makedirs(self.dest)
        self.m = self.new_manager()

    def new_manager(self):
        return JobManager(self.store, {"http": self.engine}, router=lambda spec: "http",
                          settings=lambda: self.settings, retry_base=0.1)

    def submit(self, url, **kw):
        return self.m.submit(JobSpec(url=url, dest_dir=self.dest, **kw))

    def run(self, url, timeout=60, **kw):
        jid = self.submit(url, **kw)
        return self.m.wait_for(jid, DONE, timeout)

    def files(self):
        return sorted(os.listdir(self.dest))


@pytest.fixture
def henv(tmp_path):
    env = HttpEnv(tmp_path)
    yield env
    env.m.shutdown(3)
    env.store.close()
