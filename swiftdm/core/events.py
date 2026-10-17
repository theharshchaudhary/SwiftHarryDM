"""Events published by the manager. Subscribers get their own thread-safe queue."""
import queue
import threading
from dataclasses import dataclass

from .models import JobSnapshot, ProgressSnapshot


@dataclass(frozen=True)
class JobChanged:
    snapshot: JobSnapshot


@dataclass(frozen=True)
class JobProgress:
    progress: ProgressSnapshot


@dataclass(frozen=True)
class JobRemoved:
    job_id: str


@dataclass(frozen=True)
class JobLog:
    job_id: str
    level: str
    message: str


class EventHub:
    def __init__(self):
        self._lock = threading.Lock()
        self._subs: list = []

    def subscribe(self) -> "queue.SimpleQueue":
        q = queue.SimpleQueue()
        with self._lock:
            self._subs.append(q)
        return q

    def unsubscribe(self, q) -> None:
        with self._lock:
            if q in self._subs:
                self._subs.remove(q)

    def publish(self, event) -> None:
        with self._lock:
            subs = list(self._subs)
        for q in subs:
            q.put(event)
