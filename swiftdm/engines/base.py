"""Engine protocol shared by the native HTTP engine and the media (yt-dlp) adapter."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, Optional, Protocol

from ..core.cancel import CancelToken, StopReason
from ..core.errors import ErrorInfo
from ..core.models import Job, ProgressSnapshot


@dataclass
class ProbeResult:
    updates: dict = field(default_factory=dict)   # Job fields learned from the probe
    desired_name: Optional[str] = None            # reserve this name before downloading
    reroute: Optional[str] = None                 # e.g. "media" when an HTTP URL is really a web page


@dataclass
class Completed:
    artifact: str              # finished temp file, ready to move into place
    size: int
    size_verified: bool = True


@dataclass
class Stopped:
    reason: StopReason


@dataclass
class Failed:
    error: ErrorInfo


Outcome = Completed | Stopped | Failed


@dataclass
class RunContext:
    cancel: CancelToken
    report: Callable[[ProgressSnapshot], None]
    update: Callable[..., None]        # persist Job field changes via the manager
    phase: Callable[[str], None]       # "downloading" | "postprocessing"
    log: Callable[[str, str], None]    # (level, message) shown in the job's log
    settings: Callable[[], object]


class Engine(Protocol):
    name: str

    def probe(self, job: Job, ctx: RunContext) -> ProbeResult: ...

    def run(self, job: Job, ctx: RunContext) -> Outcome: ...

    def discard(self, job: Job) -> None:
        """Delete partial data (cancel). Must be idempotent."""

    def cleanup(self, job: Job) -> None:
        """Remove leftovers after a successful finalize. Must be idempotent."""
