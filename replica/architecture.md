# Architecture: ODM, Open Download Manager (a rebuild of IDM's features)

ODM is the existing SwiftHarryDM codebase, renamed at `/replica-brand`. This plan keeps
its stack and adds what `features.csv` says is missing. The web defaults in the Replica
pack (Next.js, Postgres, Stripe, auth) do not apply: ODM is a free Windows desktop app
with no accounts, no server and no payments.

## Stack

| layer | choice | why |
| --- | --- | --- |
| core | Rust crate `swiftdm-core` | already holds the engine, manager actor, store; tested headlessly |
| desktop shell | Tauri 2 (`src-tauri`) | small installer, WebView2, tray, native dialogs already wired |
| UI | React + TypeScript (`ui`), one page per window kind | exists; tokens from `/replica-design` |
| database | SQLite, WAL, synchronous=FULL, forward-only migrations | one local file, survives power loss; already in place |
| settings | JSON file via `SettingsStore` | already holds ~50 options; new options go here with `#[serde(default)]` |
| video | yt-dlp (verified weekly update) + small FFmpeg build | exists |
| browser | MV3 extension for Chrome/Edge/Brave + loopback API (axum) | exists; Firefox build added from the same source |
| crawler (new) | `swiftdm-core::grabber` on the existing HTTP client + `html5ever`/`scraper` for links | reuses the engine's proxy, logins, limits; no headless browser |
| jobs | the manager actor's timers (scheduler ticks, retries, quota, sync re-checks) | one owner, serialized transitions |
| updates | Tauri updater + GitHub Releases | exists |
| hosting | GitHub (repo, Releases, Pages for the site) | free, already used |

No new database, no services. Everything new is a module in `swiftdm-core`, a set of
Tauri commands, and windows in `ui`.

## Schema

Tables today: `jobs`, `segments`, `queues` (JSON per queue), `schema_version`.
Migration 2 (see `schema.sql`) adds 3 tables for the grabber and 1 for sync checks.
Queues stay JSON rows, so the new queue fields (`kind`, retries, open-file action,
start-on-launch, repeat interval) are serde fields with defaults, not a migration.

Access rules: none needed (single local user). The loopback API is the only outside
door: bearer token tied to the extension ID, Host/Origin checks, no CORS. Keep it that way;
every new route goes through the same middleware.

Hard constraints, in the database:
- No two unfinished jobs target the same file (exists: `jobs_reservation`).
- A grabber project never stores the same URL twice (`unique (project_id, url)`).
- A job is in at most one queue (`spec.queue_id` is a single value; keep it).

Queue rules, in the manager (not expressible in SQL over JSON rows):
- Completed jobs may only be added to `synchronization` queues.
- The two built-in queues (`main`, `main_sync`) cannot be deleted or change kind.

## API

Desktop: Tauri commands (55 today). Browser: loopback routes. New ones per flow:

| method path | does | who | input | output | flow |
| --- | --- | --- | --- | --- | --- |
| cmd `stop_all`, `delete_completed` | Downloads menu items | UI | – | ok | F09 |
| cmd `find_jobs` | find with fields, match case, whole string | UI | text, fields, flags | job ids | F09 |
| cmd `save_category`, `delete_category` | category dialog incl. sites | UI | Category | settings | F09 |
| cmd `save_queue` (extended) | queue kind, retries, open-file, repeat, start on launch | UI | Queue | queues | F04 F05 |
| cmd `add_to_queue` (extended) | refuses completed jobs on download queues | UI | job ids, queue id | ok / error | F04 F05 |
| cmd `cancel_shutdown`, `shutdown_now` | countdown dialog S12 | UI | – | ok | F04 |
| cmd `sync_check_now` | re-check a sync queue's files | UI | queue id | per-file result | F05 |
| cmd `save_server_limits` | per-server connection exceptions | UI | [{host, max}] | settings | F10 |
| cmd `play_sound` | test a sound | UI | event | ok | – |
| cmd `grabber_create`, `grabber_save`, `grabber_load`, `grabber_list`, `grabber_delete` | projects | UI | GrabberProject | project | F08 |
| cmd `grabber_explore`, `grabber_stop` | crawl | UI | project id | events `grabber:item`, `grabber:stats` | F08 |
| cmd `grabber_check`, `grabber_download_checked`, `grabber_add_to_list` | pick and download | UI | project id, item ids | job ids | F08 |
| GET `/api/settings/capture` | keys, context menu items, panel rules for the extension | extension | – | rules | F01 F02 |
| POST `/api/links` (exists) | selected-links panel reuses it | extension | links | request id | F06 |
| CLI `/s`, `/q` | start main queue; exit when the download finishes | shell | – | – | F03 |

Webhooks in/out: none.
Jobs (manager timers):
- scheduler tick, every 15 s: start/stop queues, repeat intervals.
- sync re-check, when a sync queue starts: HEAD/range request per file, compare
  size + ETag + Last-Modified with `sync_checks`; re-download changed files in place.
- retries: failed job in a queue moves to the end; lone failed job retries every 30 s until
  the queue's retry cap.
- quota window (exists).
- yt-dlp update, weekly (exists).

## The parts that bite

- **Time and DST:** schedules are local wall-clock times ("02:00 on Mon/Wed"). Store
  `HH:MM` + days, evaluate in local time each tick, never convert to UTC. A tick can be
  skipped by sleep: on wake, start a queue whose start time passed less than its window ago.
- **Sleep and power:** a queue scheduled for 02:00 does nothing if the PC sleeps. Offer
  "wake the computer" via a Windows waitable timer (Task Scheduler entry) as a should-have.
- **Shutdown safety:** shutdown only after every job is in a final state and the store is
  flushed; 30 s countdown that any input can cancel; never when the queue was stopped by hand.
- **Idempotency:** the extension can send the same capture twice (two listeners, retries).
  The duplicate-link logic is the dedupe; keep `/api/download` keyed by request id.
- **Races:** two connections finishing at once, a pause during finalize, sync re-download
  of a file the user has open (Windows locks it: download to temp, replace on close, or fail
  with a clear message).
- **Crawler limits:** depth, per-host concurrency (default 2), a delay between requests,
  `robots.txt` honored by default, max pages per project, and same-site by default. Calendars
  and query-string loops: normalize URLs and cap pages per path.
- **Rate limits and 403s:** signed video URLs expire; refresh through yt-dlp, not by
  replaying.
- **Large files:** sparse preallocation exists; check free space before starting.
- **Search:** in-memory filter over the job list is fine to ~100k rows; no index needed.
- **Privacy:** cookies are encrypted with DPAPI (exists); site-login and proxy passwords too.
  No telemetry.

## Build order

1. **Vertical slice: done.** Browser click → Download File Info → segmented download →
   complete dialog works today (F01). Re-verify it end to end with the e2e suite before
   each milestone.
2. **Must-haves (13 open), by area:**
   - Main window menus: File (stop, remove, download now, redownload) and Downloads (pause
     all, stop all, delete completed) — S01; commands `stop_all`, `delete_completed`.
   - Category dialog with sites filter — S10; `save_category`.
   - Options depth: General, File types (exceptions editor), Save to (temp dir) — S07, S08.
   - Scheduler: custom queues with full schedule, done actions + shutdown countdown —
     S11, S12; queue fields, `cancel_shutdown`.
   - Video: panel modes and rules, TS→MP4 verified, DRM refusal message — S25.
3. **Should-haves:** Firefox add-on; keys to force/prevent; configurable context menu;
   retries and failed-to-end; files-at-once; sync queues (S11, `sync_checks`); Connection
   exceptions (S09); proxy PAC/per-protocol; Downloads tab (UA, antivirus); columns dialog;
   find dialog; Q column; FTP; Windows auth (NTLM/Negotiate); **site grabber** (S22–S24,
   migration 2).
4. **Could-haves:** grabber projects/templates/schedule/link conversion; sounds; selected
   links panel; `/s` `/q`; admin warning; drag onto queues; language packs.
5. **Fixes from `/replica-entrepreneur`**, once it has run.
