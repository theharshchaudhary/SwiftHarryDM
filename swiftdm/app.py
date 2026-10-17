"""v2 application entry point: wires storage, engines, manager, UI and local API."""
from __future__ import annotations

import sys
import tkinter as tk
from tkinter import messagebox

from . import __version__, appdata
from .api.auth import PairingStore
from .api.server import ApiServer, UiHooks
from .config.settings import SettingsStore
from .core.manager import JobManager
from .engines.http.transfer import HttpEngine
from .engines.media.engine import MediaEngine
from .engines.router import Router
from .instance import InstanceLock
from .licensing import LicenseService
from .logging_setup import get_logger, setup_logging
from .store.db import Store
from .ui import guard
from .ui.bridge import TkBridge
from .ui.window import MainWindow


def run(argv=None, licensing="default", hooks=None) -> int:
    """hooks (tests only): {"guard": bool, "ready": callable(root, window, manager, api), "api_ports": tuple}."""
    hooks = hooks or {}
    setup_logging(console=sys.stderr is not None)
    log = get_logger("app")
    log.info("SwiftHarryDM %s starting (v2 engine)", __version__)
    lock = InstanceLock(appdata.lock_path())
    if not lock.acquire():
        r = tk.Tk()
        r.withdraw()
        messagebox.showinfo("SwiftHarryDM", "SwiftHarryDM is already running.")
        r.destroy()
        return 1
    settings_store = SettingsStore()
    try:
        store = Store(appdata.db_path())
    except RuntimeError as e:
        r = tk.Tk()
        r.withdraw()
        messagebox.showerror("SwiftHarryDM", str(e))
        r.destroy()
        lock.release()
        return 2
    engines = {"http": HttpEngine(store), "media": MediaEngine()}
    manager = JobManager(store, engines, Router(), settings_store.get)
    root = tk.Tk()
    violations = guard.install(root, strict=False) if hooks.get("guard") else None
    lic = LicenseService(app_version=__version__) if licensing == "default" else licensing
    window = MainWindow(root, manager, settings_store, lic)
    bridge = TkBridge(root, manager, window)
    window.bridge = bridge
    for snap in manager.snapshot():          # jobs recovered from the previous session
        window.on_changed(snap)
    api = ApiServer(manager, settings_store, PairingStore(appdata.pairings_path()), UiHooks(bridge.call_soon, window),
                    ports=hooks.get("api_ports", (5001, 5002)))
    api.start()
    closing = {"done": False}

    def shutdown(*_):
        if closing["done"]:
            return
        closing["done"] = True
        window.log("Closing: pausing active downloads…")
        root.update_idletasks()
        bridge.stop()
        api.stop()
        manager.shutdown(5)
        store.close()
        lock.release()
        root.destroy()

    root.protocol("WM_DELETE_WINDOW", shutdown)
    root.bind("<<AppExit>>", shutdown)
    if lic is not None:
        root.after(300, window.check_license)
    else:
        window.license_var.set("")
    if hooks.get("check_updates", True):
        root.after(3000, window.check_updates)
    if hooks.get("ready"):
        root.after(200, lambda: hooks["ready"](root, window, manager, api, violations, shutdown))
    root.mainloop()
    if not closing["done"]:
        shutdown()
    log.info("SwiftHarryDM exited")
    return 0
