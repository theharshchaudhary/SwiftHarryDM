"""JobManager: the single owner of job state.

All state changes happen on one thread that drains a command queue. UI calls,
API calls, worker results and retry timers are all messages, so transitions
are serialized and every one is persisted before its event is published.
Workers never mutate shared state; they post updates and an Outcome.
"""
from __future__ import annotations

import heapq
import itertools
import os
import queue
import threading
import time
import traceback
import uuid
from typing import Callable, Optional
from urllib.parse import urlsplit

from ..engines.base import Completed, Failed, RunContext, Stopped
from ..fs.collisions import Reservations, finalize_move
from ..fs.names import sanitize_component
from ..logging_setup import get_logger
from . import states
from .cancel import CancelToken, StopReason, StopRequested
from .errors import ErrorCode, ErrorInfo, SdmError, from_os_error
from .events import EventHub, JobChanged, JobLog, JobProgress, JobRemoved
from .models import (ACTIVE_STATES, RUNNING_STATES, TERMINAL_STATES, Job, JobSnapshot, JobSpec, JobState,
                     ProgressSnapshot)

log = get_logger("manager")
S = JobState

_UPDATABLE = {"kind", "title", "final_url", "reserved_name", "temp_path", "total_bytes", "committed_bytes", "etag",
              "last_modified", "content_type", "accept_ranges", "size_verified", "media"}


def new_job_id() -> str:
    return uuid.uuid4().hex


def validate_url(url: str) -> str:
    url = (url or "").strip()
    if len(url) > 8192:
        raise ValueError("The URL is too long.")
    parts = urlsplit(url)
    if parts.scheme.lower() not in ("http", "https") or not parts.hostname:
        raise ValueError("Enter a full http:// or https:// address.")
    return url


class JobManager:
    def __init__(self, store, engines: dict, router: Callable[[JobSpec], str], settings: Callable[[], object],
                 hub: Optional[EventHub] = None, reservations: Optional[Reservations] = None,
                 max_attempts: int = 3, retry_base: float = 5.0, autostart: bool = True):
        self._store = store
        self._engines = engines
        self._router = router
        self._settings = settings
        self._hub = hub or EventHub()
        self._res = reservations or Reservations()
        self._max_attempts = max_attempts
        self._retry_base = retry_base
        self._cmds: queue.Queue = queue.Queue()
        self._jobs: dict = {}
        self._workers: dict = {}          # job_id -> (thread, CancelToken)
        self._timers: list = []
        self._seq = itertools.count()
        self._progress: dict = {}
        self._plock = threading.Lock()
        self._snaps: dict = {}
        self._slock = threading.Lock()
        self._pending_remove: dict = {}   # job_id -> delete_file
        self._stopping = False
        self._shutdown_deadline = None
        self._recover()
        self._thread = threading.Thread(target=self._loop, name="JobManager", daemon=True)
        if autostart:
            self._thread.start()

    # ------------------------------------------------------------------ public API (thread-safe)
    def submit(self, spec: JobSpec, start: bool = True) -> str:
        validate_url(spec.url)
        if not spec.dest_dir:
            raise ValueError("No download folder selected.")
        job = Job(id=new_job_id(), spec=spec)
        self._post(("submit", job, start))
        return job.id

    def pause(self, job_id): self._post(("pause", job_id))
    def resume(self, job_id): self._post(("resume", job_id))
    def cancel(self, job_id): self._post(("cancel", job_id))
    def retry(self, job_id): self._post(("retry", job_id))
    def remove(self, job_id, delete_file=False): self._post(("remove", job_id, delete_file))
    def pause_all(self): self._post(("all", "pause"))
    def resume_all(self): self._post(("all", "resume"))
    def start_all(self): self._post(("all", "start"))

    def subscribe(self):
        return self._hub.subscribe()

    def unsubscribe(self, q):
        self._hub.unsubscribe(q)

    def snapshot(self) -> list:
        with self._slock:
            return sorted(self._snaps.values(), key=lambda s: (s.created_at, s.id))

    def get(self, job_id) -> Optional[JobSnapshot]:
        with self._slock:
            return self._snaps.get(job_id)

    def progress(self, job_id) -> Optional[ProgressSnapshot]:
        with self._plock:
            return self._progress.get(job_id)

    def sync(self, timeout=10.0) -> bool:
        """Block until every command posted before this call has been processed."""
        done = threading.Event()
        self._post(("sync", done))
        return done.wait(timeout)

    def wait_for(self, job_id, wanted, timeout=30.0) -> Optional[JobSnapshot]:
        wanted = {wanted} if isinstance(wanted, JobState) else set(wanted)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            snap = self.get(job_id)
            if snap and snap.state in wanted:
                return snap
            time.sleep(0.02)
        return self.get(job_id)

    def inspect(self, fn, timeout=10.0):
        """Run fn(manager) on the manager thread (tests/diagnostics) and return its result."""
        box, done = [], threading.Event()
        self._post(("inspect", fn, box, done))
        if not done.wait(timeout):
            raise TimeoutError("manager did not respond")
        if isinstance(box[0], BaseException):
            raise box[0]
        return box[0]

    def halt_for_tests(self):
        """Simulate a crash: stop processing immediately without persisting anything further."""
        self._post(("halt",))
        self._thread.join(10)
        for t, token in list(self._workers.values()):
            token.set(StopReason.SHUTDOWN)
            t.join(5)

    def running_count(self) -> int:
        with self._slock:
            return sum(1 for s in self._snaps.values() if s.state in RUNNING_STATES)

    def shutdown(self, timeout: float = 5.0) -> None:
        if not self._thread.is_alive():
            return
        self._post(("shutdown", timeout))
        self._thread.join(timeout + 5)

    # ------------------------------------------------------------------ actor loop
    def _post(self, cmd):
        self._cmds.put(cmd)

    def _loop(self):
        while True:
            timeout = 0.25
            if self._timers:
                timeout = max(0.0, min(timeout, self._timers[0][0] - time.monotonic()))
            try:
                cmd = self._cmds.get(timeout=timeout)
            except queue.Empty:
                cmd = None
            if cmd is not None and cmd[0] == "halt":
                return
            if cmd is not None:
                try:
                    self._handle(cmd)
                except Exception:  # noqa: BLE001 - the manager thread must never die
                    log.error("manager command %s failed:\n%s", cmd[0], traceback.format_exc())
            self._fire_timers()
            if self._stopping:
                if not self._workers or time.monotonic() >= self._shutdown_deadline:
                    self._finish_shutdown()
                    return
            else:
                self._schedule()

    def _handle(self, cmd):
        kind = cmd[0]
        if kind == "sync":
            cmd[1].set()
            return
        if kind == "inspect":
            _, fn, box, done = cmd
            try:
                box.append(fn(self))
            except BaseException as e:  # noqa: BLE001 - handed back to the caller
                box.append(e)
            done.set()
            return
        handler = getattr(self, f"_on_{kind}")
        handler(*cmd[1:])

    # ------------------------------------------------------------------ state helpers
    def _set_state(self, job: Job, new: JobState, reason=None, error=None, keep_error=False):
        states.check(job.state, new)
        job.state = new
        job.reason = reason
        if error is not None:
            job.error = error
        elif not keep_error:
            job.error = None
        job.version += 1
        job.updated_at = time.time()
        self._store.save_job(job)
        self._publish(job)

    def _save(self, job: Job):
        job.version += 1
        job.updated_at = time.time()
        self._store.save_job(job)
        self._publish(job)

    def _publish(self, job: Job):
        with self._plock:
            p = self._progress.get(job.id)
        downloaded = p.downloaded if (p and job.state in RUNNING_STATES) else None
        snap = JobSnapshot.of(job, downloaded)
        with self._slock:
            self._snaps[job.id] = snap
        self._hub.publish(JobChanged(snap))

    def _log(self, job_id, level, message):
        getattr(log, level if level in ("info", "warning", "error") else "info")("job=%s %s", job_id[:8], message)
        self._hub.publish(JobLog(job_id, level, message))

    def _overwrite(self) -> bool:
        return getattr(self._settings(), "collision_policy", "rename") == "overwrite"

    # ------------------------------------------------------------------ recovery
    def _recover(self):
        auto = bool(getattr(self._settings(), "auto_resume", False))
        for job in self._store.load_jobs():
            self._jobs[job.id] = job
            if job.reserved_name and job.state not in TERMINAL_STATES:
                if not self._res.adopt(job.id, job.dest_dir, job.reserved_name):
                    job.reserved_name = None
            if job.state in (S.STARTING, S.DOWNLOADING, S.POSTPROCESSING, S.PAUSING, S.RETRY_WAIT):
                if job.state == S.PAUSING:
                    self._set_state(job, S.PAUSED, "paused", keep_error=True)
                elif auto:
                    self._set_state(job, S.QUEUED, keep_error=True)
                else:
                    self._set_state(job, S.PAUSED, "interrupted", keep_error=True)
            elif job.state == S.FINALIZING:
                self._refinalize(job)
            elif job.state == S.CANCELLING:
                self._discard(job)
                self._set_state(job, S.CANCELLED)
            elif job.state == S.PAUSED and job.reason == "interrupted" and auto:
                self._set_state(job, S.QUEUED, keep_error=True)
            else:
                self._publish(job)

    def _refinalize(self, job: Job):
        if job.final_path and os.path.isfile(job.final_path) and not job.artifact_path:
            self._set_state(job, S.COMPLETED)
            return
        if job.artifact_path and os.path.isfile(job.artifact_path):
            size = os.path.getsize(job.artifact_path)
            if job.kind == "http" and job.size_verified and job.total_bytes is not None and size != job.total_bytes:
                self._set_state(job, S.FAILED, error=ErrorInfo(
                    ErrorCode.POSTPROCESS, "The downloaded file has the wrong size after a restart."))
                return
            self._finalize(job, Completed(job.artifact_path, size, bool(job.size_verified)))
            return
        self._set_state(job, S.FAILED, error=ErrorInfo(
            ErrorCode.POSTPROCESS, "The app closed while finishing this download and the file was not found."))

    # ------------------------------------------------------------------ commands
    def _on_submit(self, job: Job, start: bool):
        job.state = S.QUEUED if start else S.PAUSED
        job.reason = None if start else "not started"
        if job.spec.filename:
            job.reserved_name = self._res.reserve(job.id, job.dest_dir, sanitize_component(job.spec.filename),
                                                  overwrite=self._overwrite())
        self._jobs[job.id] = job
        self._store.save_job(job)
        self._publish(job)
        self._log(job.id, "info", f"Added {job.spec.url}")

    def _on_pause(self, job_id):
        job = self._jobs.get(job_id)
        if not job:
            return
        if job.state in (S.QUEUED, S.RETRY_WAIT):
            self._set_state(job, S.PAUSED, "paused", keep_error=True)
        elif job.state in (S.STARTING, S.DOWNLOADING, S.POSTPROCESSING):
            self._set_state(job, S.PAUSING, "paused")
            self._workers[job_id][1].set(StopReason.PAUSE)

    def _on_resume(self, job_id):
        job = self._jobs.get(job_id)
        if job and job.state == S.PAUSED:
            job.attempts = 0
            self._set_state(job, S.QUEUED)

    def _on_retry(self, job_id):
        job = self._jobs.get(job_id)
        if job and job.state == S.FAILED:
            job.attempts = 0
            self._set_state(job, S.QUEUED)

    def _on_cancel(self, job_id):
        job = self._jobs.get(job_id)
        if not job:
            return
        if job.state in (S.QUEUED, S.PAUSED, S.RETRY_WAIT, S.FAILED):
            self._discard(job)
            self._set_state(job, S.CANCELLED)
            self._log(job_id, "info", "Cancelled")
            self._maybe_remove(job)
        elif job.state in (S.STARTING, S.DOWNLOADING, S.POSTPROCESSING, S.PAUSING):
            self._set_state(job, S.CANCELLING)
            self._workers[job_id][1].set(StopReason.CANCEL)

    def _on_remove(self, job_id, delete_file):
        job = self._jobs.get(job_id)
        if not job:
            return
        if job.state in RUNNING_STATES:
            self._pending_remove[job_id] = delete_file
            if job.state != S.FINALIZING:
                self._on_cancel(job_id)
            return
        if job.state == S.COMPLETED and delete_file and job.final_path:
            try:
                os.remove(job.final_path)
            except OSError as e:
                self._log(job_id, "warning", f"Could not delete file: {e.strerror}")
        if job.state not in TERMINAL_STATES:
            self._discard(job)
        self._res.release(job_id)
        self._store.delete_job(job_id)
        del self._jobs[job_id]
        with self._slock:
            self._snaps.pop(job_id, None)
        with self._plock:
            self._progress.pop(job_id, None)
        self._hub.publish(JobRemoved(job_id))

    def _maybe_remove(self, job):
        if job.id in self._pending_remove and job.state not in RUNNING_STATES:
            self._on_remove(job.id, self._pending_remove.pop(job.id))

    def _on_all(self, action):
        for job in list(self._jobs.values()):
            if action == "pause" and job.state in (S.QUEUED, S.RETRY_WAIT, S.STARTING, S.DOWNLOADING, S.POSTPROCESSING):
                self._on_pause(job.id)
            elif action in ("resume", "start") and job.state == S.PAUSED:
                self._on_resume(job.id)

    def _on_shutdown(self, timeout):
        self._stopping = True
        self._shutdown_deadline = time.monotonic() + timeout
        for job_id, (_, token) in self._workers.items():
            token.set(StopReason.SHUTDOWN)

    def _finish_shutdown(self):
        # Workers that did not stop in time: their checkpoints are durable, so mark them resumable.
        for job_id in list(self._workers):
            job = self._jobs.get(job_id)
            if job and job.state in RUNNING_STATES and job.state != S.FINALIZING:
                if job.state == S.CANCELLING:
                    continue  # recovery finishes the cancel on next start
                self._set_state(job, S.PAUSED, "interrupted", keep_error=True)
        self._workers.clear()

    # ------------------------------------------------------------------ worker messages
    def _on_update(self, job_id, fields):
        job = self._jobs.get(job_id)
        if not job or job.state in TERMINAL_STATES:
            return
        for k, v in fields.items():
            if k in _UPDATABLE:
                setattr(job, k, v)
        self._save(job)

    def _on_phase(self, job_id, phase):
        job = self._jobs.get(job_id)
        if not job:
            return
        if phase == "downloading" and job.state == S.STARTING:
            self._set_state(job, S.DOWNLOADING)
        elif phase == "postprocessing" and job.state == S.DOWNLOADING:
            self._set_state(job, S.POSTPROCESSING)

    def _on_done(self, job_id, outcome):
        self._workers.pop(job_id, None)
        job = self._jobs.get(job_id)
        if not job:
            return
        with self._plock:
            p = self._progress.get(job_id)
        if p is not None:
            job.committed_bytes = p.downloaded
        if isinstance(outcome, Completed):
            if job.state == S.CANCELLING:
                self._discard(job)
                self._set_state(job, S.CANCELLED)
            else:
                job.artifact_path = outcome.artifact
                job.size_verified = outcome.size_verified
                self._set_state(job, S.FINALIZING)
                self._finalize(job, outcome)
        elif isinstance(outcome, Stopped):
            if job.state == S.CANCELLING or outcome.reason == StopReason.CANCEL:
                self._discard(job)
                if job.state != S.CANCELLING:
                    self._set_state(job, S.CANCELLING)
                self._set_state(job, S.CANCELLED)
                self._log(job_id, "info", "Cancelled")
            elif outcome.reason == StopReason.SHUTDOWN and self._stopping:
                self._set_state(job, S.PAUSED, "interrupted", keep_error=True)
            else:
                self._set_state(job, S.PAUSED, "paused", keep_error=True)
                self._log(job_id, "info", "Paused")
        elif isinstance(outcome, Failed):
            self._on_failed(job, outcome.error)
        self._maybe_remove(job)

    def _on_failed(self, job: Job, err: ErrorInfo):
        if job.state == S.CANCELLING:
            self._discard(job)
            self._set_state(job, S.CANCELLED)
            return
        self._log(job.id, "error", err.message)
        if err.code == ErrorCode.DISK_FULL:
            self._set_state(job, S.PAUSED, "disk_full", error=err)
            return
        if job.state == S.PAUSING:
            self._set_state(job, S.PAUSED, "paused", error=err)
            return
        job.attempts += 1
        if err.retryable and job.attempts < self._max_attempts and not self._stopping:
            delay = err.retry_after if err.retry_after else min(self._retry_base * (3 ** (job.attempts - 1)), 120.0)
            self._set_state(job, S.RETRY_WAIT, f"retry in {int(delay)}s", error=err)
            heapq.heappush(self._timers, (time.monotonic() + delay, next(self._seq), job.id))
        else:
            self._set_state(job, S.FAILED, error=err)

    def _fire_timers(self):
        now = time.monotonic()
        while self._timers and self._timers[0][0] <= now:
            _, _, job_id = heapq.heappop(self._timers)
            job = self._jobs.get(job_id)
            if job and job.state == S.RETRY_WAIT:
                self._set_state(job, S.QUEUED, keep_error=True)

    # ------------------------------------------------------------------ scheduling & workers
    def _schedule(self):
        limit = int(getattr(self._settings(), "max_concurrent", 3))
        free = limit - len(self._workers)
        if free <= 0:
            return
        queued = sorted((j for j in self._jobs.values() if j.state == S.QUEUED),
                        key=lambda j: (-j.spec.priority, j.created_at, j.id))
        for job in queued[:free]:
            self._start(job)

    def _start(self, job: Job):
        self._set_state(job, S.STARTING, keep_error=False)
        token = CancelToken()
        jid = job.id
        ctx = RunContext(
            cancel=token,
            report=self._on_progress,
            update=lambda **f: self._post(("update", jid, f)),
            phase=lambda ph: self._post(("phase", jid, ph)),
            log=lambda level, msg: self._log(jid, level, msg),
            settings=self._settings)
        t = threading.Thread(target=self._worker, args=(job.copy(), ctx), name=f"job-{jid[:8]}", daemon=True)
        self._workers[jid] = (t, token)
        t.start()

    def _on_progress(self, p: ProgressSnapshot):
        with self._plock:
            self._progress[p.job_id] = p
        self._hub.publish(JobProgress(p))

    def _worker(self, job: Job, ctx: RunContext):
        outcome = None
        try:
            kind = job.kind or (job.spec.kind if job.spec.kind != "auto" else self._router(job.spec))
            if kind not in self._engines:
                raise SdmError(ErrorCode.UNSUPPORTED, f"No engine available for this download ({kind}).")
            if kind != job.kind:
                job.kind = kind
                ctx.update(kind=kind)
            engine = self._engines[kind]
            probe = engine.probe(job, ctx)
            if probe.reroute and job.spec.kind == "auto" and probe.reroute in self._engines and probe.reroute != kind:
                ctx.log("info", "The link is a web page; trying media extraction.")
                kind = job.kind = probe.reroute
                ctx.update(kind=kind)
                engine = self._engines[kind]
                probe = engine.probe(job, ctx)
            ctx.cancel.raise_if_set()
            updates = dict(probe.updates)
            if probe.desired_name and not job.reserved_name:
                updates["reserved_name"] = self._res.reserve(job.id, job.dest_dir, probe.desired_name,
                                                             overwrite=self._overwrite())
            for k, v in updates.items():
                setattr(job, k, v)
            if updates:
                ctx.update(**updates)
            ctx.phase("downloading")
            outcome = engine.run(job, ctx)
        except StopRequested as s:
            outcome = Stopped(s.reason)
        except SdmError as e:
            outcome = Failed(e.info)
        except OSError as e:
            outcome = Failed(from_os_error(e, job.dest_dir).info)
        except Exception as e:  # noqa: BLE001 - reported to the user, logged with traceback
            log.error("job=%s crashed:\n%s", job.id[:8], traceback.format_exc())
            outcome = Failed(ErrorInfo(ErrorCode.INTERNAL, f"Unexpected error: {type(e).__name__}: {e}",
                                       traceback.format_exc()[-2000:]))
        if ctx.cancel.is_set() and isinstance(outcome, Failed) and outcome.error.code != ErrorCode.DISK_FULL:
            outcome = Stopped(ctx.cancel.reason)
        self._post(("done", job.id, outcome))

    # ------------------------------------------------------------------ finalize / discard
    def _finalize(self, job: Job, outcome: Completed):
        engine = self._engines.get(job.kind)
        try:
            artifact = outcome.artifact
            if not os.path.isfile(artifact):
                raise SdmError(ErrorCode.POSTPROCESS, "The downloaded file is missing.")
            actual = os.path.getsize(artifact)
            if actual != outcome.size:
                raise SdmError(ErrorCode.POSTPROCESS,
                               f"The downloaded file has {actual} bytes but {outcome.size} were expected.")
            desired = job.reserved_name or sanitize_component(os.path.basename(artifact))
            final = finalize_move(artifact, job.dest_dir, desired, job.id, self._res, overwrite=self._overwrite())
            if os.path.getsize(final) != outcome.size:
                raise SdmError(ErrorCode.POSTPROCESS, "The file changed while it was being moved into place.")
            if os.path.basename(final) != desired:
                self._log(job.id, "info", f"A file named {desired} already existed; saved as {os.path.basename(final)}")
            job.final_path = final
            job.reserved_name = os.path.basename(final)
            job.artifact_path = None
            job.temp_path = None
            job.committed_bytes = outcome.size
            job.total_bytes = job.total_bytes if job.total_bytes is not None else outcome.size
            job.completed_at = time.time()
            if engine:
                try:
                    engine.cleanup(job)
                except Exception:  # noqa: BLE001 - leftovers are harmless, the file is in place
                    log.warning("cleanup failed for job=%s:\n%s", job.id[:8], traceback.format_exc())
            self._set_state(job, S.COMPLETED)
            self._res.release(job.id)
            self._log(job.id, "info", f"Completed: {final}")
        except SdmError as e:
            self._set_state(job, S.FAILED, error=e.info)
        except OSError as e:
            self._set_state(job, S.FAILED, error=from_os_error(e, job.dest_dir).info)

    def _discard(self, job: Job):
        engine = self._engines.get(job.kind) if job.kind else None
        if engine:
            try:
                engine.discard(job)
            except Exception:  # noqa: BLE001
                log.warning("discard failed for job=%s:\n%s", job.id[:8], traceback.format_exc())
        job.temp_path = None
        job.artifact_path = None
        self._res.release(job.id)
