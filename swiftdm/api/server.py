"""Local API for the browser extension (v2).

Security model:
  * bound to 127.0.0.1 only; Host must be 127.0.0.1/localhost:<port> (DNS-rebinding defence)
  * no CORS headers at all: web pages can neither read responses nor pass a JSON preflight
  * Origin must be absent or chrome-extension://<id>; any http(s) Origin gets 403
  * every endpoint except /health and pairing needs a bearer token from pairing
  * strict request schema, rate limits, and in-app confirmation before downloading
"""
from __future__ import annotations

import secrets
import threading
from typing import Optional
from urllib.parse import urlsplit

from flask import Flask, jsonify, request
from werkzeug.serving import make_server

from .. import __version__
from ..core.manager import validate_url
from ..core.models import JobSpec, JobState
from ..engines.media import formats
from ..logging_setup import get_logger, redact_url
from .auth import PairingRequests, PairingStore, RateLimiter, extension_id_from_origin

log = get_logger("api")
PORTS = (5001, 5002)
FORMATS = ("best", "2160", "1440", "1080", "720", "480", "360", "mp4", "mkv", "mp3", "m4a")
ALLOWED_FIELDS = {"url", "title", "format", "mode", "pageUrl", "type", "thumbnail", "duration", "instantDownload"}


class UiHooks:
    """Main-thread callbacks; every call is marshalled through TkBridge.call_soon."""

    def __init__(self, call_soon, window=None):
        self.call_soon = call_soon
        self.window = window

    def downloads_allowed(self) -> bool:
        return bool(getattr(self.window, "download_allowed", True))

    def show_pairing(self, extension_id, code, on_deny, register):
        def show():
            if self.window:
                register(self.window.show_pairing(extension_id, code, on_deny))
        self.call_soon(show)

    def close_dialog(self, dlg):
        if dlg is not None:
            self.call_soon(dlg.close)

    def confirm(self, req, on_accept, on_reject):
        if self.window:
            self.call_soon(self.window.confirm_extension_download, req, on_accept, on_reject)
        else:
            on_reject()

    def status(self, text):
        if self.window:
            self.call_soon(self.window.set_extension_status, text)


def _error(status, message):
    return jsonify({"success": False, "error": message}), status


def parse_download_request(data) -> dict:
    if not isinstance(data, dict):
        raise ValueError("Request body must be a JSON object.")
    unknown = set(data) - ALLOWED_FIELDS
    if unknown:
        raise ValueError(f"Unknown fields: {', '.join(sorted(unknown))}")
    url = validate_url(data.get("url") if isinstance(data.get("url"), str) else "")
    title = data.get("title") or ""
    if not isinstance(title, str) or len(title) > 300:
        raise ValueError("title must be a string of at most 300 characters")
    fmt = data.get("format") or "best"
    if fmt not in FORMATS:
        raise ValueError("unsupported format")
    mode = data.get("mode") or ("now" if data.get("instantDownload") else "queue")
    if mode not in ("queue", "now"):
        raise ValueError("mode must be 'queue' or 'now'")
    page = data.get("pageUrl") or None
    if page is not None:
        if not isinstance(page, str) or len(page) > 8192 or urlsplit(page).scheme not in ("http", "https"):
            page = None
    return {"url": url, "title": title, "format": fmt, "mode": mode, "pageUrl": page}


class ApiServer:
    def __init__(self, manager, settings_store, pairings: PairingStore, ui: UiHooks, ports=PORTS):
        self.manager = manager
        self.settings_store = settings_store
        self.pairings = pairings
        self.pending = PairingRequests()
        self.ui = ui
        self.ports = ports
        self.port: Optional[int] = None
        self._server = None
        self._thread = None
        self.limits = {"download": RateLimiter(30), "pair": RateLimiter(10), "read": RateLimiter(240)}
        self._dialogs: dict = {}
        self.confirmations: dict = {}
        self.app = self._create_app()

    # ------------------------------------------------------------------ lifecycle
    def start(self) -> Optional[int]:
        for port in self.ports:
            try:
                self._server = make_server("127.0.0.1", port, self.app, threaded=True)
            except OSError:
                continue
            self.port = port
            self._thread = threading.Thread(target=self._server.serve_forever, name="api", daemon=True)
            self._thread.start()
            log.info("extension API listening on 127.0.0.1:%s", port)
            self.ui.status(self.status_text())
            return port
        self.ui.status("🔌 Extension: unavailable (ports 5001/5002 busy)")
        return None

    def stop(self):
        if self._server:
            self._server.shutdown()
            self._server.server_close()
            self._server = None

    def status_text(self) -> str:
        n = len(self.pairings.paired())
        return f"🔌 Extension: ready on port {self.port} · {n} paired" if self.port else "🔌 Extension: off"

    def allowed_hosts(self):
        ports = [self.port] if self.port else list(self.ports)
        return {f"{h}:{p}" for h in ("127.0.0.1", "localhost") for p in ports}

    # ------------------------------------------------------------------ app
    def _create_app(self) -> Flask:
        app = Flask("swiftdm_api")
        app.config["MAX_CONTENT_LENGTH"] = 64 * 1024

        @app.before_request
        def guard():
            if request.host not in self.allowed_hosts():
                return _error(403, "forbidden host")
            origin = request.headers.get("Origin")
            if origin is not None and extension_id_from_origin(origin) is None:
                return _error(403, "forbidden origin")
            return None

        def authed() -> Optional[str]:
            auth = request.headers.get("Authorization", "")
            if not auth.startswith("Bearer "):
                return None
            owner = self.pairings.verify(auth[7:].strip())
            if owner is None:
                return None
            origin_ext = extension_id_from_origin(request.headers.get("Origin"))
            if origin_ext is not None and origin_ext != owner:
                return None
            return owner

        @app.get("/health")
        def health():
            if not self.limits["read"].allow():
                return _error(429, "rate limited")
            return jsonify({"status": "running", "app": "SwiftHarryDM", "version": __version__, "api": 2,
                            "paired": authed() is not None})

        @app.post("/api/pair")
        def pair():
            if not self.limits["pair"].allow():
                return _error(429, "rate limited")
            ext = extension_id_from_origin(request.headers.get("Origin"))
            if ext is None:
                return _error(403, "pairing is only available to browser extensions")
            try:
                p = self.pending.create(ext)
            except PermissionError:
                return _error(429, "too many pairing requests")
            self.ui.show_pairing(ext, p.code, lambda pid=p.id: self.pending.deny(pid),
                                 lambda dlg, pid=p.id: self._dialogs.__setitem__(pid, dlg))
            return jsonify({"success": True, "pairing_id": p.id, "expires_in": 120}), 202

        @app.post("/api/pair/confirm")
        def pair_confirm():
            if not self.limits["pair"].allow():
                return _error(429, "rate limited")
            ext = extension_id_from_origin(request.headers.get("Origin"))
            data = request.get_json(silent=True)
            if ext is None or not isinstance(data, dict):
                return _error(400, "bad request")
            pid, code = data.get("pairing_id"), data.get("code")
            if not isinstance(pid, str) or not isinstance(code, str):
                return _error(400, "bad request")
            result = self.pending.confirm(pid, ext, code)
            if result == "wrong_code":
                return _error(401, "wrong code")
            if result != "ok":
                self.ui.close_dialog(self._dialogs.pop(pid, None))
                return _error(410 if result in ("expired", "denied") else 404, f"pairing {result}")
            token = self.pairings.issue(ext)
            self.ui.close_dialog(self._dialogs.pop(pid, None))
            self.ui.status(self.status_text())
            log.info("extension %s paired", ext)
            return jsonify({"success": True, "token": token})

        @app.post("/api/download")
        def download():
            if not self.limits["download"].allow():
                return _error(429, "rate limited")
            if authed() is None:
                return _error(401, "not paired: open the extension and pair it with SwiftHarryDM")
            if not self.ui.downloads_allowed():
                return _error(403, "downloads are disabled until a license or trial is active")
            try:
                req = parse_download_request(request.get_json(silent=True))
            except ValueError as e:
                return _error(400, str(e))
            settings = self.settings_store.get()
            if settings.trust_extension:
                job_id = self._submit(req)
                return jsonify({"success": True, "status": "queued" if req["mode"] == "queue" else "started",
                                "job_id": job_id})
            rid = secrets.token_urlsafe(8)
            self.confirmations[rid] = "pending"
            self.ui.confirm(req, lambda: self._accept(rid, req), lambda: self.confirmations.__setitem__(rid, "rejected"))
            log.info("extension download awaiting confirmation: %s", redact_url(req["url"]))
            return jsonify({"success": True, "status": "pending_confirmation", "request_id": rid}), 202

        @app.get("/api/requests/<rid>")
        def request_status(rid):
            if authed() is None:
                return _error(401, "not paired")
            state = self.confirmations.get(rid)
            if state is None:
                return _error(404, "unknown request")
            if state in ("pending", "rejected"):
                return jsonify({"success": True, "status": state})
            return jsonify({"success": True, "status": "accepted", "job_id": state})

        @app.get("/api/jobs")
        def jobs():
            if not self.limits["read"].allow():
                return _error(429, "rate limited")
            if authed() is None:
                return _error(401, "not paired")
            out = []
            for s in self.manager.snapshot():
                pct = round(s.downloaded / s.total * 100, 1) if s.total else (100.0 if s.state == JobState.COMPLETED else None)
                out.append({"id": s.id, "title": s.title, "state": s.state.value, "progress": pct,
                            "format": s.format_label, "source": s.source})
            return jsonify({"success": True, "jobs": out})

        @app.errorhandler(404)
        def not_found(_e):
            return _error(404, "not found")

        @app.errorhandler(405)
        def bad_method(_e):
            return _error(405, "method not allowed")

        @app.errorhandler(413)
        def too_big(_e):
            return _error(413, "request too large")

        return app

    def _accept(self, rid, req):
        self.confirmations[rid] = self._submit(req)

    def _submit(self, req) -> str:
        settings = self.settings_store.get()
        spec = JobSpec(url=req["url"], dest_dir=settings.download_dir, media=formats.options_for_label(req["format"]),
                       format_label=req["format"], source="extension", referer=req["pageUrl"])
        job_id = self.manager.submit(spec, start=req["mode"] == "now")
        return job_id
