"""P7: local API security and pairing."""
import json
import socket

import pytest
import requests

from swiftdm.api.auth import PairingStore
from swiftdm.api.server import ApiServer, UiHooks
from swiftdm.config.settings import SettingsStore
from swiftdm.core.models import JobState as S
from swiftdm.store.db import Store
from tests.support.fake_engine import FakeEngine, make_manager

EXT_A = "a" * 32
EXT_B = "b" * 32
ORIGIN_A = {"Origin": f"chrome-extension://{EXT_A}"}
ORIGIN_B = {"Origin": f"chrome-extension://{EXT_B}"}
HOST = {"Host": "127.0.0.1:5001"}


class FakeWindow:
    def __init__(self):
        self.codes = []
        self.confirms = []
        self.download_allowed = True
        self.status = None

    def show_pairing(self, ext, code, on_deny):
        self.codes.append((ext, code, on_deny))
        return type("Dlg", (), {"close": lambda self: None})()

    def confirm_extension_download(self, req, on_accept, on_reject):
        self.confirms.append((req, on_accept, on_reject))

    def set_extension_status(self, text):
        self.status = text


@pytest.fixture
def api(tmp_path):
    store = Store(tmp_path / "s.db")
    settings = SettingsStore(tmp_path / "settings.json")
    settings.update(download_dir=str(tmp_path / "dl"))
    m = make_manager(store, FakeEngine(), settings.get())
    m._settings = settings.get
    win = FakeWindow()
    srv = ApiServer(m, settings, PairingStore(tmp_path / "pairings.json"), UiHooks(lambda fn, *a: fn(*a), win),
                    ports=(5001,))
    srv.port = 5001
    client = srv.app.test_client()
    yield type("A", (), {"srv": srv, "c": client, "win": win, "m": m, "settings": settings, "tmp": tmp_path})
    m.shutdown(2)


def pair(a, origin=ORIGIN_A):
    r = a.c.post("/api/pair", headers={**HOST, **origin})
    assert r.status_code == 202
    code = a.win.codes[-1][1]
    r = a.c.post("/api/pair/confirm", json={"pairing_id": r.get_json()["pairing_id"], "code": code},
                 headers={**HOST, **origin})
    assert r.status_code == 200
    return r.get_json()["token"]


def auth(token, origin=ORIGIN_A):
    return {**HOST, **origin, "Authorization": f"Bearer {token}"}


ENDPOINTS = [("get", "/health"), ("post", "/api/pair"), ("post", "/api/pair/confirm"), ("post", "/api/download"),
             ("get", "/api/jobs"), ("options", "/api/download")]


@pytest.mark.parametrize("method,path", ENDPOINTS)
@pytest.mark.parametrize("origin", ["https://evil.example", "http://localhost:3000", "null", "chrome-extension://short"])
def test_web_origins_rejected_everywhere_without_cors(api, method, path, origin):
    r = getattr(api.c, method)(path, headers={**HOST, "Origin": origin}, json={})
    assert r.status_code == 403
    assert not any(h.lower().startswith("access-control-") for h in r.headers.keys())


@pytest.mark.parametrize("host", ["evil.example:5001", "127.0.0.1.nip.io:5001", "localhost:9999", "0.0.0.0:5001"])
def test_dns_rebinding_hosts_rejected(api, host):
    assert api.c.get("/health", headers={"Host": host}).status_code == 403


def test_endpoints_require_pairing(api):
    assert api.c.post("/api/download", json={"url": "https://x.example/a.zip"}, headers={**HOST, **ORIGIN_A}).status_code == 401
    assert api.c.get("/api/jobs", headers={**HOST, **ORIGIN_A}).status_code == 401
    assert api.c.get("/api/jobs", headers=auth("forged-token")).status_code == 401


def test_pairing_flow_and_token_storage(api):
    token = pair(api)
    assert len(token) >= 40
    stored = (api.tmp / "pairings.json").read_text()
    assert token not in stored and EXT_A in stored                 # only a hash is stored
    assert api.c.get("/api/jobs", headers=auth(token)).status_code == 200
    assert api.c.get("/health", headers=auth(token)).get_json()["paired"] is True


def test_token_bound_to_its_extension(api):
    token = pair(api)
    assert api.c.get("/api/jobs", headers=auth(token, ORIGIN_B)).status_code == 401


def test_pairing_requires_extension_origin(api):
    assert api.c.post("/api/pair", headers=HOST).status_code == 403


def test_wrong_codes_then_expiry(api):
    r = api.c.post("/api/pair", headers={**HOST, **ORIGIN_A})
    pid = r.get_json()["pairing_id"]
    real = api.win.codes[-1][1]
    wrong = "000000" if real != "000000" else "111111"
    for _ in range(5):
        assert api.c.post("/api/pair/confirm", json={"pairing_id": pid, "code": wrong},
                          headers={**HOST, **ORIGIN_A}).status_code == 401
    r = api.c.post("/api/pair/confirm", json={"pairing_id": pid, "code": real}, headers={**HOST, **ORIGIN_A})
    assert r.status_code == 410                                     # locked after 5 attempts


def test_pairing_code_cannot_be_used_by_another_extension(api):
    r = api.c.post("/api/pair", headers={**HOST, **ORIGIN_A})
    pid, code = r.get_json()["pairing_id"], api.win.codes[-1][1]
    assert api.c.post("/api/pair/confirm", json={"pairing_id": pid, "code": code},
                      headers={**HOST, **ORIGIN_B}).status_code == 404


def test_denied_pairing(api):
    r = api.c.post("/api/pair", headers={**HOST, **ORIGIN_A})
    pid = r.get_json()["pairing_id"]
    ext, code, deny = api.win.codes[-1]
    deny()
    assert api.c.post("/api/pair/confirm", json={"pairing_id": pid, "code": code},
                      headers={**HOST, **ORIGIN_A}).status_code == 410


@pytest.mark.parametrize("body", [
    {"url": "ftp://x.example/a"}, {"url": "javascript:alert(1)"}, {"url": "file:///C:/Windows/win.ini"},
    {"url": "https://x.example/a", "format": "flac"}, {"url": "https://x.example/a", "title": "x" * 301},
    {"url": "https://x.example/a", "evil": 1}, {"url": 5}, {"url": "https://x.example/a", "mode": "later"}, [],
])
def test_schema_violations(api, body):
    token = pair(api)
    assert api.c.post("/api/download", json=body, headers=auth(token)).status_code == 400


def test_malformed_and_oversized_bodies(api):
    token = pair(api)
    h = auth(token)
    assert api.c.post("/api/download", data="{not json", headers={**h, "Content-Type": "application/json"}).status_code == 400
    big = json.dumps({"url": "https://x.example/a", "title": "x" * 70000})
    assert api.c.post("/api/download", data=big, headers={**h, "Content-Type": "application/json"}).status_code == 413


def test_download_needs_confirmation_by_default(api):
    token = pair(api)
    r = api.c.post("/api/download", json={"url": "https://fake.test/a.bin?size=10", "title": "A", "mode": "now"},
                   headers=auth(token))
    assert r.status_code == 202 and r.get_json()["status"] == "pending_confirmation"
    rid = r.get_json()["request_id"]
    assert api.m.snapshot() == []                                   # nothing starts before the user agrees
    req, accept, reject = api.win.confirms[-1]
    assert req["url"] == "https://fake.test/a.bin?size=10"
    accept()
    st = api.c.get(f"/api/requests/{rid}", headers=auth(token)).get_json()
    assert st["status"] == "accepted"
    snap = api.m.wait_for(st["job_id"], S.COMPLETED, 10)
    assert snap.source == "extension"


def test_rejected_request(api):
    token = pair(api)
    r = api.c.post("/api/download", json={"url": "https://fake.test/b.bin"}, headers=auth(token))
    api.win.confirms[-1][2]()
    assert api.c.get(f"/api/requests/{r.get_json()['request_id']}", headers=auth(token)).get_json()["status"] == "rejected"
    assert api.m.snapshot() == []


def test_trusted_extension_and_queue_mode(api):
    api.settings.update(trust_extension=True)
    token = pair(api)
    r = api.c.post("/api/download", json={"url": "https://fake.test/c.bin?size=5", "mode": "queue"}, headers=auth(token))
    assert r.status_code == 200 and r.get_json()["status"] == "queued"
    snap = api.m.wait_for(r.get_json()["job_id"], S.PAUSED, 5)
    assert snap.state == S.PAUSED and snap.reason == "not started"   # "Add to queue" no longer auto-starts


def test_rate_limit(api):
    api.settings.update(trust_extension=True)
    token = pair(api)
    codes = [api.c.post("/api/download", json={"url": f"https://fake.test/r{i}.bin", "mode": "queue"},
                        headers=auth(token)).status_code for i in range(32)]
    assert codes[:30] == [200] * 30 and codes[30:] == [429, 429]


def test_license_gate(api):
    token = pair(api)
    api.win.download_allowed = False
    assert api.c.post("/api/download", json={"url": "https://fake.test/z.bin"}, headers=auth(token)).status_code == 403


def test_real_server_binds_loopback_only(tmp_path):
    from swiftdm.config.settings import SettingsStore
    store = Store(tmp_path / "s.db")
    m = make_manager(store, FakeEngine(), SettingsStore(tmp_path / "st.json").get())
    srv = ApiServer(m, SettingsStore(tmp_path / "st.json"), PairingStore(tmp_path / "p.json"),
                    UiHooks(lambda fn, *a: fn(*a), FakeWindow()), ports=(5061,))
    assert srv.start() == 5061
    try:
        assert srv._server.socket.getsockname()[0] == "127.0.0.1"
        r = requests.get("http://127.0.0.1:5061/health", timeout=5)
        assert r.status_code == 200 and r.json()["api"] == 2 and "Access-Control-Allow-Origin" not in r.headers
        lan_ip = socket.gethostbyname(socket.gethostname())
        if not lan_ip.startswith("127."):
            with pytest.raises(requests.ConnectionError):
                requests.get(f"http://{lan_ip}:5061/health", timeout=3)
    finally:
        srv.stop()
        m.shutdown(2)
