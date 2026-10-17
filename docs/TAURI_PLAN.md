# SwiftHarryDM 3: Tauri 2 + Rust + React

**Goal:** close to full feature parity with Internet Download Manager (IDM), without losing
any of the correctness guarantees proven in v2 (see `AUDIT_REPORT_V2.md`).

## Architecture

```
crates/swiftdm-core     Rust library: everything except UI
  models, states        job model and explicit state machine (port of v2)
  store                 SQLite (rusqlite, bundled), WAL + synchronous=FULL, migrations
  manager               single-owner actor (tokio): queues, scheduling, retries, recovery
  fs                    Content-Disposition, sanitization, reservations, no-clobber finalize
  http                  segmented engine: probe, validated resume, dynamic split, fsync checkpoints
  media                 yt-dlp.exe + FFmpeg as Job-Object-contained processes, pinned formats, ffprobe checks
  limiter               global and per-download token-bucket speed limiter
  scheduler             queues with start/stop times, days, concurrency, after-completion actions
  categories            IDM categories (Compressed, Documents, Music, Programs, Video) -> folders
  api                   127.0.0.1 extension API (axum): Host/Origin checks, pairing, tokens, schema
  secrets               DPAPI-encrypted storage for cookies and site passwords
crates/swiftdm-testkit  fault-injection HTTP server for tests (port of tests/support/testserver.py)
crates/swiftdm-cli      headless CLI (kill/resume tests, benchmarks, scripting)
src-tauri               Tauri 2 shell: commands, events, tray, notifications, single instance,
                        autostart, clipboard monitor, power actions, windows
ui                      React + TypeScript + Vite: IDM-style UI
extension               MV3 extension: pairing, context menu, video grabber, browser download capture
```

## IDM feature map

| IDM feature | Plan |
|---|---|
| Accelerated segmented downloads, dynamic segmentation | core/http (up to 32 connections) |
| Resume, error recovery, crash recovery | core (ported with every v2 guarantee) |
| Main window: categories tree, columns, toolbar | ui |
| "Download File Info" dialog (name, category, folder, size, Start / Later) | ui + core probe |
| Per-download progress window with segment map; "Download complete" dialog | ui (separate windows) |
| Browser integration: capture downloads, context menu, video grabber | extension 3.1 + api |
| Video and audio from sites | core/media (yt-dlp.exe, FFmpeg) with a format picker |
| Queues and scheduler; shutdown/sleep/exit when done | core/scheduler + src-tauri power actions |
| Speed limiter | core/limiter |
| Proxy, user agent, site logins | core settings + secrets (DPAPI) |
| Clipboard monitoring | src-tauri |
| Batch download (wildcards), import/export lists | ui + core |
| Refresh download address | core (validated: same size and validators) |
| Tray, notifications, start with Windows | src-tauri plugins |
| Antivirus scan after download (optional) | core (configurable command) |
| Site grabber (spider) | out of scope |

## Verification (must pass before the Python code is deleted)

- `cargo test`: ports of the v2 suites (state machine, store, manager property tests,
  fs vectors, HTTP fault matrix, media lifecycle, API security).
- Forced-kill and randomized interruption runs using `swiftdm-cli` (SHA-256 checked).
- Benchmark: segmented speed-up and peak memory on a 4 GiB file.
- Live CC media test (1080p + audio, MP3).
- `vitest` for UI logic; E2E of the **built app** driven over WebView2 remote debugging (CDP).
- Real-Chrome extension E2E (pairing, capture, confirmed download, web page blocked).
- Packaged installer built and smoke-tested.

## Removal of Python

After all checks pass: delete `main.py`, `download_window.py`, `utils.py`, `src/`, `swiftdm/`,
the Python tests, the requirements files and the Python packaging. Git history keeps them.
