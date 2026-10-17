"""Property test: random command sequences (including crashes) never violate the
core invariants. SWIFTDM_PROP_EXAMPLES controls the number of sequences
(PR default 200; the acceptance run uses 10000)."""
import os
import shutil
import tempfile
from collections import Counter

from hypothesis import HealthCheck, settings, strategies as st
from hypothesis.stateful import RuleBasedStateMachine, invariant, precondition, rule

from swiftdm.config.settings import Settings
from swiftdm.core.models import RUNNING_STATES, TERMINAL_STATES, JobSpec, JobState as S
from swiftdm.store.db import Store
from tests.support.fake_engine import FakeEngine, make_manager

EXAMPLES = int(os.environ.get("SWIFTDM_PROP_EXAMPLES", "200"))

BEHAVIOURS = ["size=64&steps=2", "size=64&steps=4&delay=0.002", "fail=network&times=1&size=8",
              "fail=forbidden&times=1&size=8", "fail=diskfull&times=1&size=8", "hang=1"]
NAMES = ["a.bin", "b.bin", "A.BIN"]


class ManagerMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.tmp = tempfile.mkdtemp(prefix="sdmprop")
        self.dest = os.path.join(self.tmp, "dl")
        os.makedirs(self.dest)
        self.settings = Settings(max_concurrent=2).normalized()
        self.store = Store(os.path.join(self.tmp, "state.db"))
        self.eng = FakeEngine()
        self.m = make_manager(self.store, self.eng, self.settings, retry_base=0.01)
        self.ids = []

    def teardown(self):
        self.m.shutdown(2)
        self.store.close()
        shutil.rmtree(self.tmp, ignore_errors=True)

    @rule(behaviour=st.sampled_from(BEHAVIOURS), name=st.sampled_from(NAMES), start=st.booleans())
    def submit(self, behaviour, name, start):
        self.ids.append(self.m.submit(JobSpec(url=f"http://fake.test/{name}?{behaviour}", dest_dir=self.dest),
                                      start=start))

    @precondition(lambda self: self.ids)
    @rule(i=st.integers(0, 50), op=st.sampled_from(["pause", "resume", "cancel", "retry", "remove"]))
    def command(self, i, op):
        getattr(self.m, op)(self.ids[i % len(self.ids)])

    @rule()
    def pause_all_resume_all(self):
        self.m.pause_all()
        self.m.resume_all()

    @rule()
    def crash_and_restart(self):
        self.m.halt_for_tests()
        self.m = make_manager(self.store, self.eng, self.settings, retry_base=0.01)

    @invariant()
    def core_invariants(self):
        def check(m):
            limit = self.settings.max_concurrent
            assert len(m._workers) <= limit, "more workers than the concurrency limit"
            for job in m._jobs.values():
                has_worker = job.id in m._workers
                if job.state in (S.STARTING, S.DOWNLOADING, S.POSTPROCESSING, S.PAUSING, S.CANCELLING):
                    assert has_worker, f"{job.state} without a worker"
                else:
                    assert not has_worker, f"worker attached to {job.state} job"
                persisted = self.store.load_job(job.id)
                assert persisted is not None and persisted.state == job.state and persisted.version == job.version
                if job.state == S.COMPLETED:
                    assert job.final_path and os.path.isfile(job.final_path)
                    assert os.path.getsize(job.final_path) == job.total_bytes
            live = Counter((os.path.normcase(j.dest_dir), j.reserved_name.casefold())
                           for j in m._jobs.values() if j.reserved_name and j.state not in TERMINAL_STATES)
            assert all(c == 1 for c in live.values()), f"duplicate reservation {live}"
            finals = Counter(j.final_path.casefold() for j in m._jobs.values() if j.state == S.COMPLETED)
            assert all(c == 1 for c in finals.values()), "two jobs completed into the same file"
            return True

        assert self.m.inspect(check)


ManagerMachine.TestCase.settings = settings(
    max_examples=EXAMPLES, stateful_step_count=12, deadline=None,
    suppress_health_check=[HealthCheck.too_slow, HealthCheck.filter_too_much])
TestManagerProperties = ManagerMachine.TestCase
