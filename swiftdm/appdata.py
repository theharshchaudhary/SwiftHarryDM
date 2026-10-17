"""Locations of per-user application state.

Everything lives under %LOCALAPPDATA%\\SwiftHarryDM unless SWIFTDM_HOME is set
(used by tests and portable installs).
"""
import os
import sys
from pathlib import Path


def app_dir() -> Path:
    override = os.environ.get("SWIFTDM_HOME")
    if override:
        base = Path(override)
    else:
        root = os.environ.get("LOCALAPPDATA") or os.path.join(os.path.expanduser("~"), "AppData", "Local")
        base = Path(root) / "SwiftHarryDM"
    base.mkdir(parents=True, exist_ok=True)
    return base


def db_path() -> Path:
    return app_dir() / "state.db"


def settings_path() -> Path:
    return app_dir() / "settings.json"


def pairings_path() -> Path:
    return app_dir() / "pairings.json"


def lock_path() -> Path:
    return app_dir() / "instance.lock"


def logs_dir() -> Path:
    d = app_dir() / "logs"
    d.mkdir(exist_ok=True)
    return d


def tools_dir() -> Path:
    d = app_dir() / "tools"
    d.mkdir(exist_ok=True)
    return d


def bundle_dir() -> Path:
    """Directory holding bundled resources (PyInstaller _MEIPASS or the repo root)."""
    if getattr(sys, "frozen", False):
        return Path(getattr(sys, "_MEIPASS", os.path.dirname(sys.executable)))
    return Path(__file__).resolve().parent.parent
