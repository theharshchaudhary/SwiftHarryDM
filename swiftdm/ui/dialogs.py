"""Secondary windows: download details, settings, diagnostics, about, pairing, confirmation."""
from __future__ import annotations

import collections
import os
import tkinter as tk
from tkinter import filedialog, messagebox, ttk

from .. import __version__, appdata
from ..core.models import JobSnapshot, ProgressSnapshot
from ..logging_setup import redact_url
from .theme import COLORS, STATE_TEXT, format_eta, format_size, format_speed


def _center(win, w, h):
    win.update_idletasks()
    x = (win.winfo_screenwidth() - w) // 2
    y = (win.winfo_screenheight() - h) // 2
    win.geometry(f"{w}x{h}+{x}+{y}")


def _label(parent, text="", bold=False, fg=None, **kw):
    return tk.Label(parent, text=text, font=("Arial", 10, "bold" if bold else "normal"), anchor="w", justify="left",
                    bg=COLORS["dark_bg"], fg=fg or COLORS["dark_text"], **kw)


class DetailsWindow:
    """Non-modal per-download window with the real per-segment byte counts."""

    def __init__(self, root, snap: JobSnapshot, logs: collections.deque, on_close):
        self.job_id = snap.id
        self.on_close = on_close
        self.win = tk.Toplevel(root)
        self.win.title(f"SwiftHarryDM - {snap.title}")
        self.win.configure(bg=COLORS["dark_bg"])
        _center(self.win, 680, 520)
        self.win.protocol("WM_DELETE_WINDOW", self.close)
        body = tk.Frame(self.win, bg=COLORS["dark_bg"], padx=16, pady=12)
        body.pack(fill="both", expand=True)
        self.file = _label(body, bold=True, fg="#00ff00")
        self.file.pack(fill="x")
        _label(body, f"URL: {redact_url(snap.url)}", fg=COLORS["light_text"], wraplength=640).pack(fill="x")
        self.dest = _label(body, fg=COLORS["light_text"], wraplength=640)
        self.dest.pack(fill="x")
        self.bar = ttk.Progressbar(body, orient="horizontal", mode="determinate", maximum=1000,
                                   style="IDM.Horizontal.TProgressbar")
        self.bar.pack(fill="x", pady=8)
        grid = tk.Frame(body, bg=COLORS["dark_bg"])
        grid.pack(fill="x")
        self.fields = {}
        for i, name in enumerate(("Status", "Size", "Downloaded", "Speed", "Time left", "Engine")):
            _label(grid, f"{name}:", bold=True).grid(row=i // 2, column=(i % 2) * 2, sticky="w", padx=(0, 6))
            v = _label(grid)
            v.grid(row=i // 2, column=(i % 2) * 2 + 1, sticky="w", padx=(0, 24))
            self.fields[name] = v
        _label(body, "Connections (segments):", bold=True).pack(fill="x", pady=(10, 2))
        self.tree = ttk.Treeview(body, columns=("seg", "range", "done", "pct", "state"), show="headings", height=7,
                                 style="IDM.Treeview")
        for col, text, w in (("seg", "#", 40), ("range", "Byte range", 220), ("done", "Downloaded", 110),
                             ("pct", "%", 60), ("state", "State", 100)):
            self.tree.heading(col, text=text)
            self.tree.column(col, width=w, anchor="w")
        self.tree.pack(fill="x")
        _label(body, "Log:", bold=True).pack(fill="x", pady=(10, 2))
        self.log = tk.Text(body, height=6, bg="#1a1a1a", fg="#00ff00", font=("Consolas", 8), wrap="word")
        self.log.pack(fill="both", expand=True)
        for line in logs:
            self.log.insert("end", line + "\n")
        self.log.config(state="disabled")
        tk.Button(body, text="Close", command=self.close, bg=COLORS["primary"], fg="white", relief="flat",
                  padx=16).pack(anchor="e", pady=(8, 0))
        self.update(snap)

    def update(self, snap: JobSnapshot):
        self.file.config(text=f"File: {os.path.basename(snap.final_path) if snap.final_path else snap.title}")
        self.dest.config(text=f"Folder: {snap.dest_dir}")
        self.fields["Status"].config(text=STATE_TEXT.get(snap.state.value, snap.state.value)
                                     + (f" - {snap.error.message}" if snap.error else ""))
        self.fields["Engine"].config(text={"http": "Native HTTP", "media": "Media (yt-dlp)"}.get(snap.kind, "—"))
        if snap.state.value == "COMPLETED":
            self.bar["value"] = 1000
            self.fields["Size"].config(text=format_size(snap.total))
            self.fields["Downloaded"].config(text=format_size(snap.total))
            self.fields["Speed"].config(text="—")
            self.fields["Time left"].config(text="—")

    def update_progress(self, p: ProgressSnapshot):
        self.bar["value"] = int(min(p.downloaded / p.total, 1) * 1000) if p.total else 0
        self.fields["Size"].config(text=format_size(p.total) if p.total else "Unknown")
        pct = f" ({p.downloaded / p.total * 100:.1f}%)" if p.total else ""
        self.fields["Downloaded"].config(text=format_size(p.downloaded) + pct)
        self.fields["Speed"].config(text=format_speed(p.speed))
        self.fields["Time left"].config(text=format_eta(p.eta))
        rows = []
        for s in p.segments:
            length = (s.end - s.start + 1) if s.end >= 0 else None
            pct = f"{s.downloaded / length * 100:.0f}" if length else "?"
            state = "Done" if length and s.downloaded >= length else ("Active" if s.active else "Waiting")
            rng = f"{s.start:,} - {s.end:,}" if s.end >= 0 else f"{s.start:,} - end"
            rows.append((s.idx + 1, rng, format_size(s.downloaded), pct, state))
        existing = self.tree.get_children()
        if len(existing) != len(rows):
            self.tree.delete(*existing)
            existing = [self.tree.insert("", "end", values=r) for r in rows]
        else:
            for iid, r in zip(existing, rows):
                self.tree.item(iid, values=r)

    def append_log(self, line: str):
        self.log.config(state="normal")
        self.log.insert("end", line + "\n")
        self.log.see("end")
        self.log.config(state="disabled")

    def close(self):
        self.on_close(self.job_id)
        self.win.destroy()


class SettingsDialog:
    def __init__(self, root, store, on_saved):
        s = store.get()
        self.store, self.on_saved = store, on_saved
        self.win = tk.Toplevel(root)
        self.win.title("Settings")
        self.win.configure(bg=COLORS["dark_bg"])
        self.win.transient(root)
        _center(self.win, 560, 430)
        f = tk.Frame(self.win, bg=COLORS["dark_bg"], padx=16, pady=12)
        f.pack(fill="both", expand=True)
        self.dir_var = tk.StringVar(value=s.download_dir)
        _label(f, "Default download folder:", bold=True).grid(row=0, column=0, sticky="w")
        tk.Entry(f, textvariable=self.dir_var, width=48).grid(row=1, column=0, sticky="we")
        tk.Button(f, text="Browse…", command=self._browse).grid(row=1, column=1, padx=6)
        self.ask_var = tk.BooleanVar(value=s.ask_save_location)
        self.auto_var = tk.BooleanVar(value=s.auto_resume)
        self.trust_var = tk.BooleanVar(value=s.trust_extension)
        self.conc_var = tk.IntVar(value=s.max_concurrent)
        self.seg_var = tk.IntVar(value=s.max_segments)
        self.policy_var = tk.StringVar(value=s.collision_policy)
        self.engine_var = tk.StringVar(value=s.engine)
        row = 2
        for text, var in (("Ask where to save each download", self.ask_var),
                          ("Resume interrupted downloads automatically at startup", self.auto_var),
                          ("Start browser-extension downloads without asking", self.trust_var)):
            tk.Checkbutton(f, text=text, variable=var, bg=COLORS["dark_bg"], fg=COLORS["dark_text"],
                           selectcolor=COLORS["dark_surface"], activebackground=COLORS["dark_bg"],
                           anchor="w").grid(row=row, column=0, columnspan=2, sticky="w", pady=2)
            row += 1
        for text, var, lo, hi in (("Simultaneous downloads:", self.conc_var, 1, 10),
                                  ("Connections per download:", self.seg_var, 1, 16)):
            _label(f, text).grid(row=row, column=0, sticky="w", pady=4)
            tk.Spinbox(f, from_=lo, to=hi, textvariable=var, width=5).grid(row=row, column=1, sticky="w")
            row += 1
        _label(f, "If a file with the same name exists:").grid(row=row, column=0, sticky="w", pady=4)
        ttk.Combobox(f, textvariable=self.policy_var, values=["rename", "overwrite"], state="readonly",
                     width=12).grid(row=row, column=1, sticky="w")
        row += 1
        _label(f, "Download engine (takes effect after restart):").grid(row=row, column=0, sticky="w", pady=4)
        ttk.Combobox(f, textvariable=self.engine_var, values=["v2", "legacy"], state="readonly",
                     width=12).grid(row=row, column=1, sticky="w")
        row += 1
        btns = tk.Frame(f, bg=COLORS["dark_bg"])
        btns.grid(row=row, column=0, columnspan=2, sticky="e", pady=(16, 0))
        tk.Button(btns, text="Save", command=self._save, bg=COLORS["primary"], fg="white", padx=14).pack(side="left",
                                                                                                       padx=4)
        tk.Button(btns, text="Cancel", command=self.win.destroy, padx=10).pack(side="left")

    def _browse(self):
        d = filedialog.askdirectory(parent=self.win, initialdir=self.dir_var.get() or None)
        if d:
            self.dir_var.set(d)

    def _save(self):
        if self.policy_var.get() == "overwrite" and not messagebox.askyesno(
                "Overwrite files?", "Existing files with the same name will be replaced. Continue?", parent=self.win):
            return
        try:
            s = self.store.update(download_dir=self.dir_var.get().strip(), ask_save_location=self.ask_var.get(),
                                  auto_resume=self.auto_var.get(), trust_extension=self.trust_var.get(),
                                  max_concurrent=int(self.conc_var.get()), max_segments=int(self.seg_var.get()),
                                  collision_policy=self.policy_var.get(), engine=self.engine_var.get())
        except (ValueError, tk.TclError):
            messagebox.showerror("Settings", "Please enter whole numbers.", parent=self.win)
            return
        self.on_saved(s)
        self.win.destroy()


def show_text(root, title, text, w=640, h=420, buttons=()):
    win = tk.Toplevel(root)
    win.title(title)
    win.configure(bg=COLORS["dark_bg"])
    win.transient(root)
    _center(win, w, h)
    t = tk.Text(win, wrap="word", bg=COLORS["dark_surface"], fg=COLORS["dark_text"], font=("Consolas", 9))
    t.pack(fill="both", expand=True, padx=10, pady=10)
    t.insert("end", text)
    t.config(state="disabled")
    bar = tk.Frame(win, bg=COLORS["dark_bg"])
    bar.pack(fill="x", padx=10, pady=(0, 10))
    for label, cmd in buttons:
        tk.Button(bar, text=label, command=cmd, padx=10).pack(side="left", padx=4)
    tk.Button(bar, text="Close", command=win.destroy, padx=10).pack(side="right")
    return win


def about_text() -> str:
    return (f"SwiftHarryDM {__version__}\n\nDownload manager for Windows.\n"
            "• Native segmented HTTP/HTTPS downloads with verified resume\n"
            "• Video and audio from supported sites via yt-dlp + FFmpeg\n"
            "• Browser extension integration (paired, local only)\n\n"
            "Developed by Harsh Chaudhary. MIT License (see LICENSE.txt).\n"
            "Third-party components and their licenses: THIRD_PARTY_NOTICES.md.\n\n"
            f"Data folder: {appdata.app_dir()}\n")


class PairingDialog:
    """Shows the one-time code the user must type into the browser extension."""

    def __init__(self, root, extension_id: str, code: str, on_deny):
        self.win = tk.Toplevel(root)
        self.win.title("Connect browser extension")
        self.win.configure(bg=COLORS["dark_bg"])
        self.win.attributes("-topmost", True)
        _center(self.win, 460, 260)
        f = tk.Frame(self.win, bg=COLORS["dark_bg"], padx=18, pady=14)
        f.pack(fill="both", expand=True)
        _label(f, "A browser extension wants to connect to SwiftHarryDM.", bold=True).pack(fill="x")
        _label(f, f"Extension ID: {extension_id}", fg=COLORS["light_text"]).pack(fill="x", pady=(4, 10))
        _label(f, "If you started this, type this code into the extension:").pack(fill="x")
        tk.Label(f, text=" ".join(code), font=("Consolas", 26, "bold"), bg=COLORS["dark_bg"],
                 fg="#00ff00").pack(pady=8)
        _label(f, "The code expires in 2 minutes. If you did not start this, click Deny.",
               fg=COLORS["light_text"]).pack(fill="x")
        tk.Button(f, text="Deny", command=lambda: (on_deny(), self.close()), padx=12).pack(anchor="e", pady=(8, 0))

    def close(self):
        try:
            self.win.destroy()
        except tk.TclError:
            pass
