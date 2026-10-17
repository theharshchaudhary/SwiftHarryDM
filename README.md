# SwiftHarryDM

A download manager for Windows: segmented, resumable HTTP/HTTPS downloads, video and audio
from supported sites, and a paired browser extension.

## What it does

- **Native segmented downloads.** Up to 16 connections per file, with dynamic splitting of
  slow segments. Every partial response is validated (`Content-Range`, `If-Range` with ETag
  or Last-Modified). Data is fsync'ed before a checkpoint is recorded.
- **Resume that survives restarts.** Pause and resume, network drops, app restarts and
  crashes. If the file on the server changed, the download restarts instead of being
  stitched from two different files.
- **Honest completion.** A download is only marked *Completed* after the final file exists
  with the expected size. Media files must also pass an `ffprobe` check. Failures are shown
  with actionable messages.
- **No overwrites by default.** Name collisions become `name (1).ext`. Server-supplied file
  names are reduced to a safe basename.
- **Video and audio.** Uses yt-dlp, run as a separate process that can be stopped, plus
  FFmpeg: Best, 2160p down to 360p, MP4, MKV, MP3, M4A. The chosen formats are pinned, so a
  resumed download never switches quality. Playlists, live streams and DRM-protected media
  are not supported, and nothing bypasses access controls.
- **Browser extension.** Chrome and Edge (Manifest V3). It pairs with the app using a
  one-time code, and downloads it sends are confirmed in the app by default.

## Install and run (from source)

Requirements: Windows 10/11 x64 and Python 3.11.

```powershell
python -m venv .venv
.venv\Scripts\python -m pip install --require-hashes -r requirements.lock
.venv\Scripts\python packaging\fetch_tools.py      # pinned, checksum-verified FFmpeg (LGPL)
.venv\Scripts\python main.py
```

Optional: install [Node.js](https://nodejs.org) or Deno. yt-dlp uses a JavaScript runtime
for the best YouTube support.

### Engine selection

v2 is the default. The legacy engine is kept for rollback until the removal criteria in
`docs/MIGRATION.md` are met:

```powershell
python main.py --engine legacy        # or: set SWIFTDM_ENGINE=legacy, or Settings > Download engine
```

## Browser extension

1. Open `chrome://extensions` (or `edge://extensions`), enable Developer mode, then
   "Load unpacked" and select the `extension` folder. Packaged builds include it under
   `_internal\extension`.
2. Click the SwiftHarryDM toolbar icon, then **Connect to app**. The app shows a 6-digit
   code; type it into the popup.
3. Downloads sent from the browser show a confirmation in the app. You can turn that off
   in Settings.

## Build a release

See `docs/BUILD.md`. In short: `python packaging\build.py --zip` (add `--installer` when
Inno Setup 6 is installed).

## Tests

```powershell
.venv\Scripts\python -m pip install --require-hashes -r requirements-dev.lock
.venv\Scripts\python -m pytest -m "not live"      # PR gate
.venv\Scripts\python -m pytest                    # everything, incl. live CC-media tests
```

## Data locations

- Settings, job database and logs: `%LOCALAPPDATA%\SwiftHarryDM`. Logs redact URL query
  strings and tokens.
- Downloads: the folder you choose (default `Desktop\SwiftHarryDM Downloads`).

## License

MIT, see `LICENSE.txt`. Third-party components are listed in `THIRD_PARTY_NOTICES.md`.
