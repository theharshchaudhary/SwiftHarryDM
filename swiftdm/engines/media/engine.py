"""Media engine: yt-dlp as a separate, killable process.

probe:  `yt-dlp -J -f <selector>` resolves the preset to concrete format IDs,
        which are pinned on the job so a resume never changes quality.
run:    downloads into a per-job hidden temp folder; success requires exit code
        0 AND a reported output file that exists AND passes ffprobe checks.
Pause kills the process tree and keeps the temp folder (yt-dlp --continue
resumes its .part files). Nothing here bypasses DRM or access controls.
"""
from __future__ import annotations

import json
import os
import shutil
from typing import Optional

from ... import appdata
from ...core.cancel import StopRequested
from ...core.errors import ErrorCode, SdmError
from ...core.models import Job, ProgressSnapshot
from ...fs.names import long_path
from ..base import Completed, ProbeResult, RunContext
from . import formats, runner, tools, validate
from .errormap import map_error
from .progress import (FILE_TEMPLATE, POSTPROCESS_TEMPLATE, PROGRESS_TEMPLATE, Aggregator, is_postprocess,
                       parse_file, parse_progress)

OUTPUT_TEMPLATE = "%(title).150B.%(ext)s"
PROBE_TIMEOUT = 180.0


class MediaEngine:
    name = "media"

    def __init__(self, ytdlp_cmd=None, ffmpeg=None, ffprobe=None, js_runtime=None, extra_args=()):
        self._ytdlp_cmd = ytdlp_cmd
        self._ffmpeg = ffmpeg
        self._ffprobe = ffprobe
        self._js = js_runtime
        self._extra = tuple(extra_args)

    # ------------------------------------------------------------------ tool resolution (lazy)
    def ytdlp(self) -> list:
        return list(self._ytdlp_cmd) if self._ytdlp_cmd else tools.ytdlp_command()

    def ffmpeg(self) -> Optional[str]:
        return self._ffmpeg if self._ffmpeg is not None else tools.find_ffmpeg()

    def ffprobe(self) -> Optional[str]:
        return self._ffprobe if self._ffprobe is not None else tools.find_ffprobe()

    def js_runtime(self):
        return self._js if self._js is not None else tools.find_js_runtime()

    def _common(self) -> list:
        args = ["--ignore-config", "--no-colors", "--encoding", "utf-8", "--no-playlist",
                "--retries", "10", "--fragment-retries", "10", "--socket-timeout", "30",
                "--windows-filenames", "--no-mtime", "--cache-dir", str(appdata.app_dir() / "ytcache")]
        ff = self.ffmpeg()
        if ff:
            args += ["--ffmpeg-location", os.path.dirname(ff)]
        js = self.js_runtime()
        if js:
            args += ["--js-runtimes", f"{js[0]}:{js[1]}"]
        return args + list(self._extra)

    # ------------------------------------------------------------------ probe
    def probe(self, job: Job, ctx: RunContext) -> ProbeResult:
        opts = job.spec.media or formats.options_for_label(job.spec.format_label)
        sel = formats.build(opts)
        out: list = []
        cmd = self.ytdlp() + self._common() + ["-J", "-f", sel.selector, "--", job.spec.url]
        res = runner.run(cmd, ctx.cancel, out.append, timeout=PROBE_TIMEOUT)
        if res.stopped:
            raise StopRequested(ctx.cancel.reason)
        if res.timed_out:
            raise SdmError(ErrorCode.TIMEOUT, "Reading the video information took too long.", retryable=True)
        if res.returncode != 0:
            raise map_error(res.stderr_tail, res.returncode)
        try:
            info = json.loads("\n".join(out))
        except ValueError as e:
            raise SdmError(ErrorCode.EXTRACTOR, "yt-dlp returned unreadable video information.", str(e)) from e
        if info.get("_type") in ("playlist", "multi_video"):
            raise SdmError(ErrorCode.UNSUPPORTED, "Playlists aren't supported in this version. Open a single video.")
        if info.get("is_live") or info.get("live_status") in ("is_live", "is_upcoming", "post_live"):
            raise SdmError(ErrorCode.UNSUPPORTED, "Live streams aren't supported in this version.")
        chosen = info.get("requested_formats") or [info]
        if any(f.get("has_drm") for f in chosen):
            raise SdmError(ErrorCode.UNSUPPORTED, "This video is protected by DRM and can't be downloaded.")
        needs_ffmpeg = len(chosen) > 1 or sel.audio_only
        if needs_ffmpeg and not self.ffmpeg():
            raise SdmError(ErrorCode.TOOL_MISSING, "FFmpeg is required for this quality but was not found. "
                           "Reinstall SwiftHarryDM or check Help > Diagnostics.")
        heights = [int(f["height"]) for f in chosen if f.get("height") and f.get("vcodec") not in (None, "none")]
        has_audio = any(f.get("acodec") not in (None, "none") for f in chosen)
        media = {
            "format_id": info.get("format_id"), "preset": opts.preset, "id": info.get("id"),
            "extractor": info.get("extractor_key") or info.get("extractor"), "height": max(heights) if heights else None,
            "has_audio": has_audio, "duration": info.get("duration"), "audio_only": sel.audio_only,
            "max_height": sel.max_height, "audio_codec": sel.audio_codec,
            "sizes": {str(f.get("format_id")): f.get("filesize") or f.get("filesize_approx") for f in chosen},
        }
        if not media["format_id"]:
            raise SdmError(ErrorCode.EXTRACTOR, "yt-dlp did not report a format for this video.")
        title = info.get("title") or info.get("id") or "video"
        ctx.log("info", f"{media['extractor']}: {title} - format {media['format_id']}"
                        + (f" ({media['height']}p)" if media["height"] else ""))
        return ProbeResult(updates={"title": title, "media": media, "final_url": job.spec.url})

    # ------------------------------------------------------------------ run
    def temp_dir_for(self, job: Job) -> str:
        return os.path.join(job.dest_dir, f".{job.id[:8]}.sdmtmp")

    def run(self, job: Job, ctx: RunContext):
        media = job.media
        if not media:
            raise SdmError(ErrorCode.INTERNAL, "Internal error: media job was not probed.")
        tmp = job.temp_path or self.temp_dir_for(job)
        os.makedirs(long_path(tmp), exist_ok=True)
        if tmp != job.temp_path:
            job.temp_path = tmp
            ctx.update(temp_path=tmp)
        sel = formats.build(job.spec.media or formats.options_for_label(job.spec.format_label))
        cmd = (self.ytdlp() + self._common()
               + ["-f", media["format_id"], *sel.postprocess, "-o", OUTPUT_TEMPLATE,
                  "--paths", f"home:{tmp}", "--paths", f"temp:{tmp}", "--continue", "--no-overwrites",
                  "--newline", "--progress", "--progress-template", PROGRESS_TEMPLATE,
                  "--progress-template", POSTPROCESS_TEMPLATE,
                  "--print", FILE_TEMPLATE, "--no-simulate", "--", job.spec.url])
        agg = Aggregator(media.get("sizes") or {})
        outputs: list = []
        state = {"post": False}

        def on_stdout(line: str):
            p = parse_progress(line)
            if p:
                agg.feed(p)
                done, total, speed, eta = agg.figures()
                ctx.report(ProgressSnapshot(job.id, done, total, speed, eta,
                                            "postprocessing" if state["post"] else "downloading"))
                return
            f = parse_file(line)
            if f:
                outputs.append(f)
                return
            if is_postprocess(line) and not state["post"]:
                state["post"] = True
                ctx.phase("postprocessing")

        res = runner.run(cmd, ctx.cancel, on_stdout, on_stderr=on_stdout)
        if res.stopped:
            raise StopRequested(ctx.cancel.reason)
        if res.returncode != 0:
            raise map_error(res.stderr_tail, res.returncode)
        if not outputs:
            raise SdmError(ErrorCode.POSTPROCESS, "yt-dlp finished but reported no output file.",
                           "\n".join(res.stderr_tail[-20:]))
        path = outputs[-1]
        if not os.path.isfile(long_path(path)):
            raise SdmError(ErrorCode.POSTPROCESS, "yt-dlp reported an output file that does not exist.", path)
        if os.path.normcase(os.path.dirname(os.path.abspath(path))) != os.path.normcase(os.path.abspath(tmp)):
            raise SdmError(ErrorCode.POSTPROCESS, "yt-dlp wrote outside the job's temporary folder.", path)
        probe_exe = self.ffprobe()
        verified = False
        if probe_exe:
            validate.validate(probe_exe, path, audio_only=bool(media.get("audio_only")),
                              expect_audio=bool(media.get("has_audio")) and not media.get("audio_only"),
                              expect_height=media.get("height"), max_height=media.get("max_height"),
                              duration=media.get("duration"), audio_codec=media.get("audio_codec"))
            verified = True
        else:
            ctx.log("warning", "ffprobe not found: the media file could not be verified.")
        size = os.path.getsize(long_path(path))
        done, total, speed, eta = agg.figures()
        ctx.report(ProgressSnapshot(job.id, size, size, speed, 0, "finalizing"))
        return Completed(path, size, verified)

    def discard(self, job: Job) -> None:
        for d in {job.temp_path, self.temp_dir_for(job)} - {None}:
            shutil.rmtree(long_path(d), ignore_errors=True)

    def cleanup(self, job: Job) -> None:
        self.discard(job)
