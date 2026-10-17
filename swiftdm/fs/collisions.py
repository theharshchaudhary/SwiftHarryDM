"""Destination-name reservation and no-clobber finalization.

A name is reserved for a job as soon as it is known, so two concurrent jobs can
never pick the same file. The final move uses os.rename, which on Windows fails
with FileExistsError instead of overwriting; on a collision the next free
"name (N).ext" is chosen.
"""
from __future__ import annotations

import os
import threading

from .names import long_path, split_ext


def candidates(name: str):
    yield name
    stem, ext = split_ext(name)
    for i in range(1, 10000):
        yield f"{stem} ({i}){ext}"


def _key(dest_dir: str, name: str):
    return os.path.normcase(os.path.abspath(dest_dir)), name.casefold()


class Reservations:
    def __init__(self):
        self._lock = threading.Lock()
        self._owner: dict = {}      # key -> job_id
        self._by_job: dict = {}     # job_id -> key

    def reserve(self, job_id: str, dest_dir: str, desired: str, overwrite: bool = False) -> str:
        with self._lock:
            for cand in candidates(desired):
                k = _key(dest_dir, cand)
                owner = self._owner.get(k)
                if owner is not None and owner != job_id:
                    continue
                # Checked even for the job's own reservation: a file may appear there after reserving.
                if not overwrite and os.path.lexists(long_path(os.path.join(dest_dir, cand))):
                    continue
                self._release_locked(job_id)
                self._owner[k] = job_id
                self._by_job[job_id] = k
                return cand
        raise FileExistsError(f"no free name for {desired!r}")

    def adopt(self, job_id: str, dest_dir: str, name: str) -> bool:
        """Re-register a persisted reservation at startup. False if someone else holds it."""
        with self._lock:
            k = _key(dest_dir, name)
            if self._owner.get(k, job_id) != job_id:
                return False
            self._release_locked(job_id)
            self._owner[k] = job_id
            self._by_job[job_id] = k
            return True

    def release(self, job_id: str) -> None:
        with self._lock:
            self._release_locked(job_id)

    def _release_locked(self, job_id):
        k = self._by_job.pop(job_id, None)
        if k is not None and self._owner.get(k) == job_id:
            del self._owner[k]

    def owner_of(self, dest_dir: str, name: str):
        with self._lock:
            return self._owner.get(_key(dest_dir, name))


def rename_noclobber(src: str, dst: str) -> None:
    if os.name == "nt":
        os.rename(long_path(src), long_path(dst))   # raises FileExistsError if dst exists
    else:
        os.link(src, dst)                           # atomic create-or-fail
        os.unlink(src)


def finalize_move(src: str, dest_dir: str, desired: str, job_id: str, reservations: Reservations,
                  overwrite: bool = False) -> str:
    """Move a finished temp artifact to its final name and return the final path."""
    os.makedirs(long_path(dest_dir), exist_ok=True)
    name = reservations.reserve(job_id, dest_dir, desired, overwrite=overwrite)
    for _ in range(10000):
        dst = os.path.join(dest_dir, name)
        try:
            if overwrite:
                if os.path.isdir(long_path(dst)):
                    raise IsADirectoryError(dst)
                os.replace(long_path(src), long_path(dst))
            else:
                rename_noclobber(src, dst)
            return dst
        except FileExistsError:
            # Someone created the file after we reserved the name: pick the next free one.
            name = reservations.reserve(job_id, dest_dir, desired, overwrite=False)
    raise FileExistsError(desired)
