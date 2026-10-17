"""Verify finished media files with ffprobe before they are reported as complete."""
from __future__ import annotations

import json
import subprocess
from typing import Optional

from ...core.errors import ErrorCode, SdmError
from .jobobject import CREATE_NO_WINDOW


def probe_streams(ffprobe: str, path: str) -> dict:
    r = subprocess.run([ffprobe, "-v", "error", "-show_streams", "-show_format", "-of", "json", path],
                       capture_output=True, timeout=120, creationflags=CREATE_NO_WINDOW)
    if r.returncode != 0:
        raise SdmError(ErrorCode.POSTPROCESS, "The downloaded media file is unreadable.",
                       r.stderr.decode("utf-8", "replace"))
    return json.loads(r.stdout.decode("utf-8", "replace"))


def validate(ffprobe: str, path: str, *, audio_only: bool, expect_audio: bool, expect_height: Optional[int],
             max_height: Optional[int], duration: Optional[float], audio_codec: Optional[str] = None) -> dict:
    info = probe_streams(ffprobe, path)
    streams = info.get("streams", [])
    video = [s for s in streams if s.get("codec_type") == "video" and not s.get("disposition", {}).get("attached_pic")]
    audio = [s for s in streams if s.get("codec_type") == "audio"]
    problems = []
    if audio_only:
        if not audio:
            problems.append("no audio stream")
        if audio_codec and audio and audio[0].get("codec_name") != audio_codec:
            problems.append(f"audio codec is {audio[0].get('codec_name')}, expected {audio_codec}")
    else:
        if not video:
            problems.append("no video stream")
        else:
            h = max(int(s.get("height") or 0) for s in video)
            if max_height and h > max_height:
                problems.append(f"video is {h}p, above the requested {max_height}p")
            if expect_height and h != expect_height:
                problems.append(f"video is {h}p, expected {expect_height}p")
        if expect_audio and not audio:
            problems.append("no audio stream")
    got = info.get("format", {}).get("duration")
    if duration and got:
        got = float(got)
        if abs(got - duration) > max(2.0, duration * 0.02):
            problems.append(f"duration is {got:.1f}s, expected about {duration:.1f}s")
    if problems:
        raise SdmError(ErrorCode.POSTPROCESS, "The downloaded media failed verification: " + "; ".join(problems) + ".")
    return info
