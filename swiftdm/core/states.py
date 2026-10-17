"""The job state machine. Every state change goes through `check`."""
from .models import JobState as S


class InvalidTransition(Exception):
    pass


_ACTIVE_EXITS = {S.RETRY_WAIT, S.FAILED, S.PAUSING, S.CANCELLING, S.PAUSED, S.QUEUED}

TRANSITIONS: dict = {
    S.QUEUED: {S.STARTING, S.PAUSED, S.CANCELLED},
    S.STARTING: {S.DOWNLOADING} | _ACTIVE_EXITS,
    S.DOWNLOADING: {S.POSTPROCESSING, S.FINALIZING} | _ACTIVE_EXITS,
    S.POSTPROCESSING: {S.FINALIZING} | _ACTIVE_EXITS,
    # Finalizing is a short file move; it cannot be paused or cancelled midway.
    S.FINALIZING: {S.COMPLETED, S.FAILED},
    # A worker may still finish (or fail) after a pause was requested.
    S.PAUSING: {S.PAUSED, S.FINALIZING, S.FAILED, S.CANCELLING},
    S.PAUSED: {S.QUEUED, S.CANCELLED},
    S.RETRY_WAIT: {S.QUEUED, S.PAUSED, S.CANCELLED},
    S.CANCELLING: {S.CANCELLED},
    S.FAILED: {S.QUEUED, S.CANCELLED},
    S.COMPLETED: set(),
    S.CANCELLED: set(),
}


def can(src: S, dst: S) -> bool:
    return dst in TRANSITIONS[src]


def check(src: S, dst: S) -> None:
    if not can(src, dst):
        raise InvalidTransition(f"{src.value} -> {dst.value}")
