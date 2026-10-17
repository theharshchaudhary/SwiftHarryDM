# SwiftHarryDM v2: implementation report

- **Branch:** `v2-core` (based on `main` @ `c8d05fe`)
- **Date:** 2026-10-17
- **Scope:** plan phases P0–P8

## 1. Summary

The v2 download core, native HTTP engine, yt-dlp media adapter, persistence, filesystem
layer, Tk integration, secure local API, extension 3.0, tests and packaging are implemented.
v2 is now the default engine. The legacy engine remains selectable, because the agreed
removal criteria (§9) need weeks of real use and cannot be met today.

Every claim below is backed by a test or command that was actually run. Section 7 lists
what was **not** verified.

## 2. Test evidence (Windows 11, Python 3.11.9, Chrome 155)

| Suite | Result | Log |
|---|---|---|
| Full suite (all markers, incl. slow + live) | **294 passed, 3 xfailed, 0 failed** (9 min 3 s) | `docs/evidence/final_full_suite.txt` |
| Property test, 10,000 random command sequences incl. simulated crashes | passed (7 min) | `docs/evidence/p1_property_10000.txt` |
| 200 forced process kills (TerminateProcess) mid-transition | passed: DB integrity "ok" every time | `docs/evidence/p1_kills_200.txt` |
| 100 randomized download runs mixing server faults and process kills | passed: all SHA-256 correct (after fixing bug B1) | `docs/evidence/p4_interrupt_100.txt` |
| Live CC YouTube (Big Buck Bunny, CC-BY): 1080p + audio, MP3, pause/resume pinned format | 3/3 passed | `docs/evidence/p5_live_youtube.txt` |
| Benchmark | 8 segments **7.7×** faster than 1 on a 2 MiB/s-per-connection server; **4 GiB** file at **46.8 MiB peak RSS**; SHA verified | `bench/results.json` |
| Packaged build | built; bundled yt-dlp 2026.08.19 and FFmpeg n8.1.3 run from the exe | `docs/evidence/p8_build.txt` |

### 2.1 Final full-suite run

Command: `python -m pytest -p no:cacheprovider -rA -q`. The run includes:

- unit and integration tests
- real-Tk UI tests (strict thread guard)
- the full-app restart E2E (two real launches)
- the Chrome 155 extension E2E
- 51 API security tests
- the slow suites: 200 forced kills, and 100 randomized kill/fault runs
- the live CC YouTube tests

The 3 xfails are strict xfails documenting legacy-engine bugs (T1 naming, T5b stale file,
T7 false success), which v2 fixes.

### 2.2 Packaged application smoke tests

The packaged exe was run with an isolated profile and a dead HTTPS proxy, so it made no
contact with the production license server.

- Starts the v2 UI.
- `/health` returns `api: 2`.
- Rejects an evil Origin (403) and unpaired requests (401).
- Shows the offline-license error and disables downloads, matching legacy behaviour.
- Exits cleanly on `WM_CLOSE` with no errors in its log.
- Its frozen yt-dlp converted a local clip to MP3 with the bundled FFmpeg, verified by ffprobe.

Release artifact: `packaging/dist/SwiftHarryDM-2.0.0-win64.zip` (126 MB),
SHA-256 `d8bcd2be49135569420a0c9f05cef519a28f41469fb62dff1fa7ebe0112b949d`. The artifact is
git-ignored; rebuild it with `python packaging/build.py --zip`.

## 3. Bugs found and fixed during implementation

| # | Bug | How it was found | Fix |
|---|---|---|---|
| B1 | **Data corruption**: a non-ranged stream with a known length was dynamically split; the new segment received the file from byte 0 and wrote it at the split offset | Randomized kill/fault run #14 (reproduced 6/6) | Never split without byte ranges, plus a guard that non-ranged streams start at 0. Regression test |
| B2 | `reserve()` skipped the existence check for the job's own name, so finalize would loop forever if a file appeared after reservation | P2 race test | The existence check is now unconditional |
| B3 | A resume restart after "file changed on server" reused the stale probe | Code review | Re-probe before re-planning |
| B4 | The Details button was broken: an attribute shadowed the method | UI test | Renamed |
| B5 | Bridge kept draining after app close inside a modal dialog (TclError) | Packaged-exe smoke test | Drain stops immediately. Regression test |
| B6 | yt-dlp geo-block message not recognised | Unit test | Wider pattern |
| B7 | Sanitizer trimmed meaningful spaces | Unit test | Trim only after truncation |
| B8 | Mixed `/` and `\` in saved paths | Screenshot review | Paths normalized |
| B9 | Invisible column headings in the segment table | Screenshot review | Heading style fixed |

## 4. Implemented features

The feature-by-feature list is in `docs/PARITY.md`. Highlights:

- **Core:** stable UUID job IDs; an explicit state machine where COMPLETED is reachable only
  through FINALIZING; a single-owner actor manager; SQLite WAL with `synchronous=FULL`;
  persist before publish; restart recovery table; job-level retries with backoff and
  `Retry-After`; disk full pauses the job (resumable).
- **HTTP engine:** `Range: bytes=0-0` probe; manual redirects (max 10, https→http
  refused); TLS verification on; every 206 validated; `If-Range` with a strong ETag or
  Last-Modified; 64 KiB overlap re-verification on resume; write → fsync → checkpoint;
  free-space check and preallocation; up to 16 segments with dynamic splitting; real speed
  and ETA.
- **Filesystem:** RFC 6266 / 5987 names reduced to a safe basename; Windows reserved names;
  UTF-16 length limits; extension inferred from Content-Type; no-clobber rename to
  `name (N).ext`; long paths; permission errors shown with the folder and no silent fallback.
- **Media:** yt-dlp runs as a subprocess inside a Job Object (`KILL_ON_JOB_CLOSE`), so
  cancel, pause or an app crash kills the whole process tree (tested). Probe-time format
  pinning; merge-capable presets; FFmpeg bundled (LGPL v3); success requires exit code 0, an
  output file, and ffprobe checks (streams, height, audio, duration). Playlists, live
  streams and DRM are refused; no access-control bypass.
- **UI:** same layout and theme; rows keyed by job ID; per-row actions; non-modal details
  with real segments; settings, diagnostics and a checksum-verified yt-dlp updater; all
  widget updates through `TkBridge`. A strict Tcl thread guard recorded 0 violations.
- **Local API:** loopback only; Host check; no CORS headers; web origins rejected; pairing
  code → 256-bit token (stored hashed, bound to the extension ID); schema, size and rate
  limits; in-app confirmation.
- **Extension 3.0:** pairing UI, token auth, localhost-only host permissions, `textContent`
  for page-supplied text.

## 5. Security findings

| Finding | Status |
|---|---|
| **Code-signing certificate `SwiftHarryDM.pfx` in public git history** (`8c794dc`; repo is public) | Investigated. It is password-protected, but uses weak legacy encryption (SHA-1 MAC, 2,000 iterations), so a weak password could be brute-forced. The password was never committed (0 matches in history). **Recommendation: revoke and reissue the certificate.** History was **not** rewritten (destructive; needs your authorization). `.gitignore` now blocks signing material |
| Any website could queue downloads (wildcard CORS) | Fixed in legacy (P0) and v2 (P7); verified in real Chrome |
| TLS verification disabled in legacy yt-dlp options | Fixed (P0); v2 always verifies |
| License key and raw machine GUID printed to stdout | Fixed in legacy; v2 logging redacts keys, tokens and URL query strings |
| Download progress lied (×10 speed, fake segments, fake pause/cancel) | Removed in v2 |
| `innerHTML` with page-supplied titles in the extension | Fixed (`textContent`) |

## 6. Dependency licenses

See `THIRD_PARTY_NOTICES.md`. The runtime uses only permissive licenses (MIT, BSD-3,
Apache-2.0, MPL-2.0, Unlicense) plus FFmpeg under LGPL v3, shipped as separate replaceable
executables with the license text and a source pointer. No GPL components are bundled.
PyInstaller's bootloader exception applies.

## 7. Known limitations and unverified items

1. **Licensing was not exercised against the production license server.** It's covered by
   unit tests against a fake server. Live calls were avoided on purpose: on this machine
   they would have started a real trial on your server. The packaged exe was smoke-tested
   offline (a dead proxy), and it correctly showed "Cannot start trial" and disabled
   downloads.
2. **The Inno Setup installer was not built** (ISCC isn't installed). The script exists;
   the one-folder build and zip were built and smoke-tested.
3. **Nothing is code-signed** (no certificate configured, by design).
4. **Media sites other than YouTube** (Vimeo, Dailymotion, …) are routed to yt-dlp but were
   not live-tested.
5. **Real-world internet throughput against IDM was not measured.** The benchmark uses a
   local throttled server, which is repeatable but not the internet.
6. **The extension was tested in Chrome only** (not Edge or Firefox). Page scanning and
   context menus were not automated; the send-to-app path was.
7. **Disk-full** was tested through fault injection, not on a real full volume.
8. **No JavaScript runtime is bundled.** YouTube works without one in the tested case, but
   yt-dlp recommends one (Node was present on the test machine).
9. **Playlists and live streams are unsupported**, pending your scope decision.
10. The old 2.0 extension does not work with the v2 engine. Users must load extension 3.0.
11. Dragging a URL into the window and clipboard monitoring aren't implemented (neither
    was in legacy).

## 8. Decisions still open for Harsh

- **Licensing:** the trial system vs the MIT/open-source README. Behaviour is unchanged
  and waiting on this decision.
- **Revoke the `.pfx` certificate, and rewrite git history?**
- Playlists in scope?
- Bundle a JS runtime (Deno ≈ 100 MB) or rely on one being installed?
- Merge `v2-core` into `main`. Not done: needs your review.

## 9. Legacy removal

**Not yet.** The criteria in `docs/MIGRATION.md` (≥ 4 weeks as default, 14 green nightly
runs, zero P0/P1 bugs, no fallback reports, and your parity sign-off) can't be met on day
one. The legacy engine stays selectable.
