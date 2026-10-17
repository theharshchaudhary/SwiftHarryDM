"""Download the pinned, checksum-verified media tools used by the v2 media engine.

FFmpeg: BtbN LGPL build (no GPL-only components), pinned by release tag and
SHA-256. Only ffmpeg.exe, ffprobe.exe and the license file are extracted, by
exact member name, into packaging/tools/ffmpeg (git-ignored).

Usage: python packaging/fetch_tools.py [--dest packaging/tools]
"""
import argparse
import hashlib
import shutil
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

FFMPEG = {
    "release": "autobuild-2026-09-30-13-08",
    "asset": "ffmpeg-n8.1.3-9-g29e619e767-win64-lgpl-8.1.zip",
    "sha256": "4a7642b2264c03e8a0ce8a3825b933ee5580656f45695a086fe7e294045ffc0a",
    "members": {"bin/ffmpeg.exe": "ffmpeg.exe", "bin/ffprobe.exe": "ffprobe.exe", "LICENSE.txt": "LICENSE.txt"},
}


def _download(url: str, dest: Path) -> str:
    h = hashlib.sha256()
    with urllib.request.urlopen(url, timeout=60) as r, open(dest, "wb") as f:
        while True:
            b = r.read(1 << 20)
            if not b:
                break
            h.update(b)
            f.write(b)
    return h.hexdigest()


def fetch_ffmpeg(dest_root: Path) -> Path:
    out = dest_root / "ffmpeg"
    stamp = out / "VERSION"
    if stamp.exists() and stamp.read_text(encoding="utf-8").strip() == FFMPEG["asset"] and \
            all((out / v).exists() for v in FFMPEG["members"].values()):
        print(f"ffmpeg already present: {out}")
        return out
    url = f"https://github.com/BtbN/FFmpeg-Builds/releases/download/{FFMPEG['release']}/{FFMPEG['asset']}"
    with tempfile.TemporaryDirectory() as td:
        zpath = Path(td) / FFMPEG["asset"]
        print(f"downloading {url}")
        digest = _download(url, zpath)
        if digest != FFMPEG["sha256"]:
            raise SystemExit(f"SHA-256 mismatch for {FFMPEG['asset']}: got {digest}, expected {FFMPEG['sha256']}")
        print("sha256 verified")
        out.mkdir(parents=True, exist_ok=True)
        root = FFMPEG["asset"][:-4] + "/"
        with zipfile.ZipFile(zpath) as z:
            names = set(z.namelist())
            for member, target in FFMPEG["members"].items():
                name = root + member
                if name not in names:
                    raise SystemExit(f"{name} missing from archive")
                with z.open(name) as src, open(out / target, "wb") as dst:
                    shutil.copyfileobj(src, dst)
        stamp.write_text(FFMPEG["asset"] + "\n", encoding="utf-8")
    print(f"ffmpeg installed to {out}")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dest", default=str(Path(__file__).resolve().parent / "tools"))
    args = ap.parse_args()
    fetch_ffmpeg(Path(args.dest))
    return 0


if __name__ == "__main__":
    sys.exit(main())
