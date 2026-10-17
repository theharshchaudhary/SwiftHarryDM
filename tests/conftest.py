import os
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tests.support.testserver import TestServer  # noqa: E402


@pytest.fixture
def server():
    srv = TestServer()
    yield srv
    srv.close()


@pytest.fixture
def app_home(tmp_path, monkeypatch):
    home = tmp_path / "appdata"
    home.mkdir()
    monkeypatch.setenv("SWIFTDM_HOME", str(home))
    return home


@pytest.fixture
def fake_userprofile(tmp_path, monkeypatch):
    prof = tmp_path / "profile"
    (prof / "Desktop").mkdir(parents=True)
    monkeypatch.setenv("USERPROFILE", str(prof))
    monkeypatch.setenv("HOME", str(prof))
    return prof
