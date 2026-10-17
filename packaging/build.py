"""Reproducible Windows build.

    python packaging/build.py [--zip] [--installer]

1. fetches the pinned, SHA-256-verified FFmpeg (packaging/fetch_tools.py)
2. runs PyInstaller with packaging/SwiftHarryDM.spec -> packaging/dist/SwiftHarryDM/
3. smoke-checks the bundled yt-dlp shim and FFmpeg
4. optional: signs SwiftHarryDM.exe if SIGNTOOL_CERT_THUMBPRINT is set (certificate in
   the Windows certificate store - never a file in this repository)
5. optional: --zip writes packaging/dist/SwiftHarryDM-<version>-win64.zip + .sha256
6. optional: --installer runs Inno Setup (ISCC.exe) on packaging/SwiftHarryDM.iss
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(ROOT))
from swiftdm import __version__  # noqa: E402

DIST = HERE / "dist"
APP = DIST / "SwiftHarryDM"


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run(cmd, check=True, **kw)


def sign(path: Path):
    thumb = os.environ.get("SIGNTOOL_CERT_THUMBPRINT")
    if not thumb:
        print("signing skipped (SIGNTOOL_CERT_THUMBPRINT not set)")
        return
    run(["signtool", "sign", "/sha1", thumb, "/fd", "SHA256", "/tr", "http://timestamp.digicert.com", "/td",
         "SHA256", str(path)])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--zip", action="store_true")
    ap.add_argument("--installer", action="store_true")
    args = ap.parse_args()
    run([sys.executable, str(HERE / "fetch_tools.py")])
    run([sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--distpath", str(DIST), "--workpath",
         str(HERE / "build"), str(HERE / "SwiftHarryDM.spec")])
    exe = APP / "SwiftHarryDM.exe"
    out = run([str(exe), "--yt-dlp-shim", "--version"], capture_output=True, text=True)
    print("bundled yt-dlp:", out.stdout.strip())
    ff = next(APP.rglob("ffmpeg.exe"))
    print("bundled ffmpeg:", run([str(ff), "-hide_banner", "-version"], capture_output=True,
                                 text=True).stdout.splitlines()[0])
    sign(exe)
    if args.zip:
        zpath = DIST / f"SwiftHarryDM-{__version__}-win64.zip"
        with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
            for f in APP.rglob("*"):
                if f.is_file():
                    z.write(f, Path("SwiftHarryDM") / f.relative_to(APP))
        digest = hashlib.sha256(zpath.read_bytes()).hexdigest()
        (zpath.with_suffix(".zip.sha256")).write_text(f"{digest}  {zpath.name}\n", encoding="utf-8")
        print(f"wrote {zpath} ({zpath.stat().st_size // (1 << 20)} MB) sha256={digest}")
    if args.installer:
        iscc = shutil.which("ISCC") or r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
        if not os.path.exists(iscc):
            raise SystemExit("Inno Setup 6 (ISCC.exe) not found; install it or build without --installer")
        run([iscc, f"/DAppVersion={__version__}", str(HERE / "SwiftHarryDM.iss")])
        for inst in DIST.glob("SwiftHarryDM-*-setup.exe"):
            sign(inst)
    return 0


if __name__ == "__main__":
    sys.exit(main())
