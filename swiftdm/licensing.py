"""License / trial checks, ported from the legacy app with identical rules.

Behaviour is unchanged (same server, token file, 7-day trial, 60-day offline
grace) pending the product decision on licensing. Differences are purely
engineering: no UI calls from background threads, and license keys and machine
identifiers are never logged. The server URL is injectable for tests only.
"""
from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
import uuid
from dataclasses import dataclass
from datetime import datetime
from typing import Optional

import requests

from .logging_setup import get_logger

log = get_logger("licensing")

LICENSE_SERVER = "https://swift.harshchaudhary.com.np/"
TOKEN_FILE = os.path.join(os.path.expanduser("~"), ".swift_dm_token.json")
MACHINE_ID_FILE = os.path.join(os.path.expanduser("~"), ".swift_dm_machine_id")
TRIAL_DAYS = 7
OFFLINE_GRACE_DAYS = 60


@dataclass
class LicenseStatus:
    text: str
    allowed: bool
    action: Optional[str] = None   # None | "ask_key" | "hardware_changed" | "machine_mismatch" | "trial_started" | "error"
    detail: str = ""


def clean_key(key: str) -> str:
    return (key or "").strip().replace(" ", "").replace("-", "").upper()


class LicenseService:
    def __init__(self, server: str = LICENSE_SERVER, token_file: str = TOKEN_FILE,
                 machine_id_file: str = MACHINE_ID_FILE, app_version: str = "2.0.0", http=None, now=datetime.now):
        self.server = server.rstrip("/")
        self.token_file = token_file
        self.machine_id_file = machine_id_file
        self.app_version = app_version
        self.http = http or requests
        self.now = now

    # ------------------------------------------------------------------ storage
    def machine_id(self) -> str:
        try:
            with open(self.machine_id_file, "r", encoding="utf-8") as f:
                stored = f.read().strip()
                if len(stored) == 64:
                    return stored
        except OSError:
            pass
        base = None
        try:
            import winreg
            key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography")
            guid, _ = winreg.QueryValueEx(key, "MachineGuid")
            winreg.CloseKey(key)
            base = f"WIN_GUID_{guid}"
        except OSError:
            pass
        if not base:
            for args, prefix in ((["wmic", "csproduct", "get", "UUID"], "BIOS_UUID_"),
                                 (["wmic", "baseboard", "get", "serialnumber"], "BOARD_SN_")):
                try:
                    r = subprocess.run(args, capture_output=True, text=True, creationflags=0x08000000, timeout=10)
                    val = r.stdout.strip().split("\n")[1].strip() if r.returncode == 0 else ""
                    if val and val not in ("None", "To be filled by O.E.M."):
                        base = prefix + val
                        break
                except (OSError, IndexError, subprocess.SubprocessError):
                    continue
        if not base:
            base = f"FALLBACK_{platform.node()}_{uuid.getnode()}"
        mid = hashlib.sha256(base.encode()).hexdigest()
        try:
            with open(self.machine_id_file, "w", encoding="utf-8") as f:
                f.write(mid)
        except OSError:
            pass
        return mid

    def load_token(self) -> Optional[dict]:
        try:
            with open(self.token_file, "r", encoding="utf-8") as f:
                data = json.load(f)
            return data if isinstance(data, dict) else None
        except (OSError, ValueError):
            return None

    def save_token(self, data: dict) -> None:
        data = dict(data)
        data.update({"machine_id": self.machine_id(), "windows_platform": True,
                     "last_updated": self.now().isoformat(), "app_version": self.app_version})
        tmp = self.token_file + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(data, f)
        os.replace(tmp, self.token_file)

    def clear_token(self) -> None:
        try:
            os.remove(self.token_file)
        except OSError:
            pass

    # ------------------------------------------------------------------ server calls
    def _post(self, path, payload, timeout):
        resp = self.http.post(f"{self.server}/{path}", json=payload, timeout=timeout)
        resp.raise_for_status()
        return resp.json()

    def verify_online(self, key: str, machine_id: str):
        key = clean_key(key)
        if len(key) != 24:
            return False, "invalid_format"
        try:
            data = self._post("verify", {"license_key": key, "machine_id": machine_id}, 10)
            return bool(data.get("valid", False)), data.get("type")
        except (requests.RequestException, ValueError) as e:
            log.warning("license verification failed: %s", type(e).__name__)
            return False, "connection_error"

    def _offline_trial(self, token) -> Optional[LicenseStatus]:
        if token and "trial_start" in token:
            try:
                elapsed = (self.now() - datetime.fromisoformat(token["trial_start"])).days
            except (TypeError, ValueError):
                return None
            if elapsed < TRIAL_DAYS:
                return LicenseStatus(f"Trial: {TRIAL_DAYS - elapsed} days left (Offline)", True)
        return None

    def start_trial(self, machine_id: str) -> LicenseStatus:
        try:
            data = self._post("trial", {"machine_id": machine_id}, 10)
        except (requests.RequestException, ValueError) as e:
            offline = self._offline_trial(self.load_token())
            if offline:
                return offline
            return LicenseStatus("Status: Check Failed", False, "error", f"Cannot start trial. ({type(e).__name__})")
        if data.get("valid"):
            days = data.get("days_left", TRIAL_DAYS)
            self.save_token({"trial_start": self.now().isoformat(), "trial_start_server": data.get("trial_started"),
                             "days_left": days})
            return LicenseStatus(f"Trial: {days} days left", True, "trial_started")
        return LicenseStatus("Trial expired", False, "ask_key", "Trial expired. Enter a license key to continue.")

    # ------------------------------------------------------------------ main check (runs off the UI thread)
    def check(self) -> LicenseStatus:
        try:
            token = self.load_token()
            mid = self.machine_id()
            if not token:
                return self.start_trial(mid)
            stored = token.get("machine_id")
            if stored and stored != mid:
                return LicenseStatus("License: Hardware change detected", False, "hardware_changed")
            if "license_key" in token:
                valid, ltype = self.verify_online(token["license_key"], mid)
                if valid:
                    token.update({"machine_id": mid, "last_verified": self.now().isoformat(),
                                  "verification_status": "valid", "windows_verified": True})
                    self.save_token(token)
                    return LicenseStatus("License: Active ✓", True)
                if ltype in ("connection_error", "timeout", "server_error"):
                    last = token.get("last_verified")
                    if last:
                        try:
                            days = (self.now() - datetime.fromisoformat(last)).days
                        except (TypeError, ValueError):
                            days = OFFLINE_GRACE_DAYS
                        if days < OFFLINE_GRACE_DAYS:
                            return LicenseStatus(f"License: Active (Offline - {OFFLINE_GRACE_DAYS - days}d left)", True)
                    return LicenseStatus("License: Verification Required", False, "ask_key")
                if ltype == "machine_mismatch":
                    return LicenseStatus("License: Activated on another computer", False, "machine_mismatch")
                return LicenseStatus("License: Invalid", False, "ask_key")
            if "trial_start" in token:
                try:
                    data = self._post("trial", {"machine_id": mid}, 5)
                except (requests.RequestException, ValueError):
                    offline = self._offline_trial(token)
                    return offline or LicenseStatus("Trial expired", False, "ask_key")
                if data.get("valid"):
                    return LicenseStatus(f"Trial: {data.get('days_left', TRIAL_DAYS)} days left", True)
                return LicenseStatus("Trial expired", False, "ask_key")
            return self.start_trial(mid)
        except Exception as e:  # noqa: BLE001 - legacy behaviour: fail open in "limited mode"
            log.error("license check crashed: %s", type(e).__name__)
            return LicenseStatus("Status: Check Failed - Limited Mode", True)

    def activate(self, key: str) -> LicenseStatus:
        key = clean_key(key)
        if len(key) != 24:
            return LicenseStatus("License: Invalid format", False, "ask_key", "License key must be 24 characters long.")
        mid = self.machine_id()
        valid, ltype = self.verify_online(key, mid)
        if valid:
            self.save_token({"license_key": key, "license_type": ltype, "activated_at": self.now().isoformat(),
                             "last_verified": self.now().isoformat()})
            return LicenseStatus("License: Active", True, None, "License activated successfully! Lifetime access granted.")
        if ltype == "connection_error":
            return LicenseStatus("License: Verification Required", False, "error",
                                 "Cannot connect to license server. Please check your internet connection.")
        return LicenseStatus("License: Invalid", False, "ask_key",
                             "License key is invalid or already used on another machine.")
