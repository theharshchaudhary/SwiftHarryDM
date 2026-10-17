"""Versioned user settings stored as JSON (atomic writes, validated on load)."""
import json
import os
import threading
from dataclasses import asdict, dataclass, fields
from pathlib import Path

from .. import appdata

SETTINGS_VERSION = 1
ENGINES = ("legacy", "v2")
COLLISION_POLICIES = ("rename", "overwrite")


def default_download_dir() -> str:
    # Same default location the legacy app used, so existing users find their files.
    home = Path(os.path.expanduser("~"))
    desktop = home / "Desktop"
    base = desktop if desktop.exists() else home / "Documents"
    return str(base / "SwiftHarryDM Downloads")


@dataclass
class Settings:
    version: int = SETTINGS_VERSION
    engine: str = "legacy"  # flips to "v2" at the P8 beta gate
    download_dir: str = ""
    ask_save_location: bool = True
    max_concurrent: int = 3
    max_segments: int = 8
    collision_policy: str = "rename"
    auto_resume: bool = False
    trust_extension: bool = False
    api_port: int = 5001

    def normalized(self) -> "Settings":
        s = Settings(**asdict(self))
        s.version = SETTINGS_VERSION
        if s.engine not in ENGINES:
            s.engine = Settings.engine
        if not s.download_dir:
            s.download_dir = default_download_dir()
        s.max_concurrent = min(max(int(s.max_concurrent), 1), 10)
        s.max_segments = min(max(int(s.max_segments), 1), 16)
        if s.collision_policy not in COLLISION_POLICIES:
            s.collision_policy = "rename"
        if s.api_port not in (5001, 5002):
            s.api_port = 5001
        return s


class SettingsStore:
    def __init__(self, path: Path | None = None):
        self.path = Path(path) if path else appdata.settings_path()
        self._lock = threading.Lock()
        self._settings = self._load()

    def _load(self) -> Settings:
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return Settings().normalized()
        known = {f.name for f in fields(Settings)}
        values = {k: v for k, v in raw.items() if k in known} if isinstance(raw, dict) else {}
        try:
            return Settings(**values).normalized()
        except (TypeError, ValueError):
            return Settings().normalized()

    def get(self) -> Settings:
        with self._lock:
            return Settings(**asdict(self._settings))

    def update(self, **changes) -> Settings:
        with self._lock:
            data = asdict(self._settings)
            data.update(changes)
            self._settings = Settings(**data).normalized()
            tmp = self.path.with_suffix(".json.tmp")
            tmp.write_text(json.dumps(asdict(self._settings), indent=2), encoding="utf-8")
            os.replace(tmp, self.path)
            return Settings(**asdict(self._settings))


def resolve_engine(argv, env, settings: Settings) -> str:
    """CLI flag > SWIFTDM_ENGINE env var > settings."""
    for i, arg in enumerate(argv):
        if arg.startswith("--engine="):
            value = arg.split("=", 1)[1]
            if value in ENGINES:
                return value
        if arg == "--engine" and i + 1 < len(argv) and argv[i + 1] in ENGINES:
            return argv[i + 1]
    value = env.get("SWIFTDM_ENGINE", "")
    if value in ENGINES:
        return value
    return settings.engine
