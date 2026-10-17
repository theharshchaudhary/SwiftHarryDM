"""Child process for randomized kill/resume runs: download one URL with the real
HttpEngine and JobManager; on restart, recover and resume the persisted job."""
import os
import sys
import time

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")))

from swiftdm.config.settings import Settings  # noqa: E402
from swiftdm.core.manager import JobManager  # noqa: E402
from swiftdm.core.models import JobSpec, JobState as S  # noqa: E402
from swiftdm.engines.http.transfer import HttpEngine  # noqa: E402
from swiftdm.store.db import Store  # noqa: E402

db, dest, url, segments = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
settings = Settings(max_concurrent=1, max_segments=segments, auto_resume=True).normalized()
store = Store(db)
engine = HttpEngine(store, timeout=(3.0, 2.0), backoff_max=0.2, checkpoint_secs=0.1, checkpoint_bytes=512 * 1024)
m = JobManager(store, {"http": engine}, router=lambda s: "http", settings=lambda: settings, retry_base=0.1,
               max_attempts=50)
jobs = m.snapshot()
jid = jobs[0].id if jobs else m.submit(JobSpec(url=url, dest_dir=dest))
print("ready", flush=True)
while True:
    snap = m.get(jid)
    if snap and snap.state in (S.COMPLETED, S.FAILED):
        print(f"result {snap.state.value} {snap.final_path or ''} {snap.error.message if snap.error else ''}",
              flush=True)
        m.shutdown(2)
        sys.exit(0)
    time.sleep(0.05)
