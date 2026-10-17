"""Response validation: Content-Range parsing and resource-identity checks for resume."""
from __future__ import annotations

import re
from typing import Optional

_CR = re.compile(r"^\s*bytes\s+(\d+)-(\d+)/(\d+|\*)\s*$", re.IGNORECASE)


def parse_content_range(value: Optional[str]):
    """'bytes 0-99/1000' -> (0, 99, 1000); total None for '*'. None if malformed."""
    if not value:
        return None
    m = _CR.match(value)
    if not m:
        return None
    start, end = int(m.group(1)), int(m.group(2))
    if end < start:
        return None
    total = None if m.group(3) == "*" else int(m.group(3))
    if total is not None and end >= total:
        return None
    return start, end, total


def is_strong(etag: Optional[str]) -> bool:
    return bool(etag) and not etag.strip().startswith("W/")


def if_range_value(etag: Optional[str], last_modified: Optional[str]) -> Optional[str]:
    """RFC 9110: If-Range requires a strong validator; fall back to Last-Modified."""
    if is_strong(etag):
        return etag
    return last_modified or None


def has_validator(etag: Optional[str], last_modified: Optional[str]) -> bool:
    return if_range_value(etag, last_modified) is not None


def same_resource(old_total, old_etag, old_lm, new_total, new_etag, new_lm):
    """Decide whether partially downloaded bytes may be kept. Returns (ok, reason)."""
    if old_total is not None and new_total is not None and old_total != new_total:
        return False, f"size changed from {old_total} to {new_total} bytes"
    if old_total is None or new_total is None:
        return False, "the file size is unknown"
    if old_etag and new_etag and old_etag != new_etag:
        return False, "the server's ETag changed"
    if old_lm and new_lm and old_lm != new_lm:
        return False, "the server's modification date changed"
    etag_confirms = bool(old_etag) and old_etag == new_etag
    lm_confirms = bool(old_lm) and old_lm == new_lm
    if (old_etag or old_lm) and not (etag_confirms or lm_confirms):
        return False, "the server stopped sending its validator"
    return True, ""
