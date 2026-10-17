"""P7 E2E: the real extension in a real Chrome talks to the real local API.

Verifies: health/discovery, refusal before pairing, the pairing code flow,
an authenticated download that requires in-app confirmation and then
completes, and that an ordinary web page cannot use the API.
"""
import json
import os
import shutil
import socket
import subprocess
from pathlib import Path

import pytest

from swiftdm.api.auth import PairingStore
from swiftdm.api.server import ApiServer, UiHooks
from swiftdm.config.settings import SettingsStore
from swiftdm.core.manager import JobManager
from swiftdm.core.models import JobState as S
from swiftdm.engines.http.transfer import HttpEngine
from swiftdm.store.db import Store
from tests.http.conftest import sha_file
from tests.support.testserver import MiB, Payload, Resource

ROOT = Path(__file__).resolve().parents[2]
CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
pytestmark = [pytest.mark.gui, pytest.mark.skipif(not (os.path.exists(CHROME) and shutil.which("node")),
                                                  reason="needs Chrome and Node.js")]


def _port_free(p):
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", p)) != 0


class Driver:
    def __init__(self, profile):
        self.p = subprocess.Popen(["node", str(ROOT / "tests" / "ext" / "cdp_driver.mjs"), CHROME,
                                   str(ROOT / "extension"), str(profile)],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
        self.hello = json.loads(self.p.stdout.readline())

    def cmd(self, **c):
        self.p.stdin.write(json.dumps(c) + "\n")
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def sw(self, expr):
        r = self.cmd(cmd="sw_eval", expr=expr)
        assert r["ok"], r
        return r["value"]

    def close(self):
        try:
            self.cmd(cmd="quit")
        except (OSError, ValueError):
            pass
        self.p.wait(15)


class Window:
    def __init__(self):
        self.codes, self.confirms, self.download_allowed, self.status = [], [], True, None

    def show_pairing(self, ext, code, on_deny):
        self.codes.append((ext, code))
        return type("D", (), {"close": lambda s: None})()

    def confirm_extension_download(self, req, accept, reject):
        self.confirms.append((req, accept, reject))

    def set_extension_status(self, t):
        self.status = t


@pytest.mark.skipif(not _port_free(5001), reason="port 5001 is in use (is SwiftHarryDM running?)")
def test_real_extension_in_chrome(tmp_path, server):
    store = Store(tmp_path / "s.db")
    settings = SettingsStore(tmp_path / "settings.json")
    settings.update(download_dir=str(tmp_path / "dl"))
    m = JobManager(store, {"http": HttpEngine(store)}, lambda s: "http", settings.get)
    win = Window()
    api = ApiServer(m, settings, PairingStore(tmp_path / "pairings.json"), UiHooks(lambda fn, *a: fn(*a), win),
                    ports=(5001,))
    assert api.start() == 5001
    d = Driver(tmp_path / "profile")
    try:
        assert d.hello.get("ready"), d.hello
        ext_id = d.hello["extensionId"]

        status = d.sw("backgroundManager.status()")
        assert status["connected"] is True and status["paired"] is False and status["api"] == 2

        p = Payload("ext", 3 * MiB)
        file_url = server.add("/from-browser.bin", Resource(p))
        refused = d.sw(f"backgroundManager.handleDownload({{url: {json.dumps(file_url)}, title: 'x', format: 'best', instantDownload: true}})")
        assert refused["success"] is False and m.snapshot() == []          # not paired yet

        start = d.sw("backgroundManager.pairStart()")
        assert start["success"] is True
        assert win.codes and win.codes[-1][0] == ext_id                     # app saw the extension's real Origin
        bad = d.sw(f"backgroundManager.pairConfirm({json.dumps(start['pairingId'])}, '999999')")
        if win.codes[-1][1] != "999999":
            assert bad["success"] is False
        ok = d.sw(f"backgroundManager.pairConfirm({json.dumps(start['pairingId'])}, {json.dumps(win.codes[-1][1])})")
        assert ok["success"] is True
        assert d.sw("backgroundManager.status()")["paired"] is True

        sent = d.sw(f"backgroundManager.handleDownload({{url: {json.dumps(file_url)}, title: 'From Chrome', format: 'best', instantDownload: true, pageUrl: {json.dumps(server.base + '/page')}}})")
        assert sent["success"] is True and sent["status"] == "pending_confirmation"
        assert m.snapshot() == []                                           # waits for the user
        req, accept, _ = win.confirms[-1]
        assert req["title"] == "From Chrome" and req["mode"] == "now"
        accept()
        job_id = api.confirmations[sent["requestId"]]
        snap = m.wait_for(job_id, {S.COMPLETED, S.FAILED}, 30)
        assert snap.state == S.COMPLETED and snap.source == "extension"
        assert sha_file(snap.final_path) == p.sha256()
        jobs = d.sw("backgroundManager.jobs()")
        assert jobs["success"] and jobs["jobs"][0]["state"] == "COMPLETED"

        # An ordinary web page must not be able to drive or read the API.
        page = server.add("/page", Resource(Payload("page", 10), content_type="text/html"))
        assert d.cmd(cmd="page_open", url=page)["ok"]
        before = len(m.snapshot())
        attack = d.cmd(cmd="page_eval", expr=(
            "fetch('http://127.0.0.1:5001/api/download', {method: 'POST', headers: {'Content-Type': 'application/json'},"
            f" body: JSON.stringify({{url: {json.dumps(file_url)}, mode: 'now'}})}})"
            ".then(r => 'status ' + r.status).catch(e => 'blocked: ' + e.message)"))
        assert attack["ok"] and attack["value"].startswith("blocked"), attack
        simple = d.cmd(cmd="page_eval", expr=(
            "fetch('http://127.0.0.1:5001/api/download', {method: 'POST', mode: 'no-cors', headers: {'Content-Type': 'text/plain'},"
            f" body: JSON.stringify({{url: {json.dumps(file_url)}, mode: 'now'}})}})"
            ".then(r => 'sent opaque').catch(e => 'blocked: ' + e.message)"))
        assert simple["ok"], simple
        read = d.cmd(cmd="page_eval", expr=(
            "fetch('http://127.0.0.1:5001/health').then(r => r.text()).catch(e => 'blocked: ' + e.message)"))
        assert read["ok"] and read["value"].startswith("blocked"), read
        assert len(m.snapshot()) == before and len(win.confirms) == 1        # nothing queued, nothing to confirm
    finally:
        d.close()
        api.stop()
        m.shutdown(3)
