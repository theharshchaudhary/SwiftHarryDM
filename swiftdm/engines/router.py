"""Choose the engine for a URL: known media sites go to yt-dlp, everything else
to the native HTTP engine (which re-routes web pages to yt-dlp after probing)."""
from __future__ import annotations

import threading

from ..core.models import JobSpec


class Router:
    def __init__(self, media_available: bool = True):
        self.media_available = media_available
        self._lock = threading.Lock()
        self._extractors = None

    def _load(self):
        with self._lock:
            if self._extractors is None:
                from yt_dlp.extractor import gen_extractor_classes
                self._extractors = [ie for ie in gen_extractor_classes() if ie.ie_key() != "Generic"]
        return self._extractors

    def is_media_url(self, url: str) -> bool:
        for ie in self._load():
            try:
                if ie.suitable(url):
                    return True
            except Exception:  # noqa: BLE001 - a broken extractor regex must not break routing
                continue
        return False

    def __call__(self, spec: JobSpec) -> str:
        if spec.kind in ("http", "media"):
            return spec.kind
        if self.media_available and self.is_media_url(spec.url):
            return "media"
        return "http"
