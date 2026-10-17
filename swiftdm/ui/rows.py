"""One row per job in the queue, keyed by job id (never by list position)."""
from __future__ import annotations

import os
import tkinter as tk
from tkinter import ttk

from ..core.models import JobSnapshot, JobState as S, ProgressSnapshot
from .theme import COLORS, REASON_TEXT, STATE_COLORS, STATE_TEXT, format_eta, format_size, format_speed

ACTIVE = {S.STARTING, S.DOWNLOADING, S.POSTPROCESSING, S.FINALIZING}


class JobRow:
    def __init__(self, parent, snap: JobSnapshot, actions):
        self.actions = actions
        self.job_id = snap.id
        self.snap = snap
        self.progress = None
        self.frame = tk.Frame(parent, bd=1, relief="solid", padx=10, pady=6, bg=COLORS["dark_surface"])
        self.frame.pack(pady=3, fill="x", padx=5)
        top = tk.Frame(self.frame, bg=COLORS["dark_surface"])
        top.pack(fill="x")
        self.title = tk.Label(top, font=("Arial", 10, "bold"), anchor="w", bg=COLORS["dark_surface"])
        self.title.pack(side="left", fill="x", expand=True)
        self.status = tk.Label(top, font=("Arial", 9, "bold"), anchor="e", bg=COLORS["dark_surface"])
        self.status.pack(side="right")
        self.bar = ttk.Progressbar(self.frame, orient="horizontal", mode="determinate", maximum=1000,
                                   style="IDM.Horizontal.TProgressbar")
        self.bar.pack(fill="x", pady=4)
        bottom = tk.Frame(self.frame, bg=COLORS["dark_surface"])
        bottom.pack(fill="x")
        self.info = tk.Label(bottom, font=("Arial", 8), anchor="w", justify="left", bg=COLORS["dark_surface"])
        self.info.pack(side="left", fill="x", expand=True)
        self.buttons = tk.Frame(bottom, bg=COLORS["dark_surface"])
        self.buttons.pack(side="right")
        self._button_key = None
        self._indeterminate = False
        self._recolor_widgets = [self.frame, top, bottom, self.title, self.status, self.info, self.buttons]
        for w in (self.frame, self.title, self.info, self.status):
            w.bind("<Double-Button-1>", lambda e: self.actions.details(self.job_id))
        self.update(snap)

    # ------------------------------------------------------------------ rendering
    def update(self, snap: JobSnapshot):
        self.snap = snap
        title = snap.title or snap.url
        self.title.config(text=title if len(title) <= 90 else title[:87] + "…")
        state_text = STATE_TEXT.get(snap.state.value, snap.state.value)
        if snap.state == S.PAUSED and snap.reason:
            state_text = REASON_TEXT.get(snap.reason, state_text)
        elif snap.state == S.RETRY_WAIT and snap.reason:
            state_text = f"Waiting ({snap.reason})"
        self.status.config(text=state_text)
        bg, fg = STATE_COLORS.get(snap.state.value, (COLORS["dark_surface"], COLORS["dark_text"]))
        for w in self._recolor_widgets:
            w.config(bg=bg)
        for w in (self.title, self.status, self.info):
            w.config(fg=fg)
        if snap.state == S.COMPLETED:
            self._set_determinate(1000)
        elif snap.state not in ACTIVE and not self.progress:
            frac = (snap.downloaded / snap.total) if snap.total else 0
            self._set_determinate(int(frac * 1000))
        self._render_info()
        self._render_buttons()

    def update_progress(self, p: ProgressSnapshot):
        self.progress = p
        if self.snap.state in ACTIVE or self.snap.state in (S.PAUSING, S.CANCELLING):
            if p.total:
                self._set_determinate(int(min(p.downloaded / p.total, 1.0) * 1000))
            else:
                self._set_indeterminate()
        self._render_info()

    def _set_determinate(self, value):
        if self._indeterminate:
            self.bar.stop()
            self.bar.config(mode="determinate")
            self._indeterminate = False
        self.bar["value"] = value

    def _set_indeterminate(self):
        if not self._indeterminate:
            self.bar.config(mode="indeterminate")
            self.bar.start(15)
            self._indeterminate = True

    def _render_info(self):
        s, p = self.snap, self.progress
        parts = [f"Format: {s.format_label}"]
        if s.source == "extension":
            parts.append("🌐 Browser")
        if s.state == S.FAILED and s.error:
            parts.append(s.error.message)
        elif s.state == S.COMPLETED:
            size = s.total if s.total is not None else s.downloaded
            parts.append(format_size(size))
            if s.final_path:
                parts.append(os.path.basename(s.final_path))
        else:
            done = p.downloaded if p else s.downloaded
            total = p.total if p else s.total
            parts.append(f"{format_size(done)} / {format_size(total) if total else '?'}")
            if s.state in ACTIVE and p:
                parts.append(format_speed(p.speed))
                parts.append(f"ETA {format_eta(p.eta)}")
            if s.state == S.PAUSED and s.error:
                parts.append(s.error.message)
        self.info.config(text="  ·  ".join(parts))

    def _render_buttons(self):
        st = self.snap.state
        if st in ACTIVE or st in (S.QUEUED, S.RETRY_WAIT):
            spec = [("Pause", "pause"), ("Cancel", "cancel")]
        elif st == S.PAUSED:
            spec = [("Resume" if self.snap.reason != "not started" else "Start", "resume"), ("Cancel", "cancel")]
        elif st == S.FAILED:
            spec = [("Retry", "retry"), ("Remove", "remove")]
        elif st == S.COMPLETED:
            spec = [("Open", "open_file"), ("Folder", "open_folder"), ("Remove", "remove")]
        elif st == S.CANCELLED:
            spec = [("Remove", "remove")]
        else:
            spec = []
        spec = spec + [("Details", "details")]
        key = tuple(spec)
        if key == self._button_key:
            return
        self._button_key = key
        for w in self.buttons.winfo_children():
            w.destroy()
        for text, action in spec:
            tk.Button(self.buttons, text=text, font=("Arial", 8), relief="flat", padx=6, pady=1,
                      bg=COLORS["primary"], fg="white", activebackground=COLORS["primary_light"],
                      command=lambda a=action: getattr(self.actions, a)(self.job_id)).pack(side="left", padx=2)

    def destroy(self):
        if self._indeterminate:
            self.bar.stop()
        self.frame.destroy()
