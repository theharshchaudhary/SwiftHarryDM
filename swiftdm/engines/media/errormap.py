"""Turn yt-dlp error output into actionable messages. Unknown errors keep yt-dlp's text."""
from __future__ import annotations

import re

from ...core.errors import ErrorCode, SdmError
from ...logging_setup import redact

_RULES = [
    (r"DRM", ErrorCode.UNSUPPORTED, "This video is protected by DRM and can't be downloaded.", False),
    (r"confirm you(?:'|’)re not a bot|not a bot", ErrorCode.AUTH_REQUIRED,
     "The site asked to confirm you're not a bot. Try again later, or enable browser cookies for this download.",
     False),
    (r"Sign in to confirm your age|age[- ]restricted|inappropriate for some users", ErrorCode.AUTH_REQUIRED,
     "This video is age-restricted. Sign in with your browser and enable browser cookies for this download.", False),
    (r"members[- ]only|Join this channel", ErrorCode.AUTH_REQUIRED,
     "This video is for channel members only.", False),
    (r"Private video", ErrorCode.UNAVAILABLE, "This video is private.", False),
    (r"available in your (?:country|region)|geo[- ]?(?:restrict|block)", ErrorCode.GEO_BLOCKED,
     "This video isn't available in your country.", False),
    (r"Video unavailable|has been removed|no longer available|account associated with this video has been terminated",
     ErrorCode.UNAVAILABLE, "This video is unavailable (removed or blocked).", False),
    (r"Requested format is not available", ErrorCode.FORMAT_GONE,
     "The selected quality is not available for this video. Choose another quality.", False),
    (r"Unsupported URL", ErrorCode.UNSUPPORTED, "This website or address is not supported.", False),
    (r"ffmpeg (?:is )?not (?:found|installed)|ffprobe (?:is )?not found|Postprocessing: .*ffmpeg",
     ErrorCode.TOOL_MISSING, "FFmpeg is missing or not working. Reinstall SwiftHarryDM or check Help > Diagnostics.",
     False),
    (r"No space left|\[Errno 28\]|disk is full", ErrorCode.DISK_FULL, "The disk is full. Free some space, then resume.",
     False),
    (r"HTTP Error 429|Too Many Requests", ErrorCode.RATE_LIMITED, "The site is limiting requests. Retrying later.",
     True),
    (r"HTTP Error 403", ErrorCode.FORBIDDEN, "The site refused the download (403). It will be retried.", True),
    (r"HTTP Error 5\d\d", ErrorCode.HTTP_STATUS, "The site had a temporary problem. Retrying.", True),
    (r"timed out|Connection reset|getaddrinfo failed|Failed to resolve|Unable to download webpage|"
     r"Remote end closed|IncompleteRead|ConnectionError|Network is unreachable",
     ErrorCode.NETWORK, "The connection failed. Retrying.", True),
    (r"CERTIFICATE_VERIFY_FAILED|certificate verify failed", ErrorCode.TLS,
     "The secure connection could not be verified (certificate problem).", False),
]
_COMPILED = [(re.compile(p, re.IGNORECASE), c, m, r) for p, c, m, r in _RULES]


def map_error(stderr_lines: list, returncode) -> SdmError:
    errors = [ln for ln in stderr_lines if ln.startswith("ERROR:")]
    text = "\n".join(errors or stderr_lines[-10:])
    for rx, code, msg, retry in _COMPILED:
        if rx.search(text):
            return SdmError(code, msg, text, retryable=retry)
    last = (errors[-1][6:].strip() if errors else (stderr_lines[-1] if stderr_lines else ""))[:300]
    message = f"The media download failed: {redact(last)}" if last else \
        f"The media download failed (yt-dlp exit code {returncode})."
    return SdmError(ErrorCode.EXTRACTOR, message, text, retryable=False)
