"""File-name selection and sanitization for Windows.

Order of preference: user override > Content-Disposition (RFC 6266 / 5987) >
final URL basename > "download". Server-supplied names are reduced to a bare
basename, so a header like ``filename="..\\..\\evil.exe"`` cannot escape the
destination folder.
"""
from __future__ import annotations

import mimetypes
import os
import re
import unicodedata
from urllib.parse import unquote, unquote_to_bytes, urlsplit

_INVALID = re.compile(r'[<>:"/\\|?*\x00-\x1f\x7f]')
_RESERVED = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$",
             *(f"COM{i}" for i in "123456789¹²³"), *(f"LPT{i}" for i in "123456789¹²³")}
# Leave room for " (9999)" and the hidden temp suffix ".<id8>.sdmpart" inside 255 UTF-16 units.
MAX_NAME_UNITS = 220

_EXT_FOR_TYPE = {
    "application/zip": ".zip", "application/x-zip-compressed": ".zip", "application/pdf": ".pdf",
    "application/x-msdownload": ".exe", "application/vnd.microsoft.portable-executable": ".exe",
    "application/x-msi": ".msi", "application/x-ms-installer": ".msi", "application/x-7z-compressed": ".7z",
    "application/vnd.rar": ".rar", "application/x-rar-compressed": ".rar", "application/gzip": ".gz",
    "application/x-gzip": ".gz", "application/x-tar": ".tar", "application/x-iso9660-image": ".iso",
    "application/json": ".json", "text/plain": ".txt", "text/csv": ".csv", "text/html": ".html",
    "image/jpeg": ".jpg", "image/png": ".png", "image/gif": ".gif", "image/webp": ".webp",
    "video/mp4": ".mp4", "video/webm": ".webm", "video/x-matroska": ".mkv", "audio/mpeg": ".mp3",
    "audio/mp4": ".m4a", "audio/ogg": ".ogg", "audio/wav": ".wav", "audio/x-wav": ".wav",
}


def utf16_units(s: str) -> int:
    return len(s.encode("utf-16-le")) // 2


def split_ext(name: str) -> tuple[str, str]:
    i = name.rfind(".")
    if i > 0:
        ext = name[i + 1:]
        if 1 <= len(ext) <= 10 and ext.isalnum():
            return name[:i], name[i:]
    return name, ""


def sanitize_component(name: str, max_units: int = MAX_NAME_UNITS) -> str:
    name = unicodedata.normalize("NFC", name or "")
    name = _INVALID.sub("_", name)
    name = name.strip().rstrip(". ").strip()
    if name in ("", ".", ".."):
        name = "download"
    if name.split(".")[0].rstrip(" ").upper() in _RESERVED:
        name = "_" + name
    stem, ext = split_ext(name)
    if utf16_units(ext) >= max_units // 2:
        stem, ext = name, ""
    if utf16_units(stem + ext) > max_units:
        while utf16_units(stem + ext) > max_units and stem:
            stem = stem[:-1]
        stem = stem.rstrip(". ")
    return (stem or "download") + ext


def _parse_params(value: str) -> dict:
    params, i, n = {}, 0, len(value)
    # skip disposition type
    while i < n and value[i] != ";":
        i += 1
    while i < n:
        i += 1  # skip ';'
        while i < n and value[i] in " \t":
            i += 1
        start = i
        while i < n and value[i] not in "=;":
            i += 1
        key = value[start:i].strip().lower()
        if i >= n or value[i] == ";":
            continue
        i += 1  # '='
        while i < n and value[i] in " \t":
            i += 1
        if i < n and value[i] == '"':
            i += 1
            buf = []
            while i < n and value[i] != '"':
                if value[i] == "\\" and i + 1 < n:
                    i += 1
                buf.append(value[i])
                i += 1
            i += 1
            val = "".join(buf)
            while i < n and value[i] != ";":
                i += 1
        else:
            start = i
            while i < n and value[i] != ";":
                i += 1
            val = value[start:i].strip()
        if key and key not in params:
            params[key] = val
    return params


def _decode_ext_value(v: str) -> str | None:
    """RFC 5987 ext-value: charset'lang'pct-encoded."""
    parts = v.split("'", 2)
    if len(parts) != 3:
        return None
    charset = parts[0].strip().lower() or "utf-8"
    if charset not in ("utf-8", "iso-8859-1", "us-ascii"):
        return None
    try:
        return unquote_to_bytes(parts[2]).decode(charset)
    except (UnicodeDecodeError, LookupError):
        return None


def _basename(v: str) -> str | None:
    v = re.split(r"[\\/]", v.strip())[-1].strip()
    return None if v in ("", ".", "..") else v


def parse_content_disposition(value: str | None) -> str | None:
    if not value:
        return None
    params = _parse_params(value)
    if "filename*" in params:
        decoded = _decode_ext_value(params["filename*"])
        if decoded:
            name = _basename(decoded)
            if name:
                return name
    if "filename" in params:
        raw = params["filename"]
        # Many servers send raw UTF-8 bytes; http.client hands them over as latin-1.
        try:
            raw = raw.encode("latin-1").decode("utf-8")
        except (UnicodeEncodeError, UnicodeDecodeError):
            pass
        return _basename(raw)
    return None


def name_from_url(url: str | None) -> str | None:
    if not url:
        return None
    try:
        path = urlsplit(url).path
    except ValueError:
        return None
    return _basename(unquote(path.rsplit("/", 1)[-1])) if path else None


def ext_for_type(content_type: str | None) -> str:
    if not content_type:
        return ""
    ct = content_type.split(";")[0].strip().lower()
    if ct in ("application/octet-stream", "binary/octet-stream", ""):
        return ""
    return _EXT_FOR_TYPE.get(ct) or mimetypes.guess_extension(ct) or ""


def choose_filename(override: str | None, content_disposition: str | None, final_url: str | None,
                    content_type: str | None) -> str:
    raw = override or parse_content_disposition(content_disposition) or name_from_url(final_url) or "download"
    name = sanitize_component(raw)
    if not split_ext(name)[1] and not override:
        ext = ext_for_type(content_type)
        if ext:
            name = sanitize_component(name + ext)
    return name


def long_path(p: str) -> str:
    """Prefix long Windows paths with \\\\?\\ so >260-character paths still work."""
    if os.name != "nt":
        return p
    ap = os.path.abspath(p)
    if len(ap) < 248 or ap.startswith("\\\\?\\"):
        return ap
    if ap.startswith("\\\\"):
        return "\\\\?\\UNC\\" + ap[2:]
    return "\\\\?\\" + ap
