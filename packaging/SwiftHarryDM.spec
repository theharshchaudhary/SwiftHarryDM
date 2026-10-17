# PyInstaller spec for SwiftHarryDM (one-folder, windowed).
# Build:  python packaging/build.py      (fetches pinned FFmpeg first, then runs PyInstaller)
# No signing material is referenced here; signing happens in packaging/build.py
# only when SIGNTOOL_CERT_THUMBPRINT is set in the environment.
import os
from PyInstaller.utils.hooks import collect_submodules

ROOT = os.path.abspath(os.path.join(SPECPATH, ".."))
TOOLS = os.path.join(SPECPATH, "tools", "ffmpeg")

hidden = collect_submodules("yt_dlp") + collect_submodules("swiftdm")

datas = [
    (os.path.join(ROOT, "app.ico"), "."),
    (os.path.join(ROOT, "LICENSE.txt"), "."),
    (os.path.join(ROOT, "THIRD_PARTY_NOTICES.md"), "."),
    (os.path.join(ROOT, "extension"), "extension"),
    (os.path.join(TOOLS, "ffmpeg.exe"), os.path.join("tools", "ffmpeg")),
    (os.path.join(TOOLS, "ffprobe.exe"), os.path.join("tools", "ffmpeg")),
    (os.path.join(TOOLS, "LICENSE.txt"), os.path.join("tools", "ffmpeg")),
    (os.path.join(TOOLS, "VERSION"), os.path.join("tools", "ffmpeg")),
]

a = Analysis(
    [os.path.join(ROOT, "main.py")],
    pathex=[ROOT],
    datas=datas,
    hiddenimports=hidden,
    excludes=["youtube_dl", "pytest", "hypothesis", "psutil", "PyInstaller"],
    noarchive=False,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="SwiftHarryDM",
    console=False,
    icon=os.path.join(ROOT, "app.ico"),
    version=os.path.join(SPECPATH, "version_info.txt"),
)
coll = COLLECT(exe, a.binaries, a.datas, name="SwiftHarryDM")
