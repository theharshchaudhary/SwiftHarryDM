"""Native HTTP engine: segmented, resumable, validated downloads.

Durability rule: bytes are written, flushed and fsync'ed *before* the segment's
committed offset is updated in the database. After a crash only committed bytes
are trusted; anything beyond is downloaded again.
"""
from __future__ import annotations

import collections
import os
import random
import shutil
import threading
import time
from dataclasses import dataclass, field
from typing import Optional

import requests

from ...core.cancel import StopReason, StopRequested
from ...core.errors import ErrorCode, SdmError, from_os_error, is_disk_full
from ...core.models import Job, ProgressSnapshot, Segment, SegmentProgress
from ...fs.names import choose_filename, long_path
from ..base import Completed, ProbeResult, RunContext
from . import planner
from .probe import HttpProbe, map_request_exception, new_session, open_url, probe, status_error
from .validators import if_range_value, parse_content_range, same_resource

MiB = 1024 * 1024
CHUNK = 256 * 1024
OVERLAP = 64 * 1024
DISK_MARGIN = 64 * MiB


class _Restart(Exception):
    """Internal: the current plan cannot continue; restart from zero (optionally single-stream)."""

    def __init__(self, reason: str, single_stream: bool):
        super().__init__(reason)
        self.reason = reason
        self.single_stream = single_stream


@dataclass
class _Seg:
    seg: Segment
    pos: int                      # next absolute offset to write (in memory)
    end: int                      # inclusive; -1 unknown
    resumed: bool = False         # had committed bytes when this run began
    thread: Optional[threading.Thread] = None
    finished: bool = False
    since_ckpt: int = 0
    last_ckpt: float = field(default_factory=time.monotonic)

    @property
    def done(self) -> bool:
        return self.end >= 0 and self.pos > self.end


class HttpEngine:
    name = "http"

    def __init__(self, store, session_factory=new_session, checkpoint_bytes=4 * MiB, checkpoint_secs=1.0,
                 seg_retries=5, backoff_max=30.0, report_interval=0.25, timeout=(10.0, 30.0), opener=open):
        self.store = store
        self.session_factory = session_factory
        self.checkpoint_bytes = checkpoint_bytes
        self.checkpoint_secs = checkpoint_secs
        self.seg_retries = seg_retries
        self.backoff_max = backoff_max
        self.report_interval = report_interval
        self.timeout = timeout
        self.opener = opener     # injectable for disk-full simulation in tests
        self._probes: dict = {}
        self._lock = threading.Lock()

    # ------------------------------------------------------------------ probe
    def _headers(self, job: Job) -> dict:
        return {"Referer": job.spec.referer} if job.spec.referer else {}

    def probe(self, job: Job, ctx: RunContext) -> ProbeResult:
        hp = self._fresh_probe(job, ctx)
        with self._lock:
            self._probes[job.id] = hp
        if hp.is_html and job.spec.kind == "auto" and not job.committed_bytes:
            return ProbeResult(reroute="media")
        name = choose_filename(job.spec.filename, hp.content_disposition, hp.final_url, hp.content_type)
        # Validators are compared with the stored ones in run(), so they are not updated here.
        return ProbeResult(updates={"final_url": hp.final_url, "content_type": hp.content_type,
                                    "title": job.title or name},
                           desired_name=name)

    # ------------------------------------------------------------------ run
    def temp_path_for(self, job: Job) -> str:
        return os.path.join(job.dest_dir, f".{job.reserved_name}.{job.id[:8]}.sdmpart")

    def run(self, job: Job, ctx: RunContext):
        with self._lock:
            hp = self._probes.pop(job.id, None)
        temp = job.temp_path or self.temp_path_for(job)
        if temp != job.temp_path:
            job.temp_path = temp
            ctx.update(temp_path=temp)
        single_stream = False
        for _attempt in range(3):
            if hp is None:
                hp = self._fresh_probe(job, ctx)
            if hp.is_html and job.spec.kind != "http":
                raise SdmError(ErrorCode.NOT_A_FILE, "The link opens a web page, not a downloadable file.")
            segments = self._prepare(job, ctx, hp, temp, single_stream)
            try:
                return _Transfer(self, job, ctx, hp, temp, segments).execute()
            except _Restart as r:
                ctx.log("warning", f"{r.reason}; restarting from the beginning.")
                single_stream = single_stream or r.single_stream
                self._reset(job)
                hp = None   # the resource may have changed: probe again before re-planning
        raise SdmError(ErrorCode.RANGE_VIOLATION, "The server keeps sending inconsistent data.", retryable=True)

    def _fresh_probe(self, job: Job, ctx: RunContext) -> HttpProbe:
        session = self.session_factory()
        try:
            return probe(session, job.spec.url, ctx.cancel, self._headers(job), self.timeout)
        finally:
            session.close()

    def _reset(self, job: Job):
        self.store.delete_segments(job.id)
        if job.temp_path and os.path.exists(long_path(job.temp_path)):
            os.remove(long_path(job.temp_path))

    def _prepare(self, job: Job, ctx: RunContext, hp: HttpProbe, temp: str, single_stream: bool) -> list:
        ranges = hp.ranges and not single_stream
        segments = self.store.load_segments(job.id)
        have_data = any(s.committed for s in segments)
        if segments and not os.path.exists(long_path(temp)):
            ctx.log("warning", "The partial file is missing; starting over.")
            segments = []
        elif have_data:
            ok, why = same_resource(job.total_bytes, job.etag, job.last_modified, hp.total, hp.etag, hp.last_modified)
            if ok and not ranges:
                ok, why = False, "the server does not support resuming"
            if not ok:
                ctx.log("warning", f"Cannot resume: {why}. Restarting from the beginning.")
                self._reset(job)
                segments = []
            else:
                ctx.log("info", f"Resuming at {sum(s.committed for s in segments)} bytes.")
        elif segments:
            self._reset(job)
            segments = []
        fresh = not segments
        if fresh:
            segments = planner.plan(hp.total, ranges, int(getattr(ctx.settings(), "max_segments", 8)))
            self.store.replace_segments(job.id, segments)
        updates = {"total_bytes": hp.total, "etag": hp.etag, "last_modified": hp.last_modified,
                   "accept_ranges": ranges, "final_url": hp.final_url}
        for k, v in updates.items():
            setattr(job, k, v)
        ctx.update(**updates)
        if fresh:
            remaining = hp.total or 0
        else:
            remaining = sum((s.length or 0) - s.committed for s in segments)
        try:
            os.makedirs(long_path(job.dest_dir), exist_ok=True)
            free = shutil.disk_usage(long_path(job.dest_dir)).free
            if hp.total is not None and free < remaining + DISK_MARGIN:
                raise SdmError(ErrorCode.DISK_FULL,
                               f"Not enough free space in {job.dest_dir}: {remaining // MiB + 64} MB needed, "
                               f"{free // MiB} MB free.")
            mode = "r+b" if (not fresh and os.path.exists(long_path(temp))) else "w+b"
            with open(long_path(temp), mode) as f:
                if hp.total and os.fstat(f.fileno()).st_size != hp.total:
                    f.truncate(hp.total)   # reserve the space up front
        except OSError as e:
            raise from_os_error(e, job.dest_dir) from e
        return segments

    def discard(self, job: Job) -> None:
        self.store.delete_segments(job.id)
        for p in {job.temp_path, self.temp_path_for(job) if job.reserved_name else None} - {None}:
            try:
                os.remove(long_path(p))
            except FileNotFoundError:
                pass

    def cleanup(self, job: Job) -> None:
        self.store.delete_segments(job.id)


class _Transfer:
    def __init__(self, eng: HttpEngine, job: Job, ctx: RunContext, hp: HttpProbe, temp: str, segments: list):
        self.eng, self.job, self.ctx, self.hp, self.temp = eng, job, ctx, hp, temp
        self.lock = threading.Lock()
        self.stop = threading.Event()          # internal stop (fatal error / restart / cancel)
        self.fatal: Optional[BaseException] = None
        self.max_active = max(1, int(getattr(ctx.settings(), "max_segments", 8)))
        self.segs = {s.idx: _Seg(s, s.next_offset, s.end, resumed=s.committed > 0) for s in segments}
        self.next_idx = max(self.segs, default=-1) + 1
        self.samples = collections.deque(maxlen=40)
        self.if_range = if_range_value(job.etag, job.last_modified)

    # ------------------------------------------------------------------ coordinator
    def execute(self):
        if self.hp.total == 0:
            return Completed(self.temp, 0, True)
        cb = self.ctx.cancel.on_stop(self.stop.set)
        try:
            last_report = 0.0
            while True:
                with self.lock:
                    running = [s for s in self.segs.values() if s.thread and s.thread.is_alive()]
                    pending = [s for s in self.segs.values() if not s.done and not s.finished and s.thread is None]
                if self.stop.is_set():
                    break
                for s in pending[: max(0, self.max_active - len(running))]:
                    self._launch(s)
                    running.append(s)
                if len(running) < self.max_active and not pending:
                    self._try_split()
                now = time.monotonic()
                if now - last_report >= self.eng.report_interval:
                    self._report()
                    last_report = now
                with self.lock:
                    all_done = all(s.done or (s.end < 0 and s.finished) for s in self.segs.values())
                if all_done:
                    break
                self.stop.wait(0.05)
            for s in list(self.segs.values()):
                if s.thread:
                    s.thread.join()
        finally:
            self.ctx.cancel.remove(cb)
        self._report()
        if self.ctx.cancel.is_set():
            raise StopRequested(self.ctx.cancel.reason or StopReason.PAUSE)
        if self.fatal is not None:
            raise self.fatal
        return self._complete()

    def _complete(self):
        try:
            size = os.path.getsize(long_path(self.temp))
        except OSError as e:
            raise from_os_error(e, self.job.dest_dir) from e
        if self.hp.total is not None:
            if size != self.hp.total or not all(s.done for s in self.segs.values()):
                raise SdmError(ErrorCode.RANGE_VIOLATION, "The download finished with missing data.",
                               f"size={size} expected={self.hp.total}", retryable=True)
            self._verify_coverage()
            return Completed(self.temp, size, True)
        return Completed(self.temp, size, False)

    def _verify_coverage(self):
        """Segments must tile [0, total) exactly: no gaps, no overlaps."""
        spans = sorted((s.seg.start, s.end) for s in self.segs.values())
        expect = 0
        for start, end in spans:
            if start != expect:
                raise SdmError(ErrorCode.RANGE_VIOLATION, "Internal error: segment map has a gap or overlap.",
                               f"spans={spans}")
            expect = end + 1
        if expect != self.hp.total:
            raise SdmError(ErrorCode.RANGE_VIOLATION, "Internal error: segment map is incomplete.", f"spans={spans}")

    def _launch(self, s: _Seg):
        t = threading.Thread(target=self._segment_main, args=(s,), name=f"seg-{self.job.id[:6]}-{s.seg.idx}",
                             daemon=True)
        s.thread = t
        t.start()

    def _try_split(self):
        if not self.job.accept_ranges:
            return   # without byte ranges a new segment would receive the file from byte 0
        with self.lock:
            candidates = [s for s in self.segs.values() if s.thread and s.thread.is_alive() and s.end >= 0
                          and not s.done]
            if not candidates:
                return
            s = max(candidates, key=lambda x: x.end - x.pos)
            keep_to = planner.split_point(s.pos + CHUNK, s.end)
            if keep_to is None:
                return
            new = Segment(self.next_idx, keep_to + 1, s.end, 0)
            # Persist both halves atomically before the running segment can stop at the new end.
            self.eng.store.split_segment(self.job.id, s.seg.idx, keep_to, new)
            s.end = keep_to
            s.seg.end = keep_to
            self.next_idx += 1
            self.segs[new.idx] = _Seg(new, new.start, new.end)

    def _downloaded(self) -> int:
        return sum(s.pos - s.seg.start for s in self.segs.values())

    def _report(self):
        with self.lock:
            done_bytes = self._downloaded()
            segs = tuple(SegmentProgress(s.seg.idx, s.seg.start, s.end, s.pos - s.seg.start,
                                         bool(s.thread and s.thread.is_alive()))
                         for s in sorted(self.segs.values(), key=lambda x: x.seg.start))
        now = time.monotonic()
        self.samples.append((now, done_bytes))
        speed = None
        while len(self.samples) > 2 and now - self.samples[0][0] > 5.0:
            self.samples.popleft()
        if len(self.samples) >= 2 and self.samples[-1][0] - self.samples[0][0] >= 0.5:
            speed = (self.samples[-1][1] - self.samples[0][1]) / (self.samples[-1][0] - self.samples[0][0])
        total = self.hp.total
        eta = (total - done_bytes) / speed if (speed and total is not None and speed > 0) else None
        self.ctx.report(ProgressSnapshot(self.job.id, done_bytes, total, speed, eta, "downloading", segs))

    def _set_fatal(self, exc: BaseException):
        with self.lock:
            if self.fatal is None:
                self.fatal = exc
        self.stop.set()

    # ------------------------------------------------------------------ segment worker
    def _segment_main(self, s: _Seg):
        session = self.eng.session_factory()
        try:
            with self.eng.opener(long_path(self.temp), "r+b") as f:
                self._segment_loop(s, session, f)
        except _Restart as r:
            self._set_fatal(r)
        except SdmError as e:
            self._set_fatal(e)
        except OSError as e:
            self._set_fatal(from_os_error(e, self.job.dest_dir))
        except BaseException as e:  # noqa: BLE001 - surfaced to the manager as an internal error
            self._set_fatal(e)
        finally:
            session.close()
            s.finished = True

    def _checkpoint(self, s: _Seg, f, force=False):
        now = time.monotonic()
        if not force and s.since_ckpt < self.eng.checkpoint_bytes and now - s.last_ckpt < self.eng.checkpoint_secs:
            return
        f.flush()
        os.fsync(f.fileno())
        with self.lock:
            committed = s.pos - s.seg.start
        self.eng.store.commit_segment(self.job.id, s.seg.idx, committed)
        s.seg.committed = committed
        s.since_ckpt = 0
        s.last_ckpt = now

    def _segment_loop(self, s: _Seg, session: requests.Session, f):
        failures = 0
        first = True
        ranged = bool(self.job.accept_ranges)
        url = self.job.final_url or self.job.spec.url
        if not ranged and s.seg.start != 0:
            raise SdmError(ErrorCode.INTERNAL, "Internal error: a non-ranged stream must start at byte 0.")
        try:
            while not self.stop.is_set():
                with self.lock:
                    if s.done:
                        return
                    end = s.end
                if not ranged and s.pos != s.seg.start:
                    # No range support: a broken stream can only be restarted from zero.
                    with self.lock:
                        s.pos = s.seg.start
                    f.truncate(0)
                overlap = min(OVERLAP, s.pos - s.seg.start) if (first and s.resumed and ranged) else 0
                req_start = s.pos - overlap
                headers = dict(self.eng._headers(self.job))
                if ranged:
                    headers["Range"] = f"bytes={req_start}-{end}" if end >= 0 else f"bytes={req_start}-"
                    if self.if_range:
                        headers["If-Range"] = self.if_range
                try:
                    resp, _final = open_url(session, url, headers, self.ctx.cancel, self.eng.timeout)
                except StopRequested:
                    return
                except SdmError as e:
                    if not e.info.retryable:
                        raise
                    failures = self._backoff(failures, e)
                    continue
                cb = self.ctx.cancel.on_stop(resp.close)
                try:
                    self._check_response(resp, ranged, req_start, end)
                    progressed = self._consume(s, resp, f, overlap, req_start)
                    first = False
                except SdmError as e:
                    if self.stop.is_set():
                        return
                    if not e.info.retryable:
                        raise
                    failures = self._backoff(failures, e)
                    continue
                except (requests.RequestException, ConnectionError, OSError) as e:
                    if self.stop.is_set():
                        return
                    if isinstance(e, OSError) and not isinstance(e, ConnectionError) and (
                            is_disk_full(e) or isinstance(e, PermissionError)):
                        raise
                    failures = self._backoff(failures, map_request_exception(e) if isinstance(
                        e, requests.RequestException) else SdmError(ErrorCode.NETWORK, "The connection was lost.",
                                                                     str(e), retryable=True))
                    continue
                finally:
                    self.ctx.cancel.remove(cb)
                    resp.close()
                if progressed:
                    failures = 0
                with self.lock:
                    if s.end < 0:
                        return   # unknown length: a cleanly finished stream is the whole file
                    if s.done:
                        return
                # The response ended before the segment was complete (server sent less): ask again.
                failures = self._backoff(failures, SdmError(
                    ErrorCode.NETWORK, "The server closed the connection early.", retryable=True), quiet=progressed)
        finally:
            try:
                self._checkpoint(s, f, force=True)
            except OSError:
                pass

    def _backoff(self, failures: int, err: SdmError, quiet=False) -> int:
        failures += 1
        if failures > self.eng.seg_retries:
            raise SdmError(err.info.code, err.info.message, err.info.detail, retryable=True,
                           http_status=err.info.http_status, retry_after=err.info.retry_after)
        delay = err.info.retry_after if err.info.retry_after is not None else \
            min(self.eng.backoff_max, 2 ** (failures - 1)) * (0.75 + random.random() * 0.5)
        if quiet:
            delay = min(delay, 0.2)
        if self.ctx.cancel.wait(delay) or self.stop.is_set():
            raise StopRequested(self.ctx.cancel.reason or StopReason.PAUSE) if self.ctx.cancel.is_set() else \
                SdmError(ErrorCode.NETWORK, "stopped", retryable=True)
        return failures

    def _check_response(self, resp: requests.Response, ranged: bool, req_start: int, end: int):
        if not ranged:
            if resp.status_code != 200:
                raise status_error(resp)
            return
        if resp.status_code == 206:
            cr = parse_content_range(resp.headers.get("Content-Range"))
            if (cr is None or cr[0] != req_start or (end >= 0 and cr[1] > end)
                    or (self.hp.total is not None and cr[2] != self.hp.total)):
                raise _Restart(f"The server returned the wrong byte range ({resp.headers.get('Content-Range')!r} "
                               f"for bytes={req_start}-{end})", single_stream=True)
            return
        if resp.status_code == 200:
            if self.if_range:
                raise _Restart("The file on the server changed during the download", single_stream=False)
            raise _Restart("The server stopped honouring byte ranges", single_stream=True)
        if resp.status_code == 416:
            raise _Restart("The server rejected the requested byte range (416)", single_stream=False)
        raise status_error(resp)

    def _consume(self, s: _Seg, resp, f, overlap: int, req_start: int) -> bool:
        """Stream the body into the file. Returns True if any new bytes were written."""
        progressed = False
        verify = bytearray()
        for chunk in resp.iter_content(CHUNK):
            if self.stop.is_set():
                break
            if not chunk:
                continue
            if overlap:
                need = overlap - len(verify)
                verify += chunk[:need]
                chunk = chunk[need:]
                if len(verify) == overlap:
                    f.seek(req_start)
                    on_disk = f.read(overlap)
                    if bytes(verify) != on_disk:
                        raise _Restart("Already-downloaded data no longer matches the server's file",
                                       single_stream=False)
                    overlap = 0
                if not chunk:
                    continue
            with self.lock:
                if s.end >= 0:
                    room = s.end - s.pos + 1
                    if room <= 0:
                        break
                    chunk = chunk[:room]
                pos = s.pos
            f.seek(pos)
            try:
                f.write(chunk)
            except OSError as e:
                raise from_os_error(e, self.job.dest_dir) from e
            with self.lock:
                s.pos = pos + len(chunk)
                reached_end = s.end >= 0 and s.pos > s.end
            s.since_ckpt += len(chunk)
            progressed = True
            self._checkpoint(s, f)
            if reached_end:
                break
        return progressed
