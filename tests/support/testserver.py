"""Fault-injecting HTTP/1.1 test server used by the engine and legacy tests.

Resources are registered per path. Payloads are deterministic so tests can
verify SHA-256 without holding large files in memory.
"""
from __future__ import annotations

import gzip
import hashlib
import http.server
import re
import socket
import socketserver
import threading
import time
from dataclasses import dataclass, field
from typing import Callable, Optional

MiB = 1024 * 1024
BLOCK = 64 * 1024


class Payload:
    """Deterministic content of `size` bytes generated in 64 KiB blocks."""

    def __init__(self, seed: str, size: int):
        self.seed = seed.encode()
        self.size = size
        self._cache: dict[int, bytes] = {}

    def _block(self, i: int) -> bytes:
        b = self._cache.get(i)
        if b is None:
            b = hashlib.sha256(self.seed + i.to_bytes(8, "little")).digest() * (BLOCK // 32)
            if i == 0:
                # Fixed binary magic so content sniffers never mistake test data for HTML.
                b = b"\x00SDMTEST" + b[8:]
            if len(self._cache) > 64:
                self._cache.clear()
            self._cache[i] = b
        return b

    def read(self, start: int, length: int) -> bytes:
        out = bytearray()
        pos = start
        end = min(start + length, self.size)
        while pos < end:
            i, off = divmod(pos, BLOCK)
            take = min(BLOCK - off, end - pos)
            out += self._block(i)[off:off + take]
            pos += take
        return bytes(out)

    def sha256(self) -> str:
        h = hashlib.sha256()
        pos = 0
        while pos < self.size:
            chunk = self.read(pos, 4 * MiB)
            h.update(chunk)
            pos += len(chunk)
        return h.hexdigest()

    def all(self) -> bytes:
        return self.read(0, self.size)


@dataclass
class Fault:
    """Cut or stall a response after `after_bytes` of body on matching requests."""
    after_bytes: int
    kind: str = "cut"              # "cut" | "stall"
    stall_seconds: float = 60.0
    when: Callable[[int, Optional[str]], bool] = lambda n, rng: True  # (request number, Range header)
    times: int = 1                 # how many times the fault fires (-1 = forever)


@dataclass
class Resource:
    payload: Payload
    ranges: bool = True
    etag: Optional[str] = None
    last_modified: Optional[str] = None
    content_type: str = "application/octet-stream"
    content_disposition: Optional[str] = None
    gzip: bool = False             # Content-Encoding: gzip (whole body, ranges disabled)
    chunked: bool = False          # no Content-Length
    statuses: list = field(default_factory=list)   # statuses to return for the first GETs
    retry_after: Optional[str] = None
    throttle_bps: Optional[int] = None             # per connection
    faults: list = field(default_factory=list)
    range_bug: Optional[str] = None                # "ignore" | "wrong_start" | "wrong_total"
    replace_after: Optional[int] = None            # after N GETs switch to `replacement`
    replacement: Optional["Resource"] = None
    redirect_to: Optional[str] = None
    redirect_status: int = 302
    requests: int = 0
    bytes_sent: int = 0


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server: "TestServer._Server"

    def log_message(self, *a):
        pass

    def do_HEAD(self):
        self._serve(head=True)

    def do_GET(self):
        self._serve(head=False)

    def _serve(self, head: bool):
        ts: TestServer = self.server.owner
        path = self.path.split("?")[0]
        rng = self.headers.get("Range")
        if_range = self.headers.get("If-Range")
        with ts.lock:
            res = ts.routes.get(path)
            if res is not None and not head:
                res.requests += 1
                n = res.requests
                if res.replace_after is not None and n > res.replace_after and res.replacement:
                    rep = res.replacement
                    rep.requests = n
                    ts.routes[path] = rep
                    res = rep
            else:
                n = res.requests if res else 0
            ts.log.append((self.command, path, rng, if_range))
        if res is None:
            return self._simple(404, b"not found")
        if res.redirect_to:
            self.send_response(res.redirect_status)
            self.send_header("Location", res.redirect_to)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if not head and res.statuses and n <= len(res.statuses):
            code = res.statuses[n - 1]
            extra = {"Retry-After": res.retry_after} if (code == 429 and res.retry_after) else {}
            return self._simple(code, b"error", extra)
        size = res.payload.size
        start, end, status = 0, size - 1, 200
        ranges = res.ranges and not res.gzip
        if rng and ranges:
            m = re.match(r"bytes=(\d*)-(\d*)$", rng.strip())
            valid_if_range = True
            if if_range:
                valid_if_range = if_range in (res.etag, res.last_modified)
            if m and valid_if_range and res.range_bug != "ignore":
                if m.group(1) == "":
                    suffix = int(m.group(2))
                    start = max(size - suffix, 0)
                else:
                    start = int(m.group(1))
                    if m.group(2):
                        end = min(int(m.group(2)), size - 1)
                if start >= size:
                    return self._simple(416, b"", {"Content-Range": f"bytes */{size}"})
                status = 206
        body_start, body_len = start, (end - start + 1) if size else 0
        self.send_response(status)
        self.send_header("Content-Type", res.content_type)
        if res.etag:
            self.send_header("ETag", res.etag)
        if res.last_modified:
            self.send_header("Last-Modified", res.last_modified)
        if res.content_disposition:
            self.send_header("Content-Disposition", res.content_disposition)
        if ranges:
            self.send_header("Accept-Ranges", "bytes")
        if status == 206:
            cr_start = start + 1 if res.range_bug == "wrong_start" else start
            total = size + 7 if res.range_bug == "wrong_total" else size
            self.send_header("Content-Range", f"bytes {cr_start}-{end}/{total}")
        if res.gzip:
            body = gzip.compress(res.payload.all())
            self.send_header("Content-Encoding", "gzip")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if not head:
                self.wfile.write(body)
            return
        if res.chunked:
            self.send_header("Transfer-Encoding", "chunked")
        else:
            self.send_header("Content-Length", str(body_len))
        self.end_headers()
        if head:
            return
        self._stream(ts, res, n, rng, body_start, body_len)

    def _stream(self, ts, res: Resource, n, rng, start, length):
        fault = None
        with ts.lock:
            for f in res.faults:
                if f.times != 0 and f.when(n, rng):
                    fault = f
                    if f.times > 0:
                        f.times -= 1
                    break
        sent = 0
        t0 = time.monotonic()
        try:
            while sent < length:
                take = min(BLOCK, length - sent)
                if fault and sent + take > fault.after_bytes:
                    take = max(fault.after_bytes - sent, 0)
                chunk = res.payload.read(start + sent, take) if take else b""
                if chunk:
                    self._write(chunk, res.chunked)
                    sent += len(chunk)
                    with ts.lock:
                        res.bytes_sent += len(chunk)
                        ts.total_bytes_sent += len(chunk)
                if fault and sent >= fault.after_bytes:
                    if fault.kind == "stall":
                        time.sleep(fault.stall_seconds)
                    self.wfile.flush()
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.close_connection = True
                    return
                if res.throttle_bps:
                    expected = sent / res.throttle_bps
                    lag = expected - (time.monotonic() - t0)
                    if lag > 0:
                        time.sleep(lag)
            if res.chunked:
                self.wfile.write(b"0\r\n\r\n")
        except (ConnectionError, OSError):
            self.close_connection = True

    def _write(self, chunk: bytes, chunked: bool):
        if chunked:
            self.wfile.write(f"{len(chunk):X}\r\n".encode() + chunk + b"\r\n")
        else:
            self.wfile.write(chunk)

    def _simple(self, code, body, headers=None):
        self.send_response(code)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)


class TestServer:
    class _Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
        daemon_threads = True
        allow_reuse_address = True
        request_queue_size = 64

    def __init__(self):
        self.routes: dict[str, Resource] = {}
        self.log: list = []
        self.lock = threading.Lock()
        self.total_bytes_sent = 0
        self._srv = self._Server(("127.0.0.1", 0), _Handler)
        self._srv.owner = self
        self._thread = threading.Thread(target=self._srv.serve_forever, daemon=True)
        self._thread.start()

    @property
    def base(self) -> str:
        return f"http://127.0.0.1:{self._srv.server_address[1]}"

    def add(self, path: str, res: Resource) -> str:
        with self.lock:
            self.routes[path] = res
        return self.base + path

    def requests_for(self, path):
        with self.lock:
            return [e for e in self.log if e[1] == path and e[0] == "GET"]

    def close(self):
        self._srv.shutdown()
        self._srv.server_close()
