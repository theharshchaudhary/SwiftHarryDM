"""P6: the real Tk window driven in-process, with the Tcl main-thread guard installed."""
import os
import statistics
import subprocess
import sys
import time
import tkinter as tk
from pathlib import Path
from tkinter import messagebox

import pytest

from swiftdm.config.settings import Settings, SettingsStore
from swiftdm.core.manager import JobManager
from swiftdm.core.models import Job, JobSpec, JobState as S
from swiftdm.engines.http.transfer import HttpEngine
from swiftdm.store.db import Store
from swiftdm.ui import guard
from swiftdm.ui.bridge import TkBridge
from swiftdm.ui.window import MainWindow
from tests.http.conftest import sha_file
from tests.support.fake_engine import FakeEngine
from tests.support.testserver import MiB, Payload, Resource

pytestmark = pytest.mark.gui
ROOT = Path(__file__).resolve().parents[2]


class UI:
    def __init__(self, tmp_path, engine_factory=None, prepopulate=None):
        self.store = Store(tmp_path / "state.db")
        if prepopulate:
            prepopulate(self.store)
        self.settings_store = SettingsStore(tmp_path / "settings.json")
        self.settings_store.update(download_dir=str(tmp_path / "dl"), ask_save_location=False, max_segments=4)
        engines = engine_factory(self.store) if engine_factory else {"http": HttpEngine(self.store, timeout=(3, 3))}
        self.manager = JobManager(self.store, engines, lambda s: "http", self.settings_store.get, retry_base=0.1)
        self.root = tk.Tk()
        self.violations = guard.install(self.root, strict=False)
        self.win = MainWindow(self.root, self.manager, self.settings_store, licensing=None)
        self.bridge = TkBridge(self.root, self.manager, self.win)
        self.win.bridge = self.bridge
        for snap in self.manager.snapshot():
            self.win.on_changed(snap)
        self.dest = str(tmp_path / "dl")

    def pump(self, cond, timeout=30.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.root.update()
            if cond():
                return True
            time.sleep(0.01)
        return False

    def row(self, job_id):
        return self.win.rows.get(job_id)

    def only_id(self):
        return next(iter(self.win.rows)) if len(self.win.rows) == 1 else None

    def click(self, job_id, text):
        for b in self.row(job_id).buttons.winfo_children():
            if b.cget("text") == text:
                b.invoke()
                return
        raise AssertionError(f"no {text!r} button; have {[b.cget('text') for b in self.row(job_id).buttons.winfo_children()]}")

    def state(self, job_id):
        s = self.win.snaps.get(job_id)
        return s.state if s else None

    def close(self):
        import gc
        self.bridge.stop()
        self.manager.shutdown(3)
        self.win = self.bridge = None
        gc.collect()          # free Tk variables on the main thread while Tcl is alive
        self.root.destroy()
        self.store.close()


@pytest.fixture
def ui(tmp_path, monkeypatch):
    monkeypatch.setattr(messagebox, "askyesno", lambda *a, **k: True)
    u = UI(tmp_path)
    yield u
    u.close()


def enter(ui, url, quality="best", instant=True):
    ui.win.url_entry.delete(0, "end")
    ui.win.url_entry.insert(0, url)
    ui.win.format_var.set(quality)
    (ui.win.btn_instant if instant else ui.win.btn_add).invoke()


def test_instant_download_through_the_window(ui, server):
    p = Payload("ui1", 6 * MiB)
    enter(ui, server.add("/ui1.bin", Resource(p, etag='"u"')))
    assert ui.pump(lambda: ui.only_id() and ui.state(ui.only_id()) == S.COMPLETED, 30)
    jid = ui.only_id()
    snap = ui.win.snaps[jid]
    assert sha_file(snap.final_path) == p.sha256()
    assert "6.0 MB" in ui.row(jid).info.cget("text") and ui.row(jid).status.cget("text") == "Completed"
    assert [b.cget("text") for b in ui.row(jid).buttons.winfo_children()] == ["Open", "Folder", "Remove", "Details"]
    assert ui.violations == []


def test_add_to_queue_then_start_all(ui, server):
    p = Payload("ui2", 1 * MiB)
    enter(ui, server.add("/ui2.bin", Resource(p)), instant=False)
    assert ui.pump(lambda: ui.only_id() and ui.state(ui.only_id()) == S.PAUSED)
    assert ui.row(ui.only_id()).status.cget("text") == "Not started"
    ui.win.btn_start.invoke()
    assert ui.pump(lambda: ui.state(ui.only_id()) == S.COMPLETED)


def test_pause_resume_cancel_buttons(ui, server):
    p = Payload("ui3", 30 * MiB)
    enter(ui, server.add("/ui3.bin", Resource(p, etag='"u3"', throttle_bps=1 * MiB)))
    assert ui.pump(lambda: ui.only_id() and ui.state(ui.only_id()) == S.DOWNLOADING)
    jid = ui.only_id()
    assert ui.pump(lambda: "B/s" in ui.row(jid).info.cget("text"), 10), ui.row(jid).info.cget("text")
    ui.click(jid, "Pause")
    assert ui.pump(lambda: ui.state(jid) == S.PAUSED, 5)
    ui.click(jid, "Resume")
    assert ui.pump(lambda: ui.state(jid) == S.DOWNLOADING, 5)
    ui.click(jid, "Cancel")
    assert ui.pump(lambda: ui.state(jid) == S.CANCELLED, 5)
    assert os.listdir(ui.dest) == []
    assert ui.violations == []


def test_details_window_shows_real_segments(ui, server):
    p = Payload("ui4", 16 * MiB)
    enter(ui, server.add("/ui4.bin", Resource(p, etag='"u4"', throttle_bps=2 * MiB)))
    assert ui.pump(lambda: ui.only_id() and ui.state(ui.only_id()) == S.DOWNLOADING)
    jid = ui.only_id()
    ui.click(jid, "Details")
    d = ui.win.detail_windows[jid]
    assert ui.pump(lambda: len(d.tree.get_children()) >= 4, 10)
    totals = [d.tree.item(i)["values"] for i in d.tree.get_children()]
    assert all(len(v) == 5 for v in totals)
    assert d.win.grab_current() is None                      # non-modal: the main window stays usable
    ui.manager.cancel(jid)
    assert ui.pump(lambda: ui.state(jid) == S.CANCELLED, 5)


def test_clear_completed_leaves_running_rows_untouched(ui, server):
    done = Payload("done", 1000)
    slow = Payload("slow", 20 * MiB)
    enter(ui, server.add("/done.bin", Resource(done)))
    assert ui.pump(lambda: any(s.state == S.COMPLETED for s in ui.win.snaps.values()))
    enter(ui, server.add("/slow.bin", Resource(slow, etag='"s"', throttle_bps=1 * MiB)))
    assert ui.pump(lambda: any(s.state == S.DOWNLOADING for s in ui.win.snaps.values()))
    running = next(j for j, s in ui.win.snaps.items() if s.state == S.DOWNLOADING)
    row_before = ui.row(running)
    ui.win.btn_clear.invoke()
    assert ui.pump(lambda: len(ui.win.rows) == 1)
    assert ui.row(running) is row_before and ui.state(running) == S.DOWNLOADING
    ui.manager.cancel(running)
    ui.pump(lambda: ui.state(running) == S.CANCELLED, 5)


def test_invalid_url_shows_error_and_submits_nothing(ui, monkeypatch):
    errors = []
    monkeypatch.setattr(messagebox, "showerror", lambda title, msg, **k: errors.append(msg))
    for bad in ("", "notaurl", "ftp://example.com/x", "javascript:alert(1)"):
        enter(ui, bad)
    ui.pump(lambda: False, 0.3)
    assert len(errors) == 4 and ui.win.rows == {}


def test_recovered_jobs_appear_at_startup(tmp_path, monkeypatch):
    def pre(store):
        store.save_job(Job(id="a" * 32, spec=JobSpec(url="https://x.example/big.iso", dest_dir=str(tmp_path)),
                           state=S.DOWNLOADING, kind="http", title="big.iso"))
    u = UI(tmp_path, prepopulate=pre)
    try:
        row = u.row("a" * 32)
        assert row is not None and row.status.cget("text") == "Interrupted (app closed)"
        assert [b.cget("text") for b in row.buttons.winfo_children()][:1] == ["Resume"]
    finally:
        u.close()


def test_ui_stays_responsive_with_20_busy_jobs(tmp_path):
    class ChattyEngine(FakeEngine):
        def run(self, job, ctx):
            from swiftdm.core.models import ProgressSnapshot
            n = 0
            while not ctx.cancel.wait(0.1):          # 10 progress events per second per job
                n += 1000
                ctx.report(ProgressSnapshot(job.id, n, 10 ** 9, 1e6, 999, "downloading"))
            from swiftdm.core.cancel import StopRequested
            raise StopRequested(ctx.cancel.reason)

    u = UI(tmp_path, engine_factory=lambda store: {"http": ChattyEngine()})
    try:
        u.settings_store.update(max_concurrent=10)
        u.manager._settings = lambda: Settings(max_concurrent=20).normalized() if False else \
            u.settings_store.get().__class__(**{**u.settings_store.get().__dict__, "max_concurrent": 10})
        for i in range(20):
            u.manager.submit(JobSpec(url=f"http://fake.test/f{i}.bin?hang=1", dest_dir=u.dest))
        assert u.pump(lambda: sum(1 for s in u.win.snaps.values() if s.state == S.DOWNLOADING) >= 10, 20)
        lateness = []
        state = {"last": time.perf_counter(), "n": 0}

        def probe():
            now = time.perf_counter()
            lateness.append((now - state["last"]) * 1000 - 50)
            state["last"] = now
            state["n"] += 1
            if state["n"] < 100:
                u.root.after(50, probe)
            else:
                u.root.quit()

        state["last"] = time.perf_counter()
        u.root.after(50, probe)
        u.root.mainloop()
        p95 = statistics.quantiles(lateness, n=20)[18]
        print(f"after() lateness p50={statistics.median(lateness):.1f}ms p95={p95:.1f}ms")
        assert p95 < 100, lateness
        assert u.violations == []
        u.manager.pause_all()
    finally:
        u.close()


def test_single_instance_lock(tmp_path):
    from swiftdm.instance import InstanceLock
    a, b = InstanceLock(tmp_path / "l"), InstanceLock(tmp_path / "l")
    assert a.acquire() is True
    assert b.acquire() is False
    a.release()
    assert b.acquire() is True
    b.release()


def test_app_launches_and_exits_cleanly(tmp_path):
    """Smoke test of the real entry point (licensing and update checks disabled for the test only)."""
    script = (
        "import sys; sys.path.insert(0, %r)\n"
        "from swiftdm import app\n"
        "def ready(root, window, manager, api, violations, shutdown):\n"
        "    print('API_PORT', api.port, flush=True)\n"
        "    print('TITLE', root.title(), flush=True)\n"
        "    root.after(1500, shutdown)\n"
        "sys.exit(app.run([], licensing=None, hooks={'ready': ready, 'check_updates': False, 'api_ports': (5051, 5052)}))\n"
    ) % str(ROOT)
    env = dict(os.environ, SWIFTDM_HOME=str(tmp_path / "home"))
    r = subprocess.run([sys.executable, "-c", script], env=env, capture_output=True, text=True, timeout=60)
    assert r.returncode == 0, r.stderr[-2000:]
    assert "API_PORT 5051" in r.stdout and "TITLE SwiftHarryDM v2.0.0" in r.stdout
    assert (tmp_path / "home" / "state.db").exists() and (tmp_path / "home" / "logs" / "swiftdm.log").exists()


def test_bridge_stops_mid_batch_when_app_closes(tmp_path):
    """Regression: closing the app while a modal dialog ran inside drain() made later
    queued callbacks touch destroyed widgets (TclError in the packaged-app smoke test)."""
    u = UI(tmp_path)
    try:
        calls = []
        u.bridge.call_soon(lambda: (calls.append("first"), u.bridge.stop()))
        u.bridge.call_soon(lambda: calls.append("second"))
        u.bridge.drain()
        assert calls == ["first"]
    finally:
        u.bridge._stopped = True
        u.manager.shutdown(3)
        u.root.destroy()
        u.store.close()
