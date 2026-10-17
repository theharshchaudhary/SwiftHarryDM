"""Job domain model. A Job is owned and mutated only by the JobManager thread;
everyone else sees immutable JobSnapshot / ProgressSnapshot copies."""
from __future__ import annotations

import time
from dataclasses import asdict, dataclass, field, replace
from enum import Enum
from typing import Optional

from .errors import ErrorInfo


class JobState(str, Enum):
    QUEUED = "QUEUED"
    STARTING = "STARTING"
    DOWNLOADING = "DOWNLOADING"
    POSTPROCESSING = "POSTPROCESSING"
    FINALIZING = "FINALIZING"
    COMPLETED = "COMPLETED"
    PAUSING = "PAUSING"
    PAUSED = "PAUSED"
    RETRY_WAIT = "RETRY_WAIT"
    CANCELLING = "CANCELLING"
    CANCELLED = "CANCELLED"
    FAILED = "FAILED"


ACTIVE_STATES = frozenset({JobState.STARTING, JobState.DOWNLOADING, JobState.POSTPROCESSING, JobState.FINALIZING})
RUNNING_STATES = ACTIVE_STATES | {JobState.PAUSING, JobState.CANCELLING}   # a worker may exist
TERMINAL_STATES = frozenset({JobState.COMPLETED, JobState.CANCELLED})

MEDIA_PRESETS = ("best", "2160", "1440", "1080", "720", "480", "360", "mp4", "mkv", "mp3", "m4a")


@dataclass(frozen=True)
class MediaOptions:
    preset: str = "best"            # one of MEDIA_PRESETS
    max_height: Optional[int] = None  # used with mp4/mkv presets

    def to_dict(self):
        return asdict(self)

    @staticmethod
    def from_dict(d):
        return MediaOptions(**d) if d else None


@dataclass(frozen=True)
class JobSpec:
    url: str
    dest_dir: str
    kind: str = "auto"              # auto | http | media
    filename: Optional[str] = None  # user override
    media: Optional[MediaOptions] = None
    source: str = "ui"              # ui | extension
    referer: Optional[str] = None
    priority: int = 0
    format_label: str = "best"      # what the user picked, for display

    def to_dict(self):
        d = asdict(self)
        d["media"] = self.media.to_dict() if self.media else None
        return d

    @staticmethod
    def from_dict(d):
        d = dict(d)
        d["media"] = MediaOptions.from_dict(d.get("media"))
        return JobSpec(**d)


@dataclass
class Segment:
    idx: int
    start: int
    end: int            # inclusive; -1 = unknown length (single stream)
    committed: int = 0  # bytes durably written from `start`

    @property
    def length(self) -> Optional[int]:
        return None if self.end < 0 else self.end - self.start + 1

    @property
    def done(self) -> bool:
        return self.end >= 0 and self.committed >= self.length

    @property
    def next_offset(self) -> int:
        return self.start + self.committed


@dataclass
class Job:
    id: str
    spec: JobSpec
    state: JobState = JobState.QUEUED
    reason: Optional[str] = None
    version: int = 0
    kind: Optional[str] = None
    title: Optional[str] = None
    final_url: Optional[str] = None
    reserved_name: Optional[str] = None
    temp_path: Optional[str] = None
    artifact_path: Optional[str] = None
    final_path: Optional[str] = None
    total_bytes: Optional[int] = None
    committed_bytes: int = 0
    etag: Optional[str] = None
    last_modified: Optional[str] = None
    content_type: Optional[str] = None
    accept_ranges: Optional[bool] = None
    size_verified: Optional[bool] = None
    media: Optional[dict] = None    # pinned selection from the media probe
    error: Optional[ErrorInfo] = None
    attempts: int = 0
    created_at: float = field(default_factory=time.time)
    updated_at: float = field(default_factory=time.time)
    completed_at: Optional[float] = None

    @property
    def dest_dir(self) -> str:
        return self.spec.dest_dir

    def copy(self) -> "Job":
        return replace(self, media=dict(self.media) if self.media else None)


@dataclass(frozen=True)
class SegmentProgress:
    idx: int
    start: int
    end: int
    downloaded: int
    active: bool


@dataclass(frozen=True)
class ProgressSnapshot:
    job_id: str
    downloaded: int
    total: Optional[int]
    speed: Optional[float]          # bytes/s measured from real bytes; None = unknown
    eta: Optional[float]            # seconds; None = unknown
    phase: str = ""
    segments: tuple = ()


@dataclass(frozen=True)
class JobSnapshot:
    id: str
    url: str
    title: str
    state: JobState
    reason: Optional[str]
    kind: Optional[str]
    dest_dir: str
    final_path: Optional[str]
    total: Optional[int]
    downloaded: int
    error: Optional[ErrorInfo]
    format_label: str
    source: str
    created_at: float
    attempts: int
    version: int

    @staticmethod
    def of(job: Job, downloaded: Optional[int] = None) -> "JobSnapshot":
        title = job.title or job.reserved_name or job.spec.filename or job.spec.url
        return JobSnapshot(job.id, job.spec.url, title, job.state, job.reason, job.kind, job.dest_dir,
                           job.final_path, job.total_bytes,
                           job.committed_bytes if downloaded is None else downloaded,
                           job.error, job.spec.format_label, job.spec.source, job.created_at, job.attempts,
                           job.version)
