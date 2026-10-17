"""T0: redirected stdout under the ANSI code page must not crash on emoji output."""
import ast
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def _run(code):
    env = {k: v for k, v in os.environ.items() if not k.startswith("PYTHONIO") and k != "PYTHONUTF8"}
    env["PYTHONPATH"] = str(ROOT)
    return subprocess.run([sys.executable, "-X", "utf8=0", "-c", code], capture_output=True, env=env)


def test_unhardened_pipe_reproduces_crash():
    r = _run("import sys; print('\U0001F527 x')")
    assert r.returncode != 0 and b"UnicodeEncodeError" in r.stderr


def test_harden_stdio_prevents_crash():
    r = _run("from swiftdm.compat import harden_stdio; harden_stdio(); print('\U0001F527 x'); "
             "import sys; print('\u274c', file=sys.stderr)")
    assert r.returncode == 0, r.stderr
    assert "\U0001F527".encode("utf-8") in r.stdout


def test_main_hardens_before_any_other_import():
    tree = ast.parse((ROOT / "main.py").read_text(encoding="utf-8"))
    first_call = None
    for i, node in enumerate(tree.body):
        if isinstance(node, ast.Expr) and isinstance(node.value, ast.Call) and getattr(node.value.func, "id", "") == "harden_stdio":
            first_call = i
            break
    assert first_call is not None
    earlier_imports = [n for n in tree.body[:first_call] if isinstance(n, (ast.Import, ast.ImportFrom))]
    names = {a.name for n in earlier_imports for a in n.names}
    assert names <= {"sys", "harden_stdio"}
