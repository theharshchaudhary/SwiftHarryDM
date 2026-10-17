"""Test/debug guard: assert that every Tcl call is made on the Tk main thread.

Install before creating widgets; widgets copy root.tk, so all of them go
through the checking proxy. Violations are recorded (and raised in strict mode).
"""
import threading


class ThreadViolation(AssertionError):
    pass


class _TclProxy:
    def __init__(self, real, owner_thread, violations, strict):
        object.__setattr__(self, "_real", real)
        object.__setattr__(self, "_owner", owner_thread)
        object.__setattr__(self, "_violations", violations)
        object.__setattr__(self, "_strict", strict)

    def __getattr__(self, name):
        attr = getattr(self._real, name)
        if not callable(attr):
            return attr

        def checked(*a, **k):
            if threading.current_thread() is not self._owner:
                msg = f"Tcl call {name}{a[:2]!r} from thread {threading.current_thread().name}"
                self._violations.append(msg)
                if self._strict:
                    raise ThreadViolation(msg)
            return attr(*a, **k)

        return checked


def install(root, strict=True) -> list:
    violations: list = []
    root.tk = _TclProxy(root.tk, threading.current_thread(), violations, strict)
    return violations
