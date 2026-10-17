"""The only path from background threads to Tk widgets.

Every 100 ms the Tk main thread drains (a) callables posted with call_soon and
(b) manager events, coalescing progress so the UI never lags behind a fast
download. Background threads never touch a widget.
"""
from __future__ import annotations

import queue
import threading
import traceback

from ..core.events import JobChanged, JobLog, JobProgress, JobRemoved
from ..logging_setup import get_logger

log = get_logger("ui.bridge")
TICK_MS = 100
MAX_EVENTS_PER_TICK = 200


class TkBridge:
    def __init__(self, root, manager, handler):
        """handler must provide on_changed(snap), on_progress(p), on_removed(id), on_log(id, level, msg)."""
        self.root = root
        self.manager = manager
        self.handler = handler
        self.events = manager.subscribe()
        self.calls: queue.SimpleQueue = queue.SimpleQueue()
        self.main_thread = threading.current_thread()
        self._stopped = False
        self.tick_count = 0
        self.root.after(TICK_MS, self._tick)

    def call_soon(self, fn, *args):
        """Thread-safe: run fn(*args) on the Tk main thread."""
        self.calls.put((fn, args))

    def stop(self):
        self._stopped = True
        self.manager.unsubscribe(self.events)

    def _tick(self):
        if self._stopped:
            return
        try:
            self.drain()
        except Exception:  # noqa: BLE001 - a UI bug must not stop the event pump
            log.error("ui tick failed:\n%s", traceback.format_exc())
        self.tick_count += 1
        self.root.after(TICK_MS, self._tick)

    def drain(self):
        while True:
            try:
                fn, args = self.calls.get_nowait()
            except queue.Empty:
                break
            try:
                fn(*args)
            except Exception:  # noqa: BLE001
                log.error("ui call failed:\n%s", traceback.format_exc())
        changed, progress, removed, logs = {}, {}, [], []
        for _ in range(MAX_EVENTS_PER_TICK):
            try:
                ev = self.events.get_nowait()
            except queue.Empty:
                break
            if isinstance(ev, JobChanged):
                changed[ev.snapshot.id] = ev.snapshot
                removed = [r for r in removed if r != ev.snapshot.id]
            elif isinstance(ev, JobProgress):
                progress[ev.progress.job_id] = ev.progress
            elif isinstance(ev, JobRemoved):
                removed.append(ev.job_id)
                changed.pop(ev.job_id, None)
                progress.pop(ev.job_id, None)
            elif isinstance(ev, JobLog):
                logs.append(ev)
        for snap in changed.values():
            self.handler.on_changed(snap)
        for p in progress.values():
            self.handler.on_progress(p)
        for job_id in removed:
            self.handler.on_removed(job_id)
        for ev in logs:
            self.handler.on_log(ev.job_id, ev.level, ev.message)
