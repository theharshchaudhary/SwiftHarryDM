"""P0 hotfix: the legacy extension API must reject web origins and foreign hosts."""
import pytest

from tests.support import legacy_loader


class _Root:
    def __init__(self):
        self.scheduled = []

    def after(self, ms, fn):
        self.scheduled.append(ms)


class _Log:
    def insert(self, *a):
        pass

    def see(self, *a):
        pass


@pytest.fixture
def client(fake_userprofile):
    ns = legacy_loader.load(
        {"reject_untrusted_callers", "health_check", "handle_download", "get_queue_status",
         "sanitize_filename", "get_filename_from_url"},
        extra_globals={"root": _Root(), "log_text": _Log(), "download_queue": [], "paused_flags": [],
                       "CURRENT_VERSION": "1.0.0", "tk": type("tk", (), {"END": "end"})},
        assigns={"app", "_ALLOWED_HOSTS"})
    return ns, ns["app"].test_client()


HOST = {"Host": "127.0.0.1:5001"}


def test_evil_origin_preflight_rejected_without_cors(client):
    ns, c = client
    r = c.options("/api/download", headers={**HOST, "Origin": "https://evil.example",
                                            "Access-Control-Request-Method": "POST"})
    assert r.status_code == 403
    assert "Access-Control-Allow-Origin" not in r.headers


def test_evil_origin_post_rejected(client):
    ns, c = client
    r = c.post("/api/download", json={"url": "https://x.example/a.exe"}, headers={**HOST, "Origin": "https://evil.example"})
    assert r.status_code == 403
    assert ns["download_queue"] == []


def test_wrong_host_rejected(client):
    ns, c = client
    r = c.get("/health", headers={"Host": "attacker.example:5001"})
    assert r.status_code == 403


def test_extension_origin_still_works(client):
    ns, c = client
    r = c.post("/api/download", json={"url": "https://x.example/a.zip", "title": "a", "format": "best"},
               headers={**HOST, "Origin": "chrome-extension://abcdefghijklmnopabcdefghijklmnop"})
    assert r.status_code == 200
    assert len(ns["download_queue"]) == 1
    assert "Access-Control-Allow-Origin" not in r.headers
