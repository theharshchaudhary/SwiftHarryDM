"""Child process for the forced-kill test: churns jobs through the manager until killed."""
import os
import random
import sys
import time

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")))

from swiftdm.config.settings import Settings  # noqa: E402
from swiftdm.core.models import JobSpec  # noqa: E402
from swiftdm.store.db import Store  # noqa: E402
from tests.support.fake_engine import FakeEngine, make_manager  # noqa: E402

db, dest, seed = sys.argv[1], sys.argv[2], int(sys.argv[3])
rng = random.Random(seed)
m = make_manager(Store(db), FakeEngine(), Settings(max_concurrent=3).normalized(), retry_base=0.01)
behaviours = ["size=4096&steps=8&delay=0.001", "fail=network&times=1&size=64", "hang=1", "size=10"]
ids = []
print("ready", flush=True)
while True:
    r = rng.random()
    if r < 0.4 or not ids:
        ids.append(m.submit(JobSpec(url=f"http://fake.test/f{rng.randint(0, 5)}.bin?{rng.choice(behaviours)}",
                                    dest_dir=dest)))
    else:
        getattr(m, rng.choice(["pause", "resume", "cancel", "retry", "remove"]))(rng.choice(ids))
    time.sleep(rng.random() * 0.005)
