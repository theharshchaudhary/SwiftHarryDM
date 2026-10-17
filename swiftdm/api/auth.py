"""Extension pairing and request authentication for the local API.

Pairing: the extension asks to pair; the app shows a one-time 6-digit code; the
user types it into the extension; only then is a 256-bit bearer token issued.
Only a SHA-256 hash of each token is stored. Codes expire after 2 minutes and
allow 5 attempts.
"""
from __future__ import annotations

import hashlib
import hmac
import json
import os
import re
import secrets
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

EXTENSION_ORIGIN = re.compile(r"^chrome-extension://([a-p]{32})$")
CODE_TTL = 120.0
CODE_ATTEMPTS = 5


def extension_id_from_origin(origin: Optional[str]) -> Optional[str]:
    m = EXTENSION_ORIGIN.match(origin or "")
    return m.group(1) if m else None


def _hash(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


class PairingStore:
    def __init__(self, path: Path):
        self.path = Path(path)
        self._lock = threading.Lock()
        self._data = self._load()

    def _load(self) -> dict:
        try:
            d = json.loads(self.path.read_text(encoding="utf-8"))
            if isinstance(d, dict) and isinstance(d.get("extensions"), list):
                return d
        except (OSError, ValueError):
            pass
        return {"extensions": []}

    def _save(self):
        tmp = self.path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self._data, indent=2), encoding="utf-8")
        os.replace(tmp, self.path)

    def issue(self, extension_id: str) -> str:
        token = secrets.token_urlsafe(32)
        with self._lock:
            self._data["extensions"] = [e for e in self._data["extensions"] if e["id"] != extension_id]
            self._data["extensions"].append({"id": extension_id, "token_sha256": _hash(token), "created": time.time()})
            self._save()
        return token

    def verify(self, token: str) -> Optional[str]:
        """Return the extension id owning `token`, or None. Constant-time comparison."""
        h = _hash(token)
        owner = None
        with self._lock:
            for e in self._data["extensions"]:
                if hmac.compare_digest(e["token_sha256"], h):
                    owner = e["id"]
        return owner

    def paired(self) -> list:
        with self._lock:
            return [e["id"] for e in self._data["extensions"]]

    def revoke(self, extension_id: Optional[str] = None):
        with self._lock:
            self._data["extensions"] = [e for e in self._data["extensions"]
                                        if extension_id is not None and e["id"] != extension_id]
            self._save()


@dataclass
class PendingPairing:
    id: str
    extension_id: str
    code: str
    expires: float
    attempts: int = 0
    denied: bool = False


class PairingRequests:
    def __init__(self, clock=time.monotonic):
        self._lock = threading.Lock()
        self._pending: dict = {}
        self.clock = clock

    def create(self, extension_id: str) -> PendingPairing:
        with self._lock:
            now = self.clock()
            self._pending = {k: v for k, v in self._pending.items() if v.expires > now}
            if sum(1 for p in self._pending.values() if p.extension_id == extension_id) >= 3:
                raise PermissionError("too many pending pairing requests")
            p = PendingPairing(secrets.token_urlsafe(16), extension_id, f"{secrets.randbelow(10 ** 6):06d}",
                               now + CODE_TTL)
            self._pending[p.id] = p
            return p

    def deny(self, pairing_id: str):
        with self._lock:
            p = self._pending.get(pairing_id)
            if p:
                p.denied = True

    def confirm(self, pairing_id: str, extension_id: str, code: str) -> str:
        """Returns 'ok', 'wrong_code', 'expired', 'denied', or 'unknown'. Consumes the request on success."""
        with self._lock:
            p = self._pending.get(pairing_id)
            if not p or p.extension_id != extension_id:
                return "unknown"
            if p.denied:
                del self._pending[pairing_id]
                return "denied"
            if self.clock() > p.expires or p.attempts >= CODE_ATTEMPTS:
                del self._pending[pairing_id]
                return "expired"
            p.attempts += 1
            if not hmac.compare_digest(p.code, (code or "").strip()):
                return "wrong_code"
            del self._pending[pairing_id]
            return "ok"


class RateLimiter:
    def __init__(self, limit: int, window: float = 60.0, clock=time.monotonic):
        self.limit, self.window, self.clock = limit, window, clock
        self._hits: list = []
        self._lock = threading.Lock()

    def allow(self) -> bool:
        with self._lock:
            now = self.clock()
            self._hits = [t for t in self._hits if now - t < self.window]
            if len(self._hits) >= self.limit:
                return False
            self._hits.append(now)
            return True
