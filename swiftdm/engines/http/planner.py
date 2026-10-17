"""Segment planning and dynamic splitting."""
from __future__ import annotations

import math
from typing import Optional

from ...core.models import Segment

MiB = 1024 * 1024
SEGMENT_THRESHOLD = 4 * MiB   # smaller files use a single connection
MIN_SEGMENT = 1 * MiB
SPLIT_MIN_REMAINING = 2 * MiB


def plan(total: Optional[int], ranges: bool, max_segments: int) -> list:
    if total == 0:
        return []
    if total is None:
        return [Segment(0, 0, -1)]
    if not ranges or total < SEGMENT_THRESHOLD or max_segments <= 1:
        return [Segment(0, 0, total - 1)]
    n = max(1, min(max_segments, math.ceil(total / MIN_SEGMENT)))
    size = total // n
    segs, start = [], 0
    for i in range(n):
        end = total - 1 if i == n - 1 else start + size - 1
        segs.append(Segment(i, start, end))
        start = end + 1
    return segs


def split_point(pos: int, end: int) -> Optional[int]:
    """Return the last byte the running segment should keep, or None if not worth splitting."""
    remaining = end - pos + 1
    if remaining < SPLIT_MIN_REMAINING:
        return None
    keep = remaining // 2
    if keep < MIN_SEGMENT // 2:
        return None
    return pos + keep - 1
