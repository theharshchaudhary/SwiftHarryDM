"""Process-level compatibility shims that must run before anything prints."""
import sys


def harden_stdio():
    """Never let console encoding crash the app.

    When stdout/stderr is redirected (pipe, file, launcher) Windows uses the ANSI
    code page (cp1252), and printing emoji raised UnicodeEncodeError inside the
    download path (audit finding T0). Re-encode as UTF-8 and replace anything
    unencodable. In windowed (no-console) builds the streams are None; leave them.
    """
    for name in ("stdout", "stderr"):
        stream = getattr(sys, name, None)
        if stream is None or not hasattr(stream, "reconfigure"):
            continue
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (ValueError, OSError):
            try:
                stream.reconfigure(errors="replace")
            except (ValueError, OSError):
                pass
