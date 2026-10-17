# Tauri 2 + Rust + React rewrite: status (2026-10-17, end of day)

Branch: `tauri-rewrite`. `main` still holds the shipped Python v2 app (unchanged and working).

## Done and verified

| Area | Evidence |
|---|---|
| Toolchain | Rust 1.99.1 (MSVC), VS 2022 Build Tools + Windows SDK 10.0.26100 installed |
| `swiftdm-core` foundation: models, state machine, SQLite store, settings, categories, DPAPI secrets, WinINET proxy, speed limiter, scheduler | 37 unit tests pass |
| Manager actor: queues, Download later, scheduled start/stop, after-queue actions, recovery, retries, refresh address, AV scan hook | 13 integration tests pass |
| Randomized invariant test (incl. simulated crashes) | 200 sequences pass; found and fixed a queue-accounting bug |
| Native HTTP engine | 21-case fault matrix passes (resume, validators, B1 regression, cookies/logins scoping, speed limit, disk full, refresh address) |
| `swiftdm-testkit` fault-injection server (Rust port) | used by the matrix above |
| `scripts/fetch-tools.mjs` | yt-dlp.exe 2026.08.19 download + SHA-256 check works |

## Not done yet (in order)

1. **Fix `fetch-tools.mjs` FFmpeg extraction.** It calls `tar`, which resolves to Git's GNU tar (no zip support) in Git Bash. Use `C:\Windows\System32\tar.exe` explicitly, or PowerShell `Expand-Archive`.
2. **Media engine (Rust):** yt-dlp.exe in a Job Object, format pinning and a format list for the picker, progress parsing, error map, ffprobe validation; a fake yt-dlp test binary; live CC test.
3. **Local API (axum)** with the same pairing protocol as v2; extension 3.1 (browser download capture, optional cookies permission).
4. **`swiftdm-cli`:** forced-kill / randomized interruption runs and the benchmark; full 10,000-sequence run.
5. **Tauri app + React UI (IDM-style):**
   - main window with categories and columns;
   - Download File Info, progress and complete dialogs;
   - scheduler, options, batch, import/export;
   - tray, clipboard monitor, notifications, autostart, power actions.
6. **E2E of the built app** over WebView2 remote debugging; real-Chrome extension E2E.
7. **Packaging** (Tauri NSIS/MSI bundle with tools), docs, an IDM parity checklist.
8. **Only after all of the above pass:** delete the Python app (`main.py`, `swiftdm/`, Python tests and packaging), then merge to `main`.

## Commands

```powershell
cargo test -p swiftdm-core            # all Rust tests so far
node scripts/fetch-tools.mjs          # pinned tools into tools\bin (FFmpeg step needs fix 1)
```
