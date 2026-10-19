# SwiftHarryDM

A fast, lightweight download manager for Windows. It splits downloads across several
connections, resumes after interruptions and restarts, saves videos and audio from web
pages, and takes over downloads from your browser.

The installer is under 5 MB. Updates install with one click.

## Features

**Downloads**
- Up to 32 connections per file, with dynamic splitting: when a connection finishes, it
  takes over half of the largest remaining part.
- Resume after pauses, network drops, app restarts and crashes. Data is flushed to disk
  before progress is recorded, and the file on the server is re-validated before resuming.
- Checks that connections received the same file where their parts meet; mirrors that
  serve different files are detected and the download continues on one connection.
- Speed limiter (overall and per download, changeable while downloading), download quota
  ("no more than X MB every Y hours"), per-server connection limits.
- Refresh an expired download address without losing the downloaded part.
- Categories with their own folders, duplicate-link handling, antivirus scan after download.

**Video and audio**
- Save videos and audio from YouTube and many other sites: best quality, 4K to 360p,
  MP4, MKV, MP3 or M4A, or an exact format from the list.
- Protected (DRM) videos, playlists and live streams are not downloaded.
- The video components (yt-dlp and FFmpeg) are downloaded once when first needed, verified,
  and kept up to date automatically.

**Browser**
- Chrome, Edge and Brave extension that connects to the app by itself, with no code to type.
- Takes over browser downloads by file type, size and exception lists. Hold **Alt** while
  clicking a link to leave that download to the browser.
- "Download this video" button on players, "Download with SwiftHarryDM", and "Download
  all links" or "selected links" in the context menu.

**Organizing**
- Queues with schedules (start and stop times, days of the week), files at the same time,
  and an action when a queue finishes (exit, sleep, hibernate, shut down).
- Batch downloads (`img*.jpg` with numbers or letters), clipboard monitor, import and export
  of address lists, drag and drop of links.
- Find (Ctrl+F), sortable and configurable columns, dark mode, tray icon with live speed,
  progress on the taskbar button, floating drop target for links, start with Windows.
- Import address lists from text files, web pages and IDM export files (`.ef2`).

**Lightweight**
- Installer under 5 MB; about 25 MB of memory while downloading a 1 GB file.
- On a local test with each connection capped at 64 MB/s: 63 MB/s with one connection,
  436 MB/s with eight (`swiftdm-cli bench`).

## Install

1. Download `SwiftHarryDM_x.y.z_x64-setup.exe` from the
   [Releases](https://github.com/theharshchaudhary/SwiftHarryDM/releases) page and run it.
   No administrator rights are needed.
2. On first start, SwiftHarryDM offers to connect your browser. Follow the two steps; the
   extension then connects by itself.

Requirements: Windows 10 or 11 (64-bit). The Microsoft Edge WebView2 runtime is part of
Windows 11 and is installed automatically on Windows 10 if missing.

## Command line

```
swiftharrydm.exe <url>                  open the download window for <url>
swiftharrydm.exe /d <url> [/p <folder>] [/f <file name>] [/n] [/a]
    /n  start without asking            /a  add to the main queue instead of starting
```

If SwiftHarryDM is already running, the request goes to the open app.

## Updates

SwiftHarryDM checks for updates once a day. When one is available, a bar in the main window
offers **Update now**: the update is downloaded, its signature checked, installed, and the
app restarts with your downloads continuing where they were.

## Build from source

Requirements: Rust (stable, MSVC), Node.js 22+, and the Visual Studio C++ build tools.

```powershell
npm ci --prefix ui
node scripts/fetch-tools.mjs                        # optional: yt-dlp + FFmpeg for development
ui\node_modules\.bin\tauri build --debug --no-bundle  # app at target\debug\swiftharrydm.exe
ui\node_modules\.bin\tauri build                     # installer in target\release\bundle\nsis
```

Tests:

```powershell
cargo test -p swiftdm-core -p swiftharrydm           # engine, manager, API, media, licensing
cargo build -p swiftdm-testkit --bin sdm-observe
node tests/e2e/run.mjs                               # the built app and the extension in Chrome
cargo run -p swiftdm-cli --release -- kill-test      # hard-kill the downloader at random, check every file
cargo run -p swiftdm-cli --release -- bench          # throughput and memory, 1 vs 8 connections
```

More in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

MIT, see [LICENSE.txt](LICENSE.txt). Third-party components are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
