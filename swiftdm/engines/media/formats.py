"""Quality presets -> yt-dlp format selectors and post-processing arguments.

YouTube no longer serves progressive (audio+video) HD streams, so HD presets
select separate video and audio streams and merge them with FFmpeg. The exact
format IDs chosen at probe time are pinned, so a resumed download never
silently switches quality.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from ...core.models import MediaOptions

LABELS = {
    "best": "Best quality", "2160": "2160p (4K)", "1440": "1440p", "1080": "1080p", "720": "720p",
    "480": "480p", "360": "360p", "mp4": "MP4 (most compatible)", "mkv": "MKV (any codec)",
    "mp3": "MP3 audio", "m4a": "M4A audio (no re-encode)",
}
AUDIO_PRESETS = {"mp3", "m4a"}


@dataclass(frozen=True)
class Selection:
    selector: str
    postprocess: tuple
    audio_only: bool
    max_height: Optional[int]
    audio_codec: Optional[str] = None


def options_for_label(label: str) -> MediaOptions:
    """Map the UI's format choice (legacy labels included) to media options."""
    label = (label or "best").strip().lower()
    if label in LABELS:
        return MediaOptions(preset=label)
    return MediaOptions(preset="best")


def _h(height: Optional[int]) -> str:
    # "<=?" keeps formats whose height is unknown (e.g. direct links) instead of rejecting them.
    return f"[height<=?{height}]" if height else ""


def build(opts: MediaOptions) -> Selection:
    p = opts.preset
    if p == "mp3":
        return Selection("ba/b", ("-x", "--audio-format", "mp3", "--audio-quality", "0"), True, None, "mp3")
    if p == "m4a":
        return Selection("ba[ext=m4a]/ba/b", ("-x", "--audio-format", "m4a"), True, None, "aac")
    if p.isdigit():
        height = int(p)
        h = _h(height)
        return Selection(f"bv*{h}+ba/b{h}", ("--merge-output-format", "mp4/mkv"), False, height)
    height = opts.max_height
    h = _h(height)
    if p == "mp4":
        sel = (f"bv*{h}[vcodec^=avc1]+ba[acodec^=mp4a]/bv*{h}[ext=mp4]+ba[ext=m4a]/b{h}[ext=mp4]/bv*{h}+ba/b{h}")
        return Selection(sel, ("--merge-output-format", "mp4", "--remux-video", "mp4"), False, height)
    if p == "mkv":
        return Selection(f"bv*{h}+ba/b{h}", ("--merge-output-format", "mkv"), False, height)
    return Selection(f"bv*{h}+ba/b{h}", ("--merge-output-format", "mp4/mkv"), False, height)
