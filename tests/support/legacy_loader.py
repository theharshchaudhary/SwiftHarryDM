"""Load pieces of the legacy main.py without executing its module-level GUI code."""
import ast
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load(names, extra_globals=None, assigns=()):
    src = (ROOT / "main.py").read_text(encoding="utf-8")
    tree = ast.parse(src)
    nodes = []
    for n in tree.body:
        if isinstance(n, (ast.FunctionDef, ast.ClassDef)) and n.name in names:
            nodes.append(n)
        elif isinstance(n, ast.Assign) and any(getattr(t, "id", "") in assigns for t in n.targets):
            nodes.append(n)
    ns = {"__file__": str(ROOT / "main.py"), "__name__": "legacy_main"}
    exec("import os, sys, re, time, json, subprocess, platform, requests\n"
         "from datetime import datetime\n"
         "from yt_dlp import YoutubeDL\n"
         "from flask import Flask, request, jsonify\n"
         "from utils import convert_to_mp3, format_mapping, manual_merge_video_audio\n"
         "from swiftdm.logging_setup import redact_url\n", ns)
    ns.update(extra_globals or {})
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "main.py(extracted)", "exec"), ns)
    return ns
