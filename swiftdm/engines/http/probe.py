"""HTTP request helpers: redirect policy, error mapping, and the initial probe."""
from __future__ import annotations

import email.utils
import time
from dataclasses import dataclass
from typing import Optional
from urllib.parse import urljoin, urlsplit

import requests

from ... import __version__
from ...core.cancel import CancelToken
from ...core.errors import ErrorCode, SdmError
from .validators import parse_content_range

USER_AGENT = f"Mozilla/5.0 (Windows NT 10.0; Win64; x64) SwiftHarryDM/{__version__}"
MAX_REDIRECTS = 10
REDIRECT_CODES = {301, 302, 303, 307, 308}
CONNECT_TIMEOUT, READ_TIMEOUT = 10.0, 30.0


def new_session() -> requests.Session:
    s = requests.Session()
    adapter = requests.adapters.HTTPAdapter(max_retries=0, pool_connections=2, pool_maxsize=4)
    s.mount("http://", adapter)
    s.mount("https://", adapter)
    s.headers.update({"User-Agent": USER_AGENT, "Accept-Encoding": "identity", "Accept": "*/*"})
    s.trust_env = True   # honour system proxy settings
    return s


def next_redirect(current: str, location: Optional[str]) -> str:
    if not location:
        raise SdmError(ErrorCode.HTTP_STATUS, "The server sent a redirect without a destination.")
    nxt = urljoin(current, location)
    scheme = urlsplit(nxt).scheme.lower()
    if scheme not in ("http", "https"):
        raise SdmError(ErrorCode.REDIRECT_REFUSED, f"Refused a redirect to an unsupported address ({scheme}:).")
    if urlsplit(current).scheme.lower() == "https" and scheme == "http":
        raise SdmError(ErrorCode.REDIRECT_REFUSED,
                       "Refused a redirect from a secure (https) to an insecure (http) address.")
    return nxt


def _retry_after(value: Optional[str]) -> Optional[float]:
    if not value:
        return None
    try:
        return max(0.0, min(float(value), 600.0))
    except ValueError:
        dt = email.utils.parsedate_to_datetime(value) if value else None
        if dt is None:
            return None
        return max(0.0, min(dt.timestamp() - time.time(), 600.0))


def status_error(resp: requests.Response) -> SdmError:
    code = resp.status_code
    if code in (401, 403):
        return SdmError(ErrorCode.FORBIDDEN, f"The server denied access ({code}). The link may require a login or "
                        "may have expired.", http_status=code)
    if code in (404, 410):
        return SdmError(ErrorCode.HTTP_STATUS, f"The file was not found on the server ({code}).", http_status=code)
    if code == 429:
        return SdmError(ErrorCode.RATE_LIMITED, "The server is limiting requests (429). Retrying later.",
                        retryable=True, http_status=code, retry_after=_retry_after(resp.headers.get("Retry-After")))
    if code == 408 or code >= 500:
        return SdmError(ErrorCode.HTTP_STATUS, f"The server had a temporary problem ({code}).", retryable=True,
                        http_status=code, retry_after=_retry_after(resp.headers.get("Retry-After")))
    return SdmError(ErrorCode.HTTP_STATUS, f"The server returned an unexpected response ({code}).", http_status=code)


def map_request_exception(e: Exception) -> SdmError:
    if isinstance(e, requests.exceptions.SSLError):
        return SdmError(ErrorCode.TLS, "The secure connection could not be verified (certificate problem).", str(e))
    if isinstance(e, (requests.exceptions.ConnectTimeout, requests.exceptions.ReadTimeout)):
        return SdmError(ErrorCode.TIMEOUT, "The server stopped responding.", str(e), retryable=True)
    if isinstance(e, (requests.exceptions.InvalidURL, requests.exceptions.MissingSchema,
                      requests.exceptions.InvalidSchema)):
        return SdmError(ErrorCode.NETWORK, "The address is not valid.", str(e))
    return SdmError(ErrorCode.NETWORK, "The connection to the server failed.", str(e), retryable=True)


def open_url(session: requests.Session, url: str, headers: dict, cancel: CancelToken,
             timeout=(CONNECT_TIMEOUT, READ_TIMEOUT)):
    """GET with manual redirect handling. Returns (response, final_url). Caller closes the response."""
    current = url
    for _ in range(MAX_REDIRECTS + 1):
        cancel.raise_if_set()
        try:
            resp = session.get(current, headers=headers, stream=True, allow_redirects=False, timeout=timeout)
        except requests.RequestException as e:
            raise map_request_exception(e) from e
        if resp.status_code in REDIRECT_CODES:
            location = resp.headers.get("Location")
            resp.close()
            current = next_redirect(current, location)
            continue
        return resp, current
    raise SdmError(ErrorCode.HTTP_STATUS, f"Too many redirects (more than {MAX_REDIRECTS}).")


@dataclass
class HttpProbe:
    final_url: str
    total: Optional[int]
    ranges: bool
    etag: Optional[str]
    last_modified: Optional[str]
    content_type: Optional[str]
    content_disposition: Optional[str]
    content_encoding: Optional[str]

    @property
    def is_html(self) -> bool:
        ct = (self.content_type or "").split(";")[0].strip().lower()
        return ct in ("text/html", "application/xhtml+xml")


def probe(session: requests.Session, url: str, cancel: CancelToken, extra_headers=None,
          timeout=(CONNECT_TIMEOUT, READ_TIMEOUT)) -> HttpProbe:
    headers = {"Range": "bytes=0-0", **(extra_headers or {})}
    resp, final = open_url(session, url, headers, cancel, timeout)
    try:
        h = resp.headers
        encoding = (h.get("Content-Encoding") or "").strip().lower() or None
        if encoding == "identity":
            encoding = None
        total, ranges = None, False
        if resp.status_code == 206:
            cr = parse_content_range(h.get("Content-Range"))
            if not cr or cr[0] != 0 or cr[1] != 0:
                # Broken range support: fall back to one plain stream of unknown length.
                total, ranges = None, False
            else:
                total, ranges = cr[2], cr[2] is not None and encoding is None
        elif resp.status_code == 200:
            cl = h.get("Content-Length")
            if cl and cl.isdigit() and encoding is None:
                total = int(cl)
        elif resp.status_code == 416:
            cr = (h.get("Content-Range") or "").strip()
            if cr.endswith("/0"):
                total, ranges = 0, True
            else:
                raise status_error(resp)
        else:
            raise status_error(resp)
        return HttpProbe(final, total, ranges, h.get("ETag"), h.get("Last-Modified"), h.get("Content-Type"),
                         h.get("Content-Disposition"), encoding)
    finally:
        resp.close()
