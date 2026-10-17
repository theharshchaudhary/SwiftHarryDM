"""Main window for the v2 engine. Layout and theme follow the legacy UI; all
widget updates arrive through TkBridge on the Tk main thread."""
from __future__ import annotations

import collections
import os
import threading
import time
import tkinter as tk
import webbrowser
from tkinter import filedialog, messagebox, simpledialog, ttk

from .. import __version__, appdata
from ..core.manager import validate_url
from ..core.models import JobSnapshot, JobSpec, JobState as S, ProgressSnapshot
from ..engines.media import formats, tools
from ..logging_setup import get_logger
from .. import updates
from .dialogs import DetailsWindow, PairingDialog, SettingsDialog, about_text, show_text
from .rows import ACTIVE, JobRow
from .theme import COLORS, configure_styles, format_speed

log = get_logger("ui")
FORMAT_CHOICES = ["best", "2160", "1440", "1080", "720", "480", "360", "mp4", "mkv", "mp3", "m4a"]


class MainWindow:
    def __init__(self, root: tk.Tk, manager, settings_store, licensing=None):
        self.root = root
        self.manager = manager
        self.settings_store = settings_store
        self.licensing = licensing
        self.bridge = None                     # set by the app after construction
        self.rows: dict = {}
        self.snaps: dict = {}
        self.speeds: dict = {}
        self.logs: dict = collections.defaultdict(lambda: collections.deque(maxlen=300))
        self.detail_windows: dict = {}
        self.download_allowed = True
        root.title(f"SwiftHarryDM v{__version__} - Download Manager")
        root.configure(bg=COLORS["dark_bg"])
        root.minsize(1000, 600)
        w, h = 1200, 720
        root.geometry(f"{w}x{h}+{(root.winfo_screenwidth() - w) // 2}+{(root.winfo_screenheight() - h) // 2}")
        icon = appdata.bundle_dir() / "app.ico"
        if icon.exists():
            try:
                root.iconbitmap(str(icon))
            except tk.TclError:
                pass
        configure_styles(root)
        self._build_menu()
        self._build_header()
        self._build_controls()
        self._build_queue()
        self._build_log()
        self._build_status_bar()
        self._bind_keys()
        self._tick_status()

    # ================================================================== layout
    def _build_menu(self):
        m = tk.Menu(self.root)
        f = tk.Menu(m, tearoff=0)
        f.add_command(label="Add to Queue", command=self.add_to_queue, accelerator="Ctrl+N")
        f.add_command(label="Instant Download", command=self.instant_download, accelerator="Ctrl+I")
        f.add_separator()
        f.add_command(label="Open Download Folder", command=self.open_download_folder)
        f.add_command(label="Settings…", command=self.open_settings)
        f.add_separator()
        f.add_command(label="Exit", command=lambda: self.root.event_generate("<<AppExit>>"))
        m.add_cascade(label="File", menu=f)
        d = tk.Menu(m, tearoff=0)
        d.add_command(label="Start All", command=self.manager.start_all, accelerator="F5")
        d.add_command(label="Pause All", command=self.manager.pause_all, accelerator="F6")
        d.add_command(label="Resume All", command=self.manager.resume_all, accelerator="F7")
        d.add_separator()
        d.add_command(label="Clear Completed", command=self.clear_completed)
        m.add_cascade(label="Download", menu=d)
        if self.licensing:
            lm = tk.Menu(m, tearoff=0)
            lm.add_command(label="Enter License Key…", command=self.ask_license_key)
            lm.add_command(label="Check License Status", command=self.check_license)
            lm.add_command(label="View License Information", command=self.show_license_file)
            m.add_cascade(label="License", menu=lm)
        h = tk.Menu(m, tearoff=0)
        h.add_command(label="Diagnostics…", command=self.show_diagnostics)
        h.add_command(label="Update yt-dlp", command=self.update_ytdlp)
        h.add_command(label="Open Log Folder", command=lambda: os.startfile(appdata.logs_dir()))
        h.add_command(label="Check for Updates", command=lambda: self.check_updates(auto=False))
        h.add_separator()
        h.add_command(label="About SwiftHarryDM", command=lambda: show_text(self.root, "About", about_text(), 560, 360))
        m.add_cascade(label="Help", menu=h)
        self.root.config(menu=m)

    def _build_header(self):
        hdr = tk.Frame(self.root, bg=COLORS["header_bg"], height=72)
        hdr.pack(fill="x", side="top")
        hdr.pack_propagate(False)
        inner = tk.Frame(hdr, bg=COLORS["header_bg"])
        inner.pack(expand=True, fill="both", padx=20)
        left = tk.Frame(inner, bg=COLORS["header_bg"])
        left.pack(side="left")
        tk.Label(left, text="🚀", font=("Arial", 22), bg=COLORS["header_bg"], fg="white").pack(side="left", padx=(0, 10))
        tk.Label(left, text="SwiftHarryDM", font=("Arial", 20, "bold"), bg=COLORS["header_bg"],
                 fg="white").pack(side="left")
        tk.Label(left, text=f"v{__version__}", font=("Arial", 10), bg=COLORS["header_bg"],
                 fg="#CCCCCC").pack(side="left", padx=(5, 0))
        right = tk.Frame(inner, bg=COLORS["header_bg"])
        right.pack(side="right")
        self.license_var = tk.StringVar(value="Checking license…" if self.licensing else "")
        tk.Label(right, textvariable=self.license_var, font=("Arial", 10, "bold"), bg=COLORS["header_bg"],
                 fg="#4CAF50").pack(anchor="e")
        self.extension_var = tk.StringVar(value="🔌 Extension: starting…")
        tk.Label(right, textvariable=self.extension_var, font=("Arial", 9), bg=COLORS["header_bg"],
                 fg="#CCCCCC").pack(anchor="e")

    def _build_controls(self):
        main = tk.Frame(self.root, bg=COLORS["dark_bg"])
        main.pack(fill="both", expand=True, padx=10, pady=8)
        self.main = main
        row = tk.Frame(main, bg=COLORS["dark_bg"])
        row.pack(fill="x", pady=4)
        tk.Label(row, text="URL:", font=("Arial", 10, "bold"), bg=COLORS["dark_bg"],
                 fg=COLORS["dark_text"]).pack(side="left", padx=(0, 10))
        self.url_entry = tk.Entry(row, font=("Arial", 11), bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                                  insertbackground=COLORS["dark_text"], relief="solid", bd=1)
        self.url_entry.pack(side="left", fill="x", expand=True, padx=(0, 10))
        tk.Label(row, text="Quality:", font=("Arial", 10, "bold"), bg=COLORS["dark_bg"],
                 fg=COLORS["dark_text"]).pack(side="left", padx=(0, 6))
        self.format_var = tk.StringVar(value="best")
        cb = ttk.Combobox(row, textvariable=self.format_var, values=FORMAT_CHOICES, state="readonly", width=8)
        cb.pack(side="left")
        self.format_hint = tk.Label(row, text=formats.LABELS["best"], font=("Arial", 8), bg=COLORS["dark_bg"],
                                    fg=COLORS["light_text"], width=22, anchor="w")
        self.format_hint.pack(side="left", padx=6)
        cb.bind("<<ComboboxSelected>>", lambda e: self.format_hint.config(text=formats.LABELS[self.format_var.get()]))
        btns = tk.Frame(main, bg=COLORS["dark_bg"])
        btns.pack(fill="x", pady=8)

        def button(text, cmd, color):
            b = tk.Button(btns, text=text, command=cmd, bg=color, fg="white", font=("Arial", 9, "bold"),
                          relief="flat", padx=14, pady=6, bd=0, activebackground=color, activeforeground="white")
            b.pack(side="left", padx=4)
            return b

        self.btn_add = button("Add to Queue", self.add_to_queue, COLORS["primary"])
        self.btn_instant = button("Instant Download", self.instant_download, COLORS["success"])
        self.btn_start = button("Start All", self.manager.start_all, COLORS["primary_light"])
        self.btn_pause = button("Pause All", self.manager.pause_all, COLORS["warning"])
        self.btn_resume = button("Resume All", self.manager.resume_all, COLORS["secondary"])
        self.btn_clear = button("Clear Completed", self.clear_completed, COLORS["danger"])
        button("Open Download Folder", self.open_download_folder, COLORS["primary_light"])
        self.gated_buttons = [self.btn_add, self.btn_instant, self.btn_start, self.btn_pause, self.btn_resume,
                              self.btn_clear]

    def _build_queue(self):
        hdr = tk.Frame(self.main, bg=COLORS["dark_bg"])
        hdr.pack(fill="x", pady=(6, 4))
        tk.Label(hdr, text="Download Queue", font=("Arial", 14, "bold"), bg=COLORS["dark_bg"],
                 fg=COLORS["dark_text"]).pack(side="left")
        self.count_var = tk.StringVar(value="Downloads: 0")
        tk.Label(hdr, textvariable=self.count_var, font=("Arial", 11), bg=COLORS["dark_bg"],
                 fg=COLORS["light_text"]).pack(side="left", padx=10)
        box = tk.Frame(self.main, bg=COLORS["dark_bg"])
        box.pack(fill="both", expand=True)
        self.canvas = tk.Canvas(box, bg=COLORS["dark_bg"], highlightthickness=0)
        sb = ttk.Scrollbar(box, orient="vertical", command=self.canvas.yview, style="IDM.Vertical.TScrollbar")
        self.queue = tk.Frame(self.canvas, bg=COLORS["dark_bg"])
        self._queue_window = self.canvas.create_window((0, 0), window=self.queue, anchor="nw")
        self.queue.bind("<Configure>", lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all")))
        self.canvas.bind("<Configure>", lambda e: self.canvas.itemconfig(self._queue_window, width=e.width))
        self.canvas.configure(yscrollcommand=sb.set)
        self.canvas.pack(side="left", fill="both", expand=True)
        sb.pack(side="right", fill="y")
        self.empty_label = tk.Label(self.queue, text="No downloads yet. Paste a link above and press Enter.",
                                    font=("Arial", 10), bg=COLORS["dark_bg"], fg=COLORS["light_text"])
        self.empty_label.pack(pady=30)
        self.canvas.bind_all("<MouseWheel>", self._on_wheel)

    def _on_wheel(self, e):
        if str(e.widget).startswith(str(self.canvas)):
            self.canvas.yview_scroll(int(-e.delta / 120), "units")

    def _build_log(self):
        tk.Label(self.main, text="Activity Log:", font=("Arial", 12, "bold"), bg=COLORS["dark_bg"],
                 fg=COLORS["dark_text"]).pack(anchor="w", pady=(8, 2))
        f = tk.Frame(self.main, bg=COLORS["dark_bg"])
        f.pack(fill="x")
        self.log_text = tk.Text(f, height=6, wrap="word", font=("Consolas", 9), relief="solid", bd=1, bg="#1e1e1e",
                                fg="#00ff00", insertbackground="#00ff00", state="disabled")
        self.log_text.pack(side="left", fill="both", expand=True)
        sb = ttk.Scrollbar(f, orient="vertical", command=self.log_text.yview, style="IDM.Vertical.TScrollbar")
        self.log_text.configure(yscrollcommand=sb.set)
        sb.pack(side="right", fill="y")

    def _build_status_bar(self):
        bar = tk.Frame(self.root, bg=COLORS["primary_dark"], height=24)
        bar.pack(fill="x", side="bottom")
        bar.pack_propagate(False)
        self.status_left = tk.Label(bar, text="Ready", font=("Arial", 9), bg=COLORS["primary_dark"], fg="white")
        self.status_left.pack(side="left", padx=10)
        self.status_right = tk.Label(bar, text="", font=("Arial", 9), bg=COLORS["primary_dark"], fg="white")
        self.status_right.pack(side="right", padx=10)

    def _bind_keys(self):
        self.root.bind("<Control-n>", lambda e: self.add_to_queue())
        self.root.bind("<Control-i>", lambda e: self.instant_download())
        self.root.bind("<F5>", lambda e: self.manager.start_all())
        self.root.bind("<F6>", lambda e: self.manager.pause_all())
        self.root.bind("<F7>", lambda e: self.manager.resume_all())
        self.url_entry.bind("<Return>", lambda e: self.instant_download())

    # ================================================================== bridge handlers
    def on_changed(self, snap: JobSnapshot):
        self.snaps[snap.id] = snap
        row = self.rows.get(snap.id)
        if row is None:
            if self.empty_label.winfo_ismapped():
                self.empty_label.pack_forget()
            self.rows[snap.id] = JobRow(self.queue, snap, self)
        else:
            row.update(snap)
        if snap.state not in ACTIVE:
            self.speeds.pop(snap.id, None)
        if snap.id in self.detail_windows:
            self.detail_windows[snap.id].update(snap)

    def on_progress(self, p: ProgressSnapshot):
        row = self.rows.get(p.job_id)
        if row:
            row.update_progress(p)
        self.speeds[p.job_id] = p.speed or 0
        if p.job_id in self.detail_windows:
            self.detail_windows[p.job_id].update_progress(p)

    def on_removed(self, job_id):
        row = self.rows.pop(job_id, None)
        if row:
            row.destroy()
        self.snaps.pop(job_id, None)
        self.speeds.pop(job_id, None)
        if job_id in self.detail_windows:
            self.detail_windows[job_id].close()
        if not self.rows:
            self.empty_label.pack(pady=30)

    def on_log(self, job_id, level, message):
        line = f"[{time.strftime('%H:%M:%S')}] {message}"
        self.logs[job_id].append(line)
        if job_id in self.detail_windows:
            self.detail_windows[job_id].append_log(line)
        self.log(("⚠ " if level == "warning" else "✖ " if level == "error" else "") + message)

    def log(self, message):
        self.log_text.config(state="normal")
        self.log_text.insert("end", f"[{time.strftime('%H:%M:%S')}] {message}\n")
        if int(self.log_text.index("end-1c").split(".")[0]) > 2000:
            self.log_text.delete("1.0", "500.0")
        self.log_text.see("end")
        self.log_text.config(state="disabled")

    def _tick_status(self):
        snaps = list(self.snaps.values())
        active = sum(1 for s in snaps if s.state in ACTIVE)
        total_speed = sum(v for k, v in self.speeds.items() if k in self.snaps and self.snaps[k].state in ACTIVE)
        self.count_var.set(f"Downloads: {len(snaps)} (Active: {active})")
        self.status_right.config(text=f"Total: {len(snaps)}  |  Active: {active}  |  Speed: {format_speed(total_speed)}")
        self.root.after(1000, self._tick_status)

    # ================================================================== row actions (called by JobRow)
    def pause(self, job_id): self.manager.pause(job_id)
    def resume(self, job_id): self.manager.resume(job_id)
    def retry(self, job_id): self.manager.retry(job_id)

    def cancel(self, job_id):
        if messagebox.askyesno("Cancel download", "Cancel this download and delete the partial file?",
                               parent=self.root):
            self.manager.cancel(job_id)

    def remove(self, job_id):
        snap = self.snaps.get(job_id)
        delete = False
        if snap and snap.state == S.COMPLETED and snap.final_path and os.path.exists(snap.final_path):
            answer = messagebox.askyesnocancel("Remove download", "Remove from the list.\n\nAlso delete the file "
                                               "from disk?", parent=self.root)
            if answer is None:
                return
            delete = bool(answer)
        self.manager.remove(job_id, delete_file=delete)

    def open_file(self, job_id):
        snap = self.snaps.get(job_id)
        if snap and snap.final_path and os.path.exists(snap.final_path):
            os.startfile(snap.final_path)
        else:
            messagebox.showinfo("Open", "The file is no longer there.", parent=self.root)

    def open_folder(self, job_id):
        snap = self.snaps.get(job_id)
        if snap and snap.final_path and os.path.exists(snap.final_path):
            import subprocess
            subprocess.Popen(["explorer", "/select,", os.path.normpath(snap.final_path)])
        elif snap:
            os.startfile(snap.dest_dir)

    def details(self, job_id):
        if job_id in self.detail_windows:
            self.detail_windows[job_id].win.lift()
            return
        snap = self.snaps.get(job_id)
        if not snap:
            return
        win = DetailsWindow(self.root, snap, self.logs[job_id], on_close=lambda jid: self.detail_windows.pop(jid, None))
        self.detail_windows[job_id] = win
        p = self.manager.progress(job_id)
        if p:
            win.update_progress(p)

    # ================================================================== commands
    def _destination(self):
        s = self.settings_store.get()
        if s.ask_save_location:
            chosen = filedialog.askdirectory(parent=self.root, title="Select save location",
                                             initialdir=s.download_dir if os.path.isdir(s.download_dir) else None)
            if chosen:
                return chosen
        return s.download_dir

    def _submit_from_entry(self, start: bool):
        if not self.download_allowed:
            messagebox.showwarning("License", "Downloads are disabled until a valid license or trial is active.",
                                   parent=self.root)
            return
        url = self.url_entry.get().strip()
        try:
            url = validate_url(url)
        except ValueError as e:
            messagebox.showerror("Invalid address", str(e), parent=self.root)
            return
        dest = os.path.normpath(self._destination())
        try:
            os.makedirs(dest, exist_ok=True)
        except OSError as e:
            messagebox.showerror("Download folder", f"Cannot create the folder:\n{dest}\n\n{e.strerror}",
                                 parent=self.root)
            return
        label = self.format_var.get()
        spec = JobSpec(url=url, dest_dir=dest, media=formats.options_for_label(label), format_label=label)
        self.manager.submit(spec, start=start)
        self.url_entry.delete(0, "end")
        self.log(f"{'Downloading' if start else 'Queued'}: {url}  →  {dest}")

    def add_to_queue(self):
        self._submit_from_entry(start=False)

    def instant_download(self):
        self._submit_from_entry(start=True)

    def clear_completed(self):
        for snap in list(self.snaps.values()):
            if snap.state == S.COMPLETED:
                self.manager.remove(snap.id)

    def open_download_folder(self):
        d = self.settings_store.get().download_dir
        try:
            os.makedirs(d, exist_ok=True)
            os.startfile(d)
        except OSError as e:
            messagebox.showerror("Folder", f"Cannot open {d}:\n{e.strerror}", parent=self.root)

    def open_settings(self):
        SettingsDialog(self.root, self.settings_store, lambda s: self.log("Settings saved."))

    def show_diagnostics(self):
        self.log("Collecting diagnostics…")

        def work():
            v = tools.versions()
            text = "\n".join(f"{k}: {val or 'NOT FOUND'}" for k, val in v.items())
            text += f"\n\nData folder: {appdata.app_dir()}\nLogs: {appdata.logs_dir()}\n"
            self.bridge.call_soon(show_text, self.root, "Diagnostics", text)

        threading.Thread(target=work, daemon=True).start()

    def update_ytdlp(self):
        if not messagebox.askyesno("Update yt-dlp", "Download the latest yt-dlp from its official GitHub release "
                                   "and verify its checksum?", parent=self.root):
            return
        self.log("Updating yt-dlp…")

        def work():
            try:
                ver = tools.update_ytdlp()
                self.bridge.call_soon(self.log, f"yt-dlp updated to {ver}.")
                self.bridge.call_soon(messagebox.showinfo, "Update yt-dlp", f"yt-dlp updated to {ver}.")
            except Exception as e:  # noqa: BLE001 - shown to the user
                self.bridge.call_soon(self.log, f"yt-dlp update failed: {e}")
                self.bridge.call_soon(messagebox.showerror, "Update yt-dlp", f"Update failed:\n{e}")

        threading.Thread(target=work, daemon=True).start()

    def check_updates(self, auto=True):
        def work():
            try:
                newer = updates.newer_version(__version__)
            except Exception as e:  # noqa: BLE001
                self.bridge.call_soon(self.log, f"Update check failed: {type(e).__name__}")
                if not auto:
                    self.bridge.call_soon(messagebox.showwarning, "Updates", "Unable to check for updates.")
                return
            if newer:
                self.bridge.call_soon(self.log, f"Update available: {newer}")
                if not auto:
                    self.bridge.call_soon(self._offer_update, newer)
            elif not auto:
                self.bridge.call_soon(messagebox.showinfo, "Updates", "You are running the latest version.")

        threading.Thread(target=work, daemon=True).start()

    def _offer_update(self, newer):
        if messagebox.askyesno("Update available", f"Version {newer} is available. Open the download page?"):
            webbrowser.open(updates.DOWNLOAD_PAGE_URL)

    # ================================================================== licensing (behaviour as legacy)
    def set_download_allowed(self, allowed: bool):
        self.download_allowed = allowed
        for b in self.gated_buttons:
            b.config(state="normal" if allowed else "disabled")

    def check_license(self):
        if not self.licensing:
            return

        def work():
            status = self.licensing.check()
            self.bridge.call_soon(self._apply_license, status)

        threading.Thread(target=work, daemon=True).start()

    def _apply_license(self, status):
        self.license_var.set(status.text)
        self.set_download_allowed(status.allowed)
        if status.action == "trial_started":
            messagebox.showinfo("Trial Started", f"Your trial started! {status.text}")
        elif status.action == "ask_key":
            if status.detail:
                messagebox.showinfo("License", status.detail)
            self.ask_license_key()
        elif status.action == "hardware_changed":
            if messagebox.askyesno("Hardware Change Detected", "Significant hardware changes detected.\n\n"
                                   "Do you want to reactivate your license?"):
                self.licensing.clear_token()
                self.ask_license_key()
        elif status.action == "machine_mismatch":
            messagebox.showerror("License Activation Error", "This license is already activated on a different "
                                 "computer.\n\nIf this is your only computer, please contact support.")
        elif status.action == "error" and status.detail:
            messagebox.showerror("License", status.detail)

    def ask_license_key(self):
        key = simpledialog.askstring("License Key", "Enter your 24-character license key:", parent=self.root)
        if not key:
            return

        def work():
            status = self.licensing.activate(key)
            self.bridge.call_soon(self._after_activate, status)

        threading.Thread(target=work, daemon=True).start()

    def _after_activate(self, status):
        self.license_var.set(status.text)
        if status.allowed:
            self.set_download_allowed(True)
            messagebox.showinfo("License Activated", status.detail or "License activated.")
        else:
            messagebox.showerror("License", status.detail or "Activation failed.")

    def show_license_file(self):
        p = appdata.bundle_dir() / "LICENSE.txt"
        try:
            text = p.read_text(encoding="utf-8")
        except OSError:
            text = "LICENSE.txt not found."
        show_text(self.root, "License Information", text)

    # ================================================================== extension hooks (main thread)
    def set_extension_status(self, text):
        self.extension_var.set(text)

    def show_pairing(self, extension_id, code, on_deny):
        return PairingDialog(self.root, extension_id, code, on_deny)

    def confirm_extension_download(self, request: dict, on_accept, on_reject):
        url = request["url"]
        title = request.get("title") or url
        from ..logging_setup import redact_url
        ok = messagebox.askyesno("Download from browser",
                                 f"The browser extension asks to download:\n\n{title[:200]}\n{redact_url(url)[:300]}\n\n"
                                 f"Quality: {request.get('format', 'best')}\n\nStart this download?",
                                 parent=self.root)
        (on_accept if ok else on_reject)()
