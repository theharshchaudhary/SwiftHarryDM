"""Cooperative cancellation shared between the manager and an engine run."""
import threading
from enum import Enum


class StopReason(str, Enum):
    PAUSE = "pause"
    SHUTDOWN = "shutdown"
    CANCEL = "cancel"


_RANK = {StopReason.PAUSE: 0, StopReason.SHUTDOWN: 1, StopReason.CANCEL: 2}


class StopRequested(Exception):
    def __init__(self, reason: StopReason):
        super().__init__(reason.value)
        self.reason = reason


class CancelToken:
    def __init__(self):
        self._event = threading.Event()
        self._lock = threading.Lock()
        self._reason = None
        self._callbacks = {}
        self._next = 0

    @property
    def reason(self):
        return self._reason

    def is_set(self) -> bool:
        return self._event.is_set()

    def set(self, reason: StopReason) -> None:
        """Request a stop. A stronger reason (cancel > shutdown > pause) upgrades a weaker one."""
        with self._lock:
            if self._reason is not None and _RANK[reason] <= _RANK[self._reason]:
                return
            first = self._reason is None
            self._reason = reason
            callbacks = list(self._callbacks.values()) if first else []
        self._event.set()
        for cb in callbacks:
            try:
                cb()
            except Exception:  # noqa: BLE001 - closing sockets may raise; the stop still happens
                pass

    def on_stop(self, callback) -> int:
        """Run `callback` when a stop is requested (immediately if already stopped)."""
        with self._lock:
            if self._reason is None:
                key = self._next
                self._next += 1
                self._callbacks[key] = callback
                return key
        callback()
        return -1

    def remove(self, key: int) -> None:
        with self._lock:
            self._callbacks.pop(key, None)

    def wait(self, timeout: float) -> bool:
        return self._event.wait(timeout)

    def raise_if_set(self) -> None:
        if self._event.is_set():
            raise StopRequested(self._reason)
