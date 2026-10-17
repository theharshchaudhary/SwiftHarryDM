"""Locate and update external media tools (yt-dlp, FFmpeg, ffprobe, JS runtime)."""
from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path
from typing import Optional

from ... import appdata
from .jobobject import CREATE_NO_WINDOW

YTDLP_RELEASE = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/"


def _tool_dirs() -> list:
    dirs = [appdata.bundle_dir() / "tools" / "ffmpeg",                       # packaged build
            Path(__file__).resolve().parents[3] / "packaging" / "tools" / "ffmpeg",  # dev checkout
            appdata.tools_dir() / "ffmpeg"]
    return [d for d in dirs if d.is_dir()]


def _find(exe: str) -> Optional[str]:
    for d in _tool_dirs():
        p = d / exe
        if p.is_file():
            return str(p)
    return shutil.which(exe)


def find_ffmpeg() -> Optional[str]:
    return _find("ffmpeg.exe")


def find_ffprobe() -> Optional[str]:
    return _find("ffprobe.exe")


def find_js_runtime() -> Optional[tuple]:
    """yt-dlp needs a JavaScript runtime for full YouTube support. Deno is preferred; Node also works."""
    for name in ("deno", "node"):
        for d in [appdata.tools_dir(), appdata.bundle_dir() / "tools"]:
            p = d / f"{name}.exe"
            if p.is_file():
                return name, str(p)
        found = shutil.which(name)
        if found:
            return name, found
    return None


def updated_ytdlp() -> Path:
    return appdata.tools_dir() / "yt-dlp.exe"


def ytdlp_command() -> list:
    """Command prefix that runs yt-dlp as a separate, killable process."""
    upd = updated_ytdlp()
    if upd.is_file():
        return [str(upd)]
    if getattr(sys, "frozen", False):
        return [sys.executable, "--yt-dlp-shim"]
    return [sys.executable, "-m", "yt_dlp"]


def _version(cmd: list) -> Optional[str]:
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=30, creationflags=CREATE_NO_WINDOW,
                           encoding="utf-8", errors="replace")
        return (r.stdout or r.stderr).strip().splitlines()[0] if r.returncode == 0 else None
    except (OSError, subprocess.SubprocessError, IndexError):
        return None


def versions() -> dict:
    ff = find_ffmpeg()
    js = find_js_runtime()
    return {
        "yt-dlp": _version(ytdlp_command() + ["--version"]),
        "ffmpeg": _version([ff, "-hide_banner", "-version"]) if ff else None,
        "ffmpeg_path": ff,
        "ffprobe_path": find_ffprobe(),
        "js_runtime": f"{js[0]} ({js[1]})" if js else None,
    }


def update_ytdlp(base_url: str = YTDLP_RELEASE, opener=urllib.request.urlopen) -> str:
    """Download the latest yt-dlp.exe, verify it against the release's SHA2-256SUMS,
    sanity-check it, then swap it in atomically, keeping the previous copy."""
    tools = appdata.tools_dir()
    with opener(base_url + "SHA2-256SUMS", timeout=60) as r:
        sums = r.read().decode("utf-8", "replace")
    expected = None
    for line in sums.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].lstrip("*") == "yt-dlp.exe":
            expected = parts[0].lower()
    if not expected:
        raise RuntimeError("yt-dlp.exe is not listed in SHA2-256SUMS")
    new = tools / "yt-dlp.exe.new"
    h = hashlib.sha256()
    with opener(base_url + "yt-dlp.exe", timeout=120) as r, open(new, "wb") as f:
        while True:
            b = r.read(1 << 20)
            if not b:
                break
            h.update(b)
            f.write(b)
    if h.hexdigest() != expected:
        new.unlink(missing_ok=True)
        raise RuntimeError("Downloaded yt-dlp.exe failed its SHA-256 check; nothing was changed.")
    ver = _version([str(new), "--version"])
    if not ver:
        new.unlink(missing_ok=True)
        raise RuntimeError("Downloaded yt-dlp.exe does not run; nothing was changed.")
    cur = tools / "yt-dlp.exe"
    if cur.exists():
        os.replace(cur, tools / "yt-dlp.exe.prev")
    os.replace(new, cur)
    return ver


def rollback_ytdlp() -> bool:
    tools = appdata.tools_dir()
    prev = tools / "yt-dlp.exe.prev"
    if not prev.exists():
        return False
    os.replace(prev, tools / "yt-dlp.exe")
    return True
