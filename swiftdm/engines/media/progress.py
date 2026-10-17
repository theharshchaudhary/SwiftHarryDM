"""Parse yt-dlp's machine-readable progress lines (our --progress-template)."""
from __future__ import annotations

import json
from typing import Optional

PROGRESS_PREFIX = "SDMP "
FILE_PREFIX = "SDMF "
# Fields separated by '|'. Missing values are printed as "NA" by yt-dlp.
PROGRESS_TEMPLATE = ("download:" + PROGRESS_PREFIX + "%(info.format_id)s|%(progress.status)s|"
                     "%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|"
                     "%(progress.speed)s|%(progress.eta)s")
FILE_TEMPLATE = "after_move:" + FILE_PREFIX + "%(filepath)j"
POSTPROCESS_PREFIX = "SDMQ "
POSTPROCESS_TEMPLATE = "postprocess:" + POSTPROCESS_PREFIX + "%(progress.status)s|%(progress.postprocessor)s"
POSTPROCESS_MARKERS = ("[Merger]", "[ExtractAudio]", "[VideoRemuxer]", "[VideoConvertor]", "[FixupM3u8]",
                       "[FixupM4a]", "[FixupStretched]", "[FixupDuplicateMoov]", "[FixupTimestamp]")


def _num(v: str) -> Optional[float]:
    try:
        f = float(v)
    except (TypeError, ValueError):
        return None
    return f if f >= 0 else None


def parse_progress(line: str) -> Optional[dict]:
    if not line.startswith(PROGRESS_PREFIX):
        return None
    parts = line[len(PROGRESS_PREFIX):].split("|")
    if len(parts) != 7:
        return None
    fmt, status, done, total, estimate, speed, eta = parts
    return {"format_id": fmt, "status": status, "downloaded": _num(done), "total": _num(total),
            "estimate": _num(estimate), "speed": _num(speed), "eta": _num(eta)}


def parse_file(line: str) -> Optional[str]:
    if not line.startswith(FILE_PREFIX):
        return None
    try:
        v = json.loads(line[len(FILE_PREFIX):])
    except ValueError:
        return None
    return v if isinstance(v, str) else None


def is_postprocess(line: str) -> bool:
    return line.startswith(POSTPROCESS_PREFIX) or line.startswith(POSTPROCESS_MARKERS)


class Aggregator:
    """Combine per-stream progress (video + audio downloads) into one job figure."""

    def __init__(self, expected_sizes: dict):
        self.expected = dict(expected_sizes)   # format_id -> approx bytes (may be None)
        self.done: dict = {}
        self.total: dict = {}
        self.speed: Optional[float] = None

    def feed(self, p: dict):
        fid = p["format_id"]
        if p["downloaded"] is not None:
            self.done[fid] = p["downloaded"]
        tot = p["total"] or p["estimate"]
        if tot:
            self.total[fid] = tot
        if p["status"] == "finished" and fid in self.done:
            self.total[fid] = self.done[fid]
        self.speed = p["speed"]

    def figures(self):
        ids = set(self.expected) | set(self.done) | set(self.total)
        downloaded = int(sum(self.done.values()))
        totals = [self.total.get(i) or self.expected.get(i) for i in ids]
        total = int(sum(totals)) if totals and all(totals) else None
        eta = (total - downloaded) / self.speed if (total and self.speed) else None
        return downloaded, total, self.speed, eta
