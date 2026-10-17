"""App update check (same endpoint as the legacy app, with version validation)."""
import re

import requests

UPDATE_CHECK_URL = "https://swiftharrydm.harshchaudhary.com.np/version.txt"
DOWNLOAD_PAGE_URL = "https://swiftharrydm.harshchaudhary.com.np/downloads"
_VERSION = re.compile(r"^\d+(?:\.\d+){0,3}$")


def parse_version(text: str):
    text = (text or "").strip().lstrip("vV")
    if not _VERSION.match(text):
        return None
    return tuple(int(p) for p in text.split("."))


def newer_version(current: str, url: str = UPDATE_CHECK_URL, http=requests):
    """Return the newer version string, None if up to date. Raises ValueError for a bad response.

    The legacy check reported an update for *any* different text (e.g. an HTML
    error page); a response must now be a version number greater than ours.
    """
    resp = http.get(url, timeout=5)
    resp.raise_for_status()
    latest = parse_version(resp.text)
    if latest is None:
        raise ValueError("The update server returned an unexpected response.")
    cur = parse_version(current) or (0,)
    width = max(len(latest), len(cur))
    pad = lambda v: v + (0,) * (width - len(v))  # noqa: E731
    return resp.text.strip() if pad(latest) > pad(cur) else None
