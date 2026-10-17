"""Licensing rules (ported unchanged from legacy) against a fake server, the
update-version check, and the checksum-verified yt-dlp updater."""
import hashlib
import io
import json
from datetime import datetime, timedelta

import pytest
import requests

from swiftdm import updates
from swiftdm.licensing import LicenseService


class FakeResp:
    def __init__(self, data=None, status=200, text=""):
        self._data, self.status_code, self.text = data, status, text

    def json(self):
        return self._data

    def raise_for_status(self):
        if self.status_code >= 400:
            raise requests.HTTPError(str(self.status_code))


class FakeHttp:
    def __init__(self, routes=None, down=False):
        self.routes = routes or {}
        self.down = down
        self.calls = []

    def post(self, url, json=None, timeout=None):
        self.calls.append((url.rsplit("/", 1)[-1], json))
        if self.down:
            raise requests.ConnectionError("offline")
        return FakeResp(self.routes[url.rsplit("/", 1)[-1]](json))

    def get(self, url, timeout=None):
        if self.down:
            raise requests.ConnectionError("offline")
        return self.routes["get"]()


@pytest.fixture
def svc_factory(tmp_path):
    def make(http, now=datetime(2026, 10, 17, 12, 0)):
        mid = tmp_path / "mid"
        mid.write_text("a" * 64)
        return LicenseService(server="http://license.test", token_file=str(tmp_path / "token.json"),
                              machine_id_file=str(mid), http=http, now=lambda: now)
    return make


def test_first_run_starts_trial(svc_factory):
    s = svc_factory(FakeHttp({"trial": lambda b: {"valid": True, "days_left": 7}}))
    st = s.check()
    assert st.allowed and st.action == "trial_started" and "7 days" in st.text
    assert "trial_start" in s.load_token()


def test_expired_trial_asks_for_key(svc_factory):
    s = svc_factory(FakeHttp({"trial": lambda b: {"valid": False}}))
    st = s.check()
    assert not st.allowed and st.action == "ask_key"


def test_offline_trial_counts_days_locally(svc_factory):
    s = svc_factory(FakeHttp(down=True))
    s.save_token({"trial_start": (datetime(2026, 10, 17, 12) - timedelta(days=3)).isoformat()})
    st = s.check()
    assert st.allowed and "4 days left (Offline)" in st.text


def test_license_valid_and_offline_grace(svc_factory):
    ok = svc_factory(FakeHttp({"verify": lambda b: {"valid": True, "type": "lifetime"}}))
    st = ok.activate("ABCD-EFGH-IJKL-MNOP-QRST-UVWX")
    assert st.allowed
    assert ok.check().text.startswith("License: Active")
    offline = svc_factory(FakeHttp(down=True), now=datetime(2026, 10, 17, 12) + timedelta(days=10))
    st = offline.check()
    assert st.allowed and "Offline - 50d left" in st.text
    expired = svc_factory(FakeHttp(down=True), now=datetime(2026, 10, 17, 12) + timedelta(days=61))
    assert expired.check().action == "ask_key"


def test_license_key_format_and_mismatch(svc_factory):
    s = svc_factory(FakeHttp({"verify": lambda b: {"valid": False, "type": "machine_mismatch"}}))
    assert s.activate("short").detail.startswith("License key must be 24")
    s.save_token({"license_key": "A" * 24, "last_verified": datetime(2026, 10, 1).isoformat()})
    assert s.check().action == "machine_mismatch"


def test_hardware_change_detected(svc_factory, tmp_path):
    s = svc_factory(FakeHttp({}))
    (tmp_path / "token.json").write_text(json.dumps({"machine_id": "b" * 64, "license_key": "A" * 24}))
    assert s.check().action == "hardware_changed"


def test_license_key_never_logged(svc_factory, caplog):
    s = svc_factory(FakeHttp(down=True))
    s.activate("SECRETKEY1234567890ABCDE")
    assert "SECRETKEY" not in caplog.text


@pytest.mark.parametrize("server_text,current,expected", [
    ("2.1.0", "2.0.0", "2.1.0"), ("2.0.0", "2.0.0", None), ("1.0.0", "2.0.0", None), ("2.0.1\n", "2.0", "2.0.1"),
])
def test_update_check(server_text, current, expected):
    http = FakeHttp({"get": lambda: FakeResp(text=server_text)})
    assert updates.newer_version(current, http=http) == expected


def test_update_check_rejects_garbage():
    http = FakeHttp({"get": lambda: FakeResp(text="<html>502 Bad Gateway</html>")})
    with pytest.raises(ValueError):
        updates.newer_version("2.0.0", http=http)


def _opener(files):
    class R(io.BytesIO):
        def __enter__(self):
            return self

        def __exit__(self, *a):
            self.close()

    return lambda url, timeout=None: R(files[url.rsplit("/", 1)[-1]])


def test_ytdlp_update_rejects_checksum_mismatch(app_home):
    from swiftdm.engines.media import tools
    files = {"SHA2-256SUMS": b"0" * 64 + b"  yt-dlp.exe\n", "yt-dlp.exe": b"not the real binary"}
    with pytest.raises(RuntimeError, match="SHA-256"):
        tools.update_ytdlp("https://x/", opener=_opener(files))
    assert not (app_home / "tools" / "yt-dlp.exe").exists()


def test_ytdlp_update_rejects_binary_that_does_not_run(app_home):
    from swiftdm.engines.media import tools
    body = b"MZ but not really an executable"
    files = {"SHA2-256SUMS": hashlib.sha256(body).hexdigest().encode() + b"  yt-dlp.exe\n", "yt-dlp.exe": body}
    with pytest.raises(RuntimeError, match="does not run"):
        tools.update_ytdlp("https://x/", opener=_opener(files))
    assert not (app_home / "tools" / "yt-dlp.exe").exists()
