# Feature parity: legacy app vs v2

**Legend**
- ✅ = present and verified by an automated test.
- 🟡 = present but only partly verified (the reason is given).
- ⛔ = deliberately dropped (the reason is given).
- ✏️ = behaviour changed on purpose.

Harsh's sign-off on this list is one of the conditions for removing the legacy engine
(see `MIGRATION.md`).

## Main window

| Legacy capability | v2 | Evidence / note |
|---|---|---|
| URL box + quality choice (best / 1080 / 720 / 480 / mp3 / mp4) | ✅ | Same values plus 2160 / 1440 / 360 / mkv / m4a. `tests/ui`, `tests/e2e` |
| Add to Queue (pending) and Start All | ✅ | `test_add_to_queue_then_start_all` |
| Instant Download | ✅ | `test_instant_download_through_the_window` |
| Pause All / Resume All | ✅ ✏️ | Now actually pauses: legacy only changed the label. `test_pause_resume_cancel_buttons`, P3 pause tests |
| Clear Completed | ✅ ✏️ | No longer duplicates rows or disturbs running jobs. `test_clear_completed_leaves_running_rows_untouched` |
| Open Download Folder | 🟡 | Implemented (`os.startfile`); opens Explorer, not automated |
| Ask for a save folder on each download | ✅ | Setting "Ask where to save each download" (default on, as before) |
| Per-item progress bar | ✅ ✏️ | Real bytes; indeterminate when the size is unknown |
| Activity log | ✅ | |
| Status bar | ✅ ✏️ | Was static text; now real totals and speed |
| Same dark theme and colours | ✅ | Theme ported (`swiftdm/ui/theme.py`); screenshot reviewed |
| Keyboard shortcuts | ✅ ✏️ | Legacy only showed labels; Ctrl+N, Ctrl+I, F5/F6/F7 and Enter now work |

## Progress window

| Legacy | v2 | Note |
|---|---|---|
| A modal window opened automatically for every download | ✏️ | Non-modal Details window, opened on demand, so it no longer blocks the main window |
| Segment table | ✅ ✏️ | Shows **real** segments (legacy simulated 8 fake rows). `test_details_window_shows_real_segments` |
| Speed shown ×10 "for premium feel" | ⛔ | Removed: the speed is now measured from real bytes |
| Cancel button | ✅ ✏️ | Now actually stops the transfer and deletes the partial file |
| Hide button | ✏️ | Replaced by closing the window; the download continues |

## Menus

| Legacy item | v2 | Note |
|---|---|---|
| File > Add / Instant / Exit | ✅ | |
| File > Import List…, Export List… | ⛔ | Had no implementation in legacy |
| Download > Start / Pause / Resume | ✅ | |
| Download > Schedule…, Category… | ⛔ | Had no implementation in legacy |
| View > Toolbar / Status Bar / Language / Skin | ⛔ | Had no implementation in legacy |
| License > Management, Enter Key, Check Status, View License | ✅ | Same rules (`tests/test_licensing_updates.py`); dialogs moved onto the UI thread |
| Help > Documentation | ⛔ | Had no implementation; README and `docs/` replace it |
| Help > Check for Updates | ✅ ✏️ | Now validates the version number (legacy reported any error page as an update) |
| Help > About | ✅ | |
| — | ✅ new | Settings, Diagnostics, Update yt-dlp, Open Log Folder |

## Downloads

| Legacy | v2 | Note |
|---|---|---|
| Direct HTTP/HTTPS files (through yt-dlp) | ✅ ✏️ | Native engine: segmented, verified resume, correct names. P3/P4 suites |
| Automatic retries (3) | ✅ | Per-segment backoff plus job-level retries (3) |
| YouTube 1080p / 720p / 480p | ✅ ✏️ | Legacy actually produced 360p. Live test confirms 1080p with audio (`test_live_youtube.py`) |
| MP3 | ✅ ✏️ | FFmpeg is bundled. Legacy's auto-installer was never called and was broken. Live and offline tests |
| Other sites (Vimeo, Dailymotion, …) | 🟡 | Routed to yt-dlp (`test_router_recognises_media_sites`); downloads from these sites were not live-tested |
| Playlists | ⛔ (unchanged) | Legacy used `noplaylist` too. v2 refuses playlist URLs with a clear message. Awaiting your scope decision |
| Facebook fallback downloader | ⛔ | Dead code in legacy |

## Browser extension

| Legacy | v2 | Note |
|---|---|---|
| Send a video, audio or link to the app | ✅ ✏️ | Requires pairing plus in-app confirmation (can be turned off). Real-Chrome E2E test |
| Context menu, scan page, download all media | 🟡 | Kept unchanged in content.js; send path verified, page scanning not automated |
| Unauthenticated API with `Access-Control-Allow-Origin: *` | ⛔ | Security fix: any website could queue downloads |
| `/api/queue` | ✏️ | Now `/api/jobs`, authenticated |
| Extension "Add to Queue" auto-started the download | ✏️ | Now queued; "Download Now" starts it |
| Old extension (2.0) working with the new app | ⛔ | Extension 3.0 is required for v2. The legacy engine still accepts 2.0 |

## v2 requirements (from the agreed plan)

| Requirement | Status |
|---|---|
| Stable job IDs, state machine, persistence, restart recovery | ✅ (P1 tests, 10,000 property sequences, 200 forced kills, full-app restart E2E) |
| Segmented transfer, resume validation, cancellation, collisions | ✅ (P2–P4 suites, 100 randomized kill runs) |
| yt-dlp adapter: pinned formats, process-tree kill, ffprobe validation | ✅ (P5 offline + live) |
| Thread-safe Tk integration | ✅ (strict guard, 0 violations; p95 latency 12.5 ms) |
| Local API security and pairing | ✅ (51 API tests + Chrome E2E) |
| Packaging | ✅ one-folder build + zip built and smoke-tested; 🟡 Inno Setup installer script not built (ISCC not installed) |
| Legacy removal | ⛔ not yet (criteria in `MIGRATION.md`) |
