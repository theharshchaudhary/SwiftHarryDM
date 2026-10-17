"""Runs the real application (swiftdm.app.run) and drives it like a user from
inside its Tk loop. Usage: python app_scenario.py <phase> <json-config>.
Prints one line 'RESULT <json>' before exiting. Licensing is disabled for the
test process only (no requests to the production license server)."""
import json
import os
import sys
import time

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")))

from swiftdm import app  # noqa: E402
from swiftdm.core.models import JobState as S  # noqa: E402

phase, cfg = sys.argv[1], json.loads(sys.argv[2])
result = {"phase": phase, "events": []}


def scenario(root, window, manager, api, violations, shutdown):
    window.settings_store.update(ask_save_location=False, download_dir=cfg["dest"], max_segments=4)
    steps = iter(STEPS[phase](window, manager))
    deadline = time.monotonic() + 120

    def tick():
        try:
            cond = next(steps)
            while cond is None:
                cond = next(steps)
        except StopIteration:
            result["violations"] = list(violations or [])
            print("RESULT " + json.dumps(result), flush=True)
            shutdown()
            return
        wait(cond)

    def wait(cond):
        if time.monotonic() > deadline:
            result["error"] = "timeout"
            print("RESULT " + json.dumps(result), flush=True)
            shutdown()
            return
        if cond():
            root.after(10, tick)
        else:
            root.after(50, lambda: wait(cond))

    root.after(100, tick)


def enter(window, url, quality="best", instant=True):
    window.url_entry.delete(0, "end")
    window.url_entry.insert(0, url)
    window.format_var.set(quality)
    (window.btn_instant if instant else window.btn_add).invoke()


def state_of(window, jid):
    s = window.snaps.get(jid)
    return s.state if s else None


def phase1(window, manager):
    enter(window, cfg["big_url"])
    yield lambda: len(window.rows) == 1
    jid = next(iter(window.rows))
    result["job_id"] = jid
    yield lambda: (manager.progress(jid) is not None and manager.progress(jid).downloaded > 4 * 1024 * 1024)
    result["downloaded_before_close"] = manager.progress(jid).downloaded
    result["events"].append("closing app mid-download")


def phase2(window, manager):
    jid = cfg["job_id"]
    yield lambda: jid in window.rows
    row = window.rows[jid]
    result["row_status_at_startup"] = row.status.cget("text")
    result["buttons_at_startup"] = [b.cget("text") for b in row.buttons.winfo_children()]
    next(b for b in row.buttons.winfo_children() if b.cget("text") == "Resume").invoke()
    yield lambda: state_of(window, jid) == S.COMPLETED
    result["resumed_final_path"] = window.snaps[jid].final_path
    # Collision: the same small file twice must give two files.
    for _ in range(2):
        enter(window, cfg["small_url"])
        yield lambda: sum(1 for s in window.snaps.values() if s.url == cfg["small_url"] and s.state == S.COMPLETED) >= (_ + 1)
    result["small_paths"] = sorted(s.final_path for s in window.snaps.values() if s.url == cfg["small_url"])
    # MP3 from a direct video link goes through yt-dlp + FFmpeg.
    enter(window, cfg["clip_url"], quality="mp3")
    yield lambda: any(s.url == cfg["clip_url"] and s.state in (S.COMPLETED, S.FAILED) for s in window.snaps.values())
    snap = next(s for s in window.snaps.values() if s.url == cfg["clip_url"])
    result["mp3"] = {"state": snap.state.value, "path": snap.final_path, "kind": snap.kind,
                     "error": snap.error.message if snap.error else None}
    result["status_bar"] = window.status_right.cget("text")


STEPS = {"1": phase1, "2": phase2}

code = app.run([], licensing=None, hooks={"ready": scenario, "guard": True, "check_updates": False,
                                          "api_ports": (5081, 5082)})
sys.exit(code)
