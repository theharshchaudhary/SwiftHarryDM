"""Visual theme carried over from the legacy UI (same colours and ttk styles)."""
import math
from tkinter import ttk

COLORS = {
    "primary": "#2C5F8A", "primary_light": "#3A7BB3", "primary_dark": "#1E4A6B", "secondary": "#E8A735",
    "success": "#4CAF50", "warning": "#FF9800", "danger": "#F44336", "dark_bg": "#2D2D2D",
    "dark_surface": "#3C3C3C", "dark_text": "#FFFFFF", "light_text": "#CCCCCC", "border": "#555555",
    "header_bg": "#1E3A5F", "row_even": "#2D2D2D", "row_odd": "#363636", "progress_bg": "#1E4A6B",
    "progress_fg": "#4CAF50",
}

STATE_COLORS = {
    # state: (row background, text colour)
    "QUEUED": (COLORS["dark_surface"], COLORS["dark_text"]),
    "STARTING": ("#1e3a5f", "#4fc3f7"), "DOWNLOADING": ("#1e3a5f", "#4fc3f7"),
    "POSTPROCESSING": ("#1e3a5f", "#4fc3f7"), "FINALIZING": ("#1e3a5f", "#4fc3f7"),
    "PAUSING": ("#5d4037", "#ffb74d"), "PAUSED": ("#5d4037", "#ffb74d"), "RETRY_WAIT": ("#5d4037", "#ffb74d"),
    "COMPLETED": ("#2e7d32", "#c8e6c9"), "FAILED": ("#c62828", "#ffebee"),
    "CANCELLING": ("#424242", "#bdbdbd"), "CANCELLED": ("#424242", "#bdbdbd"),
}

STATE_TEXT = {
    "QUEUED": "Queued", "STARTING": "Connecting…", "DOWNLOADING": "Downloading", "POSTPROCESSING": "Processing…",
    "FINALIZING": "Finishing…", "COMPLETED": "Completed", "PAUSING": "Pausing…", "PAUSED": "Paused",
    "RETRY_WAIT": "Waiting to retry", "CANCELLING": "Cancelling…", "CANCELLED": "Cancelled", "FAILED": "Failed",
}

REASON_TEXT = {"not started": "Not started", "interrupted": "Interrupted (app closed)", "disk_full": "Disk full",
               "paused": "Paused"}


def configure_styles(root=None):
    style = ttk.Style(root)
    style.configure("IDM.TFrame", background=COLORS["dark_bg"])
    style.configure("IDM.TLabel", background=COLORS["dark_bg"], foreground=COLORS["dark_text"])
    style.configure("IDM.Horizontal.TProgressbar", background=COLORS["progress_fg"],
                    troughcolor=COLORS["progress_bg"], borderwidth=0, lightcolor=COLORS["progress_fg"],
                    darkcolor=COLORS["progress_fg"])
    style.configure("IDM.Treeview", background=COLORS["dark_surface"], foreground=COLORS["dark_text"],
                    fieldbackground=COLORS["dark_surface"], borderwidth=0, rowheight=24)
    style.configure("IDM.Treeview.Heading", background=COLORS["primary_dark"], foreground=COLORS["dark_text"],
                    relief="flat", borderwidth=0, font=("Arial", 10, "bold"))
    style.configure("IDM.Vertical.TScrollbar", background=COLORS["primary_dark"], troughcolor=COLORS["dark_bg"],
                    borderwidth=0, arrowsize=12)
    return style


def format_size(n) -> str:
    if n is None:
        return "?"
    n = float(n)
    if n < 1024:
        return f"{int(n)} B"
    units = ["KB", "MB", "GB", "TB"]
    i = min(int(math.log(n, 1024)), len(units))
    return f"{n / 1024 ** i:.1f} {units[i - 1]}"


def format_speed(bps) -> str:
    return "—" if not bps else f"{format_size(bps)}/s"


def format_eta(seconds) -> str:
    if seconds is None or seconds < 0:
        return "—"
    s = int(round(seconds))
    if s < 60:
        return f"{s} s"
    if s < 3600:
        return f"{s // 60} min {s % 60:02d} s"
    return f"{s // 3600} h {(s % 3600) // 60:02d} min"
