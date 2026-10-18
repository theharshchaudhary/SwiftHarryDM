# Architecture

```
crates/swiftdm-core     Everything except the user interface (tested headlessly)
  models, states        Download model and its state machine
  store                 SQLite (WAL, synchronous=FULL), forward-only migrations
  manager               Single-owner actor: queues, scheduling, retries, recovery, quota
  http                  Segmented engine: probe, validated resume, dynamic splitting,
                        boundary checks between connections, fsync before checkpoints
  media                 yt-dlp and FFmpeg in a Windows Job Object, pinned formats,
                        ffprobe checks, on-demand install and verified updates
  api                   Loopback API for the browser extension (axum)
  capture, batch,       Browser take-over rules, numbered series and URL lists,
  search, wildcard      Find, * patterns
  licensing             License and trial checks
crates/swiftdm-testkit  Fault-injecting HTTP server for tests; sdm-observe scenario server
src-tauri               Desktop shell (Tauri 2): commands, event bridge, windows, tray,
                        clipboard monitor, notifications, power actions, native messaging
                        host, updates, license
ui                      React + TypeScript windows (one page, the window kind is injected)
extension               Chrome/Edge extension (Manifest V3)
tests/e2e               End-to-end tests of the built app and the extension in Chrome
```

## Download lifecycle

A download is a `Job` owned by the manager actor. UI calls, API calls, worker results,
retry timers and schedule ticks are all messages to the actor, so transitions are
serialized and every one is written to SQLite before its event is published.

```
QUEUED -> STARTING -> DOWNLOADING -> (POSTPROCESSING) -> FINALIZING -> COMPLETED
                \-> PAUSING -> PAUSED       \-> RETRY_WAIT -> QUEUED     \-> FAILED
```

The HTTP engine writes each connection's bytes directly into one preallocated file, so
there is no joining step at the end. A connection's committed offset is recorded only after
its data is flushed; after a crash only committed bytes are trusted, and resuming re-reads a
little of the existing data to make sure the file on the server has not changed.

When a connection finishes early, the largest remaining part is split in half and a new
connection takes the second half. Where two parts meet, a few kilobytes are compared
across connections (during the transfer, or in a final check); if mirrors behind one
address serve different files, the download restarts on a single connection.

## Browser connection

The extension talks to `127.0.0.1:5001` (or 5002). Requests need a bearer token tied to the
extension's ID; web pages cannot call the API (Host and Origin checks, no CORS).

The token comes from the app's native messaging host: the app registers itself for Chrome,
Edge, Brave and Chromium at every start, and the browser only lets the extension with the
listed ID start it. So the extension connects with nothing to type. Pairing with a one-time
6-digit code remains as a fallback.

Browser downloads are checked with `/api/capture/check` (type, size, exception lists); if
the app takes one, the extension cancels it in the browser and sends it with its referrer
(and cookies, if the user allowed that).

## Windows and events

Every window loads the same page; the shell injects the window kind and its parameters
before the page runs. Windows subscribe to one `core` event stream (job changes, progress,
queue, quota, license and update events) after reading a full snapshot.

## Updates

- App: the Tauri updater with a signing key; GitHub Releases host the installer and
  `latest.json` (see `.github/workflows/release.yml`).
- yt-dlp: weekly, checked against the release's SHA2-256SUMS and test-run before use.
- FFmpeg: pinned build, installed on demand and verified by SHA-256.
