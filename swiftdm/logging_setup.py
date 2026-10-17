"""Logging with secret redaction.

URLs frequently carry signed tokens in their query strings (media CDNs, cloud
storage), so log output never contains query strings, userinfo or bearer tokens.
"""
import logging
import logging.handlers
import re
import sys
from urllib.parse import urlsplit, urlunsplit

from . import appdata

_URL_RE = re.compile(r"\b(?:https?|ftp)://[^\s\"'<>]+", re.IGNORECASE)
_BEARER_RE = re.compile(r"(Bearer\s+)[A-Za-z0-9._~+/=-]+", re.IGNORECASE)
_KV_SECRET_RE = re.compile(r"((?:token|license_key|password|cookie|authorization)\"?\s*[:=]\s*\"?)[^\s\",}]+", re.IGNORECASE)


def redact_url(url: str) -> str:
    try:
        parts = urlsplit(url)
    except ValueError:
        return "<invalid-url>"
    netloc = parts.hostname or ""
    if parts.port:
        netloc = f"{netloc}:{parts.port}"
    query = "<redacted>" if parts.query else ""
    return urlunsplit((parts.scheme, netloc, parts.path, query, ""))


def redact(text: str) -> str:
    text = _URL_RE.sub(lambda m: redact_url(m.group(0)), text)
    text = _BEARER_RE.sub(r"\1<redacted>", text)
    text = _KV_SECRET_RE.sub(r"\1<redacted>", text)
    return text


class RedactingFilter(logging.Filter):
    def filter(self, record: logging.LogRecord) -> bool:
        try:
            msg = record.getMessage()
        except Exception:  # noqa: BLE001 - never let logging break the app
            return True
        record.msg = redact(msg)
        record.args = ()
        return True


_configured = False


def setup_logging(level=logging.INFO, console=True) -> logging.Logger:
    global _configured
    root = logging.getLogger("swiftdm")
    if _configured:
        return root
    root.setLevel(level)
    fmt = logging.Formatter("%(asctime)s %(levelname)s [%(threadName)s] %(name)s: %(message)s")
    fh = logging.handlers.RotatingFileHandler(
        appdata.logs_dir() / "swiftdm.log", maxBytes=2_000_000, backupCount=5, encoding="utf-8")
    fh.setFormatter(fmt)
    fh.addFilter(RedactingFilter())
    root.addHandler(fh)
    if console and sys.stderr is not None:
        ch = logging.StreamHandler(sys.stderr)
        ch.setFormatter(fmt)
        ch.addFilter(RedactingFilter())
        root.addHandler(ch)
    _configured = True
    return root


def get_logger(name: str) -> logging.Logger:
    logger = logging.getLogger(f"swiftdm.{name}")
    if not any(isinstance(f, RedactingFilter) for f in logger.filters):
        logger.addFilter(RedactingFilter())
    return logger
