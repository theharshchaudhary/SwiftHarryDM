# Tauri 2 + Rust + React rewrite: status (2026-10-18)

Branch: `tauri-rewrite`. `main` still holds the shipped Python v2 app (unchanged and working).

The goal is a download manager with the feature set and workflow people know from the
leading Windows download managers, built with SwiftHarryDM's own code, wording, icons and
design. No third-party program code, text or artwork is used.

## Done and verified

| Area | Evidence |
|---|---|
| Core: models, state machine, SQLite store, settings, categories, DPAPI secrets, proxy, limiter, scheduler | 53 unit tests |
| Manager: queues, Download later, schedules, after-queue actions, recovery, retries, refresh address, AV scan, download quota, duplicate lookup, live per-download speed limit, priorities | 13 manager tests + randomized invariant test |
| HTTP engine: segmented, validated resume, dynamic splitting, per-server connection limits, boundary checks between connections (falls back to one connection if mirrors differ) | 27-case fault matrix (`tests/http_matrix.rs`) |
| Fixed: downloads whose middle segment was split always failed at the end | `slow_middle_segment_is_split_and_file_is_exact` |
| Find, batch series, URL extraction, capture rules, wildcard lists | unit tests in `search`, `batch`, `capture`, `wildcard` |
| Extension API (axum): loopback only, Host/Origin checks, pairing, hashed tokens, confirmation, capture check, link lists | 5 API tests (`tests/api.rs`) |
| Media engine (yt-dlp + FFmpeg in a Job Object, pinned formats, ffprobe checks, format list) | 6 tests with `sdm-fake-ytdlp`; live Creative Commons video at 360p |
| `scripts/fetch-tools.mjs` | yt-dlp and FFmpeg downloaded and SHA-256 verified |
| Desktop app (Tauri 2) + React UI | driven over WebView2 DevTools: add -> info -> progress -> complete; pause/resume; every dialog opens; browser pairing, confirmation and link list; SHA-256 of downloads checked |
| Extension 3.1: takes over browser downloads, Alt+click bypass, all/selected links, optional cookies | syntax-checked; API side covered by tests; not yet run in a real Chrome |

Run everything: `cargo test -p swiftdm-core` (105 tests).

## App features

Main window: menu bar, toolbar (add, resume, stop, stop all, delete, clear done, options,
scheduler, start/stop queue, batch), category and queue tree, configurable sortable list,
find bar (Ctrl+F / F3), status bar with total speed, limiter and quota, drag and drop of
links, tray icon, single instance.

Windows: Add download, Download file info (file or video mode with quality picker, duplicate
warning, browser requests), Download progress (status, segment map, connections, per-download
limiter, action when finished), Download complete, Properties (refresh address), Batch
download, Choose files (clipboard, import, drops, browser links), Options (10 pages),
Scheduler, extension pairing code, power-action countdown, About.

## Not done yet (in order)

1. **Real-browser E2E** of extension 3.1 (Chrome with the unpacked extension): capture,
   Alt bypass, cookies, links. Port `tests/ext/cdp_driver.mjs` to the new app.
2. **In-page video panel** polish in the extension (the v2 content script detects media and
   offers a button; it should offer the app's quality list).
3. **Automated app E2E in the repo:** move the scratch CDP driver into `tests/app/` and run it
   against `sdm-observe` in CI.
4. **`swiftdm-cli`:** forced-kill / randomized interruption runs and the 4 GiB benchmark.
5. **Packaging:** Tauri NSIS bundle including yt-dlp, FFmpeg (LGPL) and the extension;
   update `THIRD_PARTY_NOTICES.md`.
6. **Only after all of the above pass:** delete the Python app and merge to `main`.

## Commands

```powershell
cargo test -p swiftdm-core                       # all Rust tests
node scripts/fetch-tools.mjs                     # pinned yt-dlp + FFmpeg into tools\bin
ui\node_modules\.bin\tauri build --debug --no-bundle   # app at target\debug\swiftharrydm.exe
target\debug\sdm-observe.exe 18080 <out-dir>     # local test server with fixed scenarios
```
