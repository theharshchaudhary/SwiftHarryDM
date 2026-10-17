"""Error taxonomy shared by all engines. Every failure carries a user message,
redacted technical detail and whether retrying can help."""
from __future__ import annotations

import errno
from dataclasses import asdict, dataclass
from enum import Enum

from ..logging_setup import redact


class ErrorCode(str, Enum):
    NETWORK = "network"
    TIMEOUT = "timeout"
    HTTP_STATUS = "http_status"
    RATE_LIMITED = "rate_limited"
    FORBIDDEN = "forbidden"
    RANGE_VIOLATION = "range_violation"
    VALIDATOR_MISMATCH = "validator_mismatch"
    DISK_FULL = "disk_full"
    PERMISSION_DENIED = "permission_denied"
    PATH_INVALID = "path_invalid"
    TLS = "tls"
    REDIRECT_REFUSED = "redirect_refused"
    NOT_A_FILE = "not_a_file"
    EXTRACTOR = "extractor"
    AUTH_REQUIRED = "auth_required"
    UNAVAILABLE = "unavailable"
    GEO_BLOCKED = "geo_blocked"
    UNSUPPORTED = "unsupported"
    FORMAT_GONE = "format_gone"
    POSTPROCESS = "postprocess"
    TOOL_MISSING = "tool_missing"
    INTERNAL = "internal"


@dataclass(frozen=True)
class ErrorInfo:
    code: ErrorCode
    message: str
    detail: str = ""
    retryable: bool = False
    http_status: int | None = None
    retry_after: float | None = None

    def to_dict(self) -> dict:
        d = asdict(self)
        d["code"] = self.code.value
        return d

    @staticmethod
    def from_dict(d: dict | None) -> "ErrorInfo | None":
        if not d:
            return None
        return ErrorInfo(code=ErrorCode(d["code"]), message=d.get("message", ""), detail=d.get("detail", ""),
                         retryable=bool(d.get("retryable")), http_status=d.get("http_status"),
                         retry_after=d.get("retry_after"))


class SdmError(Exception):
    def __init__(self, code: ErrorCode, message: str, detail: str = "", retryable: bool = False,
                 http_status: int | None = None, retry_after: float | None = None):
        super().__init__(message)
        self.info = ErrorInfo(code, message, redact(detail)[:2000], retryable, http_status, retry_after)


def is_disk_full(exc: BaseException) -> bool:
    return isinstance(exc, OSError) and (exc.errno == errno.ENOSPC or getattr(exc, "winerror", None) in (39, 112))


def from_os_error(exc: OSError, path: str = "") -> SdmError:
    """Map filesystem errors to actionable messages (never silently fall back elsewhere)."""
    where = f" ({path})" if path else ""
    if is_disk_full(exc):
        return SdmError(ErrorCode.DISK_FULL, f"The disk is full{where}. Free some space, then resume.", str(exc))
    if isinstance(exc, PermissionError) or getattr(exc, "winerror", None) in (5, 32, 33):
        winerr = getattr(exc, "winerror", None)
        if winerr in (32, 33):
            return SdmError(ErrorCode.PERMISSION_DENIED,
                            f"The file is in use by another program{where}.", str(exc), retryable=True)
        return SdmError(ErrorCode.PERMISSION_DENIED,
                        f"Windows denied access to the download folder{where}. Choose a folder you can write to.",
                        str(exc))
    if getattr(exc, "winerror", None) in (3, 123, 206) or exc.errno in (errno.ENOENT, errno.ENAMETOOLONG, errno.EINVAL):
        return SdmError(ErrorCode.PATH_INVALID, f"The download path is invalid or no longer exists{where}.", str(exc))
    return SdmError(ErrorCode.INTERNAL, f"File system error{where}: {exc.strerror or exc}", str(exc))
