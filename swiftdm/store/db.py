"""SQLite persistence for jobs and segment checkpoints.

One connection guarded by a lock (sqlite3 objects are not shared across threads
unsafely), WAL journal, synchronous=FULL so a committed transition survives a
power loss. Migrations are forward-only; the database is backed up first.
"""
from __future__ import annotations

import json
import sqlite3
import threading
from pathlib import Path
from typing import Iterable

from ..core.errors import ErrorInfo
from ..core.models import Job, JobSpec, JobState, Segment

SCHEMA_VERSION = 1

MIGRATIONS = {
    1: """
    CREATE TABLE jobs (
        id TEXT PRIMARY KEY,
        spec TEXT NOT NULL,
        state TEXT NOT NULL,
        reason TEXT,
        version INTEGER NOT NULL,
        kind TEXT,
        title TEXT,
        final_url TEXT,
        dest_dir TEXT NOT NULL,
        reserved_name TEXT,
        temp_path TEXT,
        artifact_path TEXT,
        final_path TEXT,
        total_bytes INTEGER,
        committed_bytes INTEGER NOT NULL DEFAULT 0,
        etag TEXT,
        last_modified TEXT,
        content_type TEXT,
        accept_ranges INTEGER,
        size_verified INTEGER,
        media TEXT,
        error TEXT,
        attempts INTEGER NOT NULL DEFAULT 0,
        created_at REAL NOT NULL,
        updated_at REAL NOT NULL,
        completed_at REAL
    );
    CREATE TABLE segments (
        job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
        idx INTEGER NOT NULL,
        start INTEGER NOT NULL,
        end INTEGER NOT NULL,
        committed INTEGER NOT NULL,
        PRIMARY KEY (job_id, idx)
    );
    -- No two unfinished jobs may target the same file name in the same folder.
    CREATE UNIQUE INDEX jobs_reservation ON jobs(lower(dest_dir), lower(reserved_name))
        WHERE reserved_name IS NOT NULL AND state NOT IN ('COMPLETED', 'CANCELLED');
    """,
}

_JOB_COLS = ("id", "spec", "state", "reason", "version", "kind", "title", "final_url", "dest_dir",
             "reserved_name", "temp_path", "artifact_path", "final_path", "total_bytes", "committed_bytes",
             "etag", "last_modified", "content_type", "accept_ranges", "size_verified", "media", "error",
             "attempts", "created_at", "updated_at", "completed_at")


def _to_row(job: Job) -> tuple:
    return (job.id, json.dumps(job.spec.to_dict()), job.state.value, job.reason, job.version, job.kind, job.title,
            job.final_url, job.dest_dir, job.reserved_name, job.temp_path, job.artifact_path, job.final_path,
            job.total_bytes, job.committed_bytes, job.etag, job.last_modified, job.content_type,
            None if job.accept_ranges is None else int(job.accept_ranges),
            None if job.size_verified is None else int(job.size_verified),
            json.dumps(job.media) if job.media is not None else None,
            json.dumps(job.error.to_dict()) if job.error else None,
            job.attempts, job.created_at, job.updated_at, job.completed_at)


def _from_row(r: sqlite3.Row) -> Job:
    return Job(
        id=r["id"], spec=JobSpec.from_dict(json.loads(r["spec"])), state=JobState(r["state"]), reason=r["reason"],
        version=r["version"], kind=r["kind"], title=r["title"], final_url=r["final_url"],
        reserved_name=r["reserved_name"], temp_path=r["temp_path"], artifact_path=r["artifact_path"],
        final_path=r["final_path"], total_bytes=r["total_bytes"], committed_bytes=r["committed_bytes"],
        etag=r["etag"], last_modified=r["last_modified"], content_type=r["content_type"],
        accept_ranges=None if r["accept_ranges"] is None else bool(r["accept_ranges"]),
        size_verified=None if r["size_verified"] is None else bool(r["size_verified"]),
        media=json.loads(r["media"]) if r["media"] else None,
        error=ErrorInfo.from_dict(json.loads(r["error"])) if r["error"] else None,
        attempts=r["attempts"], created_at=r["created_at"], updated_at=r["updated_at"],
        completed_at=r["completed_at"])


class ReservationConflict(Exception):
    pass


class Store:
    def __init__(self, path: Path | str):
        self.path = Path(path)
        self._lock = threading.RLock()
        self._conn = sqlite3.connect(str(self.path), check_same_thread=False, isolation_level=None, timeout=30)
        self._conn.row_factory = sqlite3.Row
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.execute("PRAGMA synchronous=FULL")
        self._conn.execute("PRAGMA foreign_keys=ON")
        self._migrate()

    # -- schema ----------------------------------------------------------
    def schema_version(self) -> int:
        with self._lock:
            row = self._conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='schema_version'").fetchone()
            if not row:
                return 0
            v = self._conn.execute("SELECT v FROM schema_version").fetchone()
            return v[0] if v else 0

    def _migrate(self):
        with self._lock:
            current = self.schema_version()
            if current > SCHEMA_VERSION:
                raise RuntimeError(f"state.db schema v{current} is newer than this app (v{SCHEMA_VERSION}); "
                                   "refusing to modify it. Restore a backup or update the app.")
            if current == SCHEMA_VERSION:
                return
            if current > 0:
                self.backup(self.path.with_name(f"{self.path.name}.bak-v{current}"))
            for v in range(current + 1, SCHEMA_VERSION + 1):
                self._conn.execute("BEGIN IMMEDIATE")
                try:
                    for stmt in [s for s in MIGRATIONS[v].split(";") if s.strip()]:
                        self._conn.execute(stmt)
                    self._conn.execute("CREATE TABLE IF NOT EXISTS schema_version (v INTEGER NOT NULL)")
                    self._conn.execute("DELETE FROM schema_version")
                    self._conn.execute("INSERT INTO schema_version (v) VALUES (?)", (v,))
                    self._conn.execute("COMMIT")
                except Exception:
                    self._conn.execute("ROLLBACK")
                    raise

    def backup(self, dest: Path) -> None:
        with self._lock:
            target = sqlite3.connect(str(dest))
            try:
                self._conn.backup(target)
            finally:
                target.close()

    def integrity_check(self) -> str:
        with self._lock:
            return self._conn.execute("PRAGMA integrity_check").fetchone()[0]

    def close(self):
        with self._lock:
            self._conn.close()

    # -- jobs ------------------------------------------------------------
    def save_job(self, job: Job) -> None:
        placeholders = ",".join("?" * len(_JOB_COLS))
        updates = ",".join(f"{c}=excluded.{c}" for c in _JOB_COLS[1:])
        with self._lock:
            try:
                self._conn.execute(
                    f"INSERT INTO jobs ({','.join(_JOB_COLS)}) VALUES ({placeholders}) "
                    f"ON CONFLICT(id) DO UPDATE SET {updates}", _to_row(job))
            except sqlite3.IntegrityError as e:
                raise ReservationConflict(str(e)) from e

    def load_jobs(self) -> list:
        with self._lock:
            rows = self._conn.execute("SELECT * FROM jobs ORDER BY created_at, id").fetchall()
        return [_from_row(r) for r in rows]

    def load_job(self, job_id: str) -> Job | None:
        with self._lock:
            r = self._conn.execute("SELECT * FROM jobs WHERE id=?", (job_id,)).fetchone()
        return _from_row(r) if r else None

    def delete_job(self, job_id: str) -> None:
        with self._lock:
            self._conn.execute("DELETE FROM jobs WHERE id=?", (job_id,))

    # -- segments --------------------------------------------------------
    def load_segments(self, job_id: str) -> list:
        with self._lock:
            rows = self._conn.execute(
                "SELECT idx, start, end, committed FROM segments WHERE job_id=? ORDER BY start", (job_id,)).fetchall()
        return [Segment(r["idx"], r["start"], r["end"], r["committed"]) for r in rows]

    def replace_segments(self, job_id: str, segments: Iterable[Segment]) -> None:
        with self._lock:
            self._conn.execute("BEGIN IMMEDIATE")
            try:
                self._conn.execute("DELETE FROM segments WHERE job_id=?", (job_id,))
                self._conn.executemany(
                    "INSERT INTO segments (job_id, idx, start, end, committed) VALUES (?,?,?,?,?)",
                    [(job_id, s.idx, s.start, s.end, s.committed) for s in segments])
                self._conn.execute("COMMIT")
            except Exception:
                self._conn.execute("ROLLBACK")
                raise

    def commit_segment(self, job_id: str, idx: int, committed: int) -> None:
        with self._lock:
            self._conn.execute("UPDATE segments SET committed=? WHERE job_id=? AND idx=?", (committed, job_id, idx))

    def split_segment(self, job_id: str, idx: int, new_end: int, new_seg: Segment) -> None:
        """Shrink segment `idx` and add `new_seg` atomically, so no byte range is ever lost."""
        with self._lock:
            self._conn.execute("BEGIN IMMEDIATE")
            try:
                self._conn.execute("UPDATE segments SET end=? WHERE job_id=? AND idx=?", (new_end, job_id, idx))
                self._conn.execute("INSERT INTO segments (job_id, idx, start, end, committed) VALUES (?,?,?,?,?)",
                                   (job_id, new_seg.idx, new_seg.start, new_seg.end, new_seg.committed))
                self._conn.execute("COMMIT")
            except Exception:
                self._conn.execute("ROLLBACK")
                raise

    def delete_segments(self, job_id: str) -> None:
        with self._lock:
            self._conn.execute("DELETE FROM segments WHERE job_id=?", (job_id,))
