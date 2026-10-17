# Third-party components

SwiftHarryDM is distributed under the MIT License (`LICENSE.txt`). Packaged builds
include the components below. Versions are pinned in `requirements.lock` and
`packaging/fetch_tools.py`.

| Component | Version | License | How it is used |
|---|---|---|---|
| Python runtime (CPython) | 3.11 | PSF License | Embedded interpreter (PyInstaller) |
| Tcl/Tk | bundled with Python | Tcl/Tk license (BSD-style) | User interface |
| yt-dlp | 2026.8.19 | Unlicense (public domain) | Media extraction, run as a separate process |
| FFmpeg / ffprobe | n8.1.3-9-g29e619e767 (BtbN build `autobuild-2026-09-30-13-08`, LGPL variant) | **LGPL v3** (`--enable-version3`, no GPL or non-free parts) | Separate executables for merging, audio conversion and verification |
| Flask | 3.1.3 | BSD-3-Clause | Local API for the browser extension |
| Werkzeug | 3.1.9 | BSD-3-Clause | Flask dependency |
| Jinja2 | 3.1.6 | BSD-3-Clause | Flask dependency |
| MarkupSafe | 3.0.4 | BSD-3-Clause | Flask dependency |
| itsdangerous | 2.2.0 | BSD-3-Clause | Flask dependency |
| click | 8.5.0 | BSD-3-Clause | Flask dependency |
| blinker | 1.9.0 | MIT | Flask dependency |
| colorama | 0.4.6 | BSD-3-Clause | click dependency |
| requests | 2.34.2 | Apache-2.0 | HTTP client (native engine, licensing, updates) |
| urllib3 | 2.8.0 | MIT | requests dependency |
| certifi | 2026.7.22 | MPL-2.0 | CA certificates for TLS verification |
| idna | 3.20 | BSD-3-Clause | requests dependency |
| charset-normalizer | 3.5.2 | MIT | requests dependency |
| tqdm | 4.70.1 | MPL-2.0 AND MIT | Legacy engine only |
| PyInstaller bootloader | 6.22.3 | GPL-2.0-or-later **with the PyInstaller bootloader exception** | Build tool; the exception permits distributing the bundled app under any license |

## FFmpeg (LGPL) compliance

* FFmpeg is shipped as unmodified, separate executables (`tools/ffmpeg/ffmpeg.exe`,
  `ffprobe.exe`), not linked into SwiftHarryDM. You may replace them with your own build.
* Its license text is shipped as `tools/ffmpeg/LICENSE.txt`.
* Corresponding source: FFmpeg commit `29e619e767` (tag base n8.1.3) at
  https://github.com/FFmpeg/FFmpeg, built with the public scripts at
  https://github.com/BtbN/FFmpeg-Builds (release `autobuild-2026-09-30-13-08`).
  The exact archive is pinned by SHA-256 in `packaging/fetch_tools.py`.

## Not bundled

* A JavaScript runtime (Deno or Node.js) improves YouTube support in yt-dlp. SwiftHarryDM
  uses one if it is installed or placed in `%LOCALAPPDATA%\SwiftHarryDM\tools`; none is bundled.
