# Recon map: Internet Download Manager (Windows desktop)

Scope: IDM 6.43's download manager as a whole: capture from the browser, segmented
downloading, the main list with categories and queues, the scheduler, the site grabber,
options, video capture. Rebuilt into the existing SwiftHarryDM codebase, launched as
**ODM (Open Download Manager)**.
For: a free, open-source Windows download manager for people who use (or pirate) IDM
and want the same workflow without a license or a nag.
Date: 2026-10-25

## How this map was made

- IDM's public site and help pages (table below).
- The existing checklist [docs/IDM-PARITY.md](../docs/IDM-PARITY.md) (47 open, 27 done
  on 2026-10-25), which this map supersedes as the feature source of truth for the
  Replica steps. The checklist says it was built partly from the resources of
  `IDMan.exe`. **From here on, no more reading IDM's binary or its resources**: the
  Replica rules allow public sources and the user's own copy, driven by the user, only.
  IDM's license agreement also forbids reverse engineering. Nothing from that earlier
  reading (text, icons, layouts copied pixel for pixel) may ship; the wording in the
  app is our own and `/replica-brand`'s sweep checks it.
- Screenshots of IDM, if taken, go in `replica/screens/` and are never shipped.

## Sources

| # | source | URL | notes |
| --- | --- | --- | --- |
| 1 | features page | https://www.internetdownloadmanager.com/features2.html | 21 headline features: segmentation, browser takeover, scheduler, grabber, quotas, antivirus hook, proxy, auth types |
| 2 | pricing page | https://secure.internetdownloadmanager.com/buy_idm.html | $11.95/yr or $24.95 lifetime per PC; 30-day trial. Nothing feature-gated: the whole app is the paid product |
| 3 | changelog | https://www.internetdownloadmanager.com/news.html | 6.43 builds 1–16 (Jun–Oct 2026) are almost all video: missing download panel, 403s, TS→MP4, MKV mux. Video is what keeps users paying |
| 4 | help: main window | https://www.internetdownloadmanager.com/support/using_idm/using_idm.html | list, Q column, toolbar, categories tree, columns and toolbar customize |
| 5 | help: starting downloads | https://www.internetdownloadmanager.com/support/using_idm/starting.html | the ways in: browser click, Add URL, drag, clipboard, command line |
| 6 | help: download complete | https://www.internetdownloadmanager.com/support/using_idm/completeD.html | complete dialog |
| 7 | help: properties | https://www.internetdownloadmanager.com/support/using_idm/properties.html | per-download save path, description, URL, login |
| 8 | help: options | https://www.internetdownloadmanager.com/support/using_idm/options.html | tabs General, File types, Connection, Save to, Downloads, Proxy/Socks, Site logins, Dial-up/VPN, Sounds |
| 9 | help: quick update | https://www.internetdownloadmanager.com/support/using_idm/update.html | weekly update check |
| 10 | help: scheduler | https://www.internetdownloadmanager.com/support/idm-scheduler/idm_scheduler.html | one-time vs periodic synchronization, start/stop times, retries, done actions, files tab |
| 11 | help: queues | https://www.internetdownloadmanager.com/support/idm-scheduler/idm_queues.html | main download queue, synchronization queue, custom queues |
| 12 | help: grabber intro, wizard, main dialog, settings, scheduler | https://www.internetdownloadmanager.com/support/idm-grabber/idm_grabber.html | 4-step wizard, explore/download, projects, filters |
| 13 | FAQ: browser integration, video, sharing sites | https://www.internetdownloadmanager.com/register/new_faq/bi_main.cgi | what breaks for users, what they ask for |
| 14 | the user's own IDM install | (local, user-driven) | to check states and flows by hand, never to read the binary |

## Core loop

Click a download link (or play a video) in the browser, IDM takes it over, shows the file
info, and pulls it down faster than the browser with segmented connections, resuming
after any failure.

## Screens

| ID | screen | route / how to reach | purpose | key components | states seen |
| --- | --- | --- | --- | --- | --- |
| S01 | Main window | app start, tray icon | the download list | menu bar, toolbar, categories tree, list with columns, status bar, find bar | empty, filled, downloading, categories hidden, sorted, dark |
| S02 | Add URL | toolbar Add URL, Tasks menu, tray | start a download by address | URL field (clipboard prefilled), login checkbox + fields | empty, prefilled, invalid URL |
| S03 | Download File Info | after any captured or added download | confirm name, folder, category; start now or later | category select, folder picker, description, remember-path check, Start/Later/Cancel, zip preview | probing, size known, size unknown, file exists |
| S04 | Download progress | starting a download | watch one download | tabs (status, speed limiter, on completion), segment bar, connections table, Pause/Cancel, hide details | downloading, paused, retrying, error, resume unsupported |
| S05 | Download complete | download finishes | act on the file | Open, Open with, Open folder, Close, don't show again, file drag | complete |
| S06 | Properties | list context menu | edit one download | save path, description, URL, referer, login | complete, incomplete |
| S07 | Options | toolbar Options, Downloads menu | settings | tabbed dialog: General, File types, Connection, Save to, Downloads, Proxy/Socks, Site logins, Sounds | dirty, applied |
| S08 | Keys dialog | Options > General > Keys | force/prevent takeover with modifier keys | key checkboxes per action | |
| S09 | Connection exceptions | Options > Connection | per-server connection counts | table with new/edit/delete | empty, filled |
| S10 | Category dialog | categories tree context menu | add/edit a category | name, file types, sites, folder | new, edit |
| S11 | Scheduler | toolbar Scheduler | queues and their schedules | queue list, Schedule tab, Files in the queue tab, Start now/Stop | one-time queue, sync queue, running |
| S12 | Shutdown countdown | queue done with "turn off computer" | last chance to cancel | countdown, Shut down now, Cancel | counting |
| S13 | Batch download | Tasks menu | numbered series (`file*.zip`, 1–50) | template, from/to, letters/numbers, wildcard size | preview |
| S14 | Download all links | browser context menu, Tasks | pick links from a page | link list with type/size, filters, check all, rename pattern, save mode | checking sizes, filtered |
| S15 | Duplicate link | adding an address already in the list | choose what to do | numbered / overwrite / resume existing, remember | |
| S16 | Delete confirmation | Delete | confirm removal | also delete files from disk, don't show again | complete, incomplete |
| S17 | Login prompt | server returns 401 | enter credentials | user, password, save password | |
| S18 | Find | Downloads > Find | search the list | text, fields, match case, whole string; find next | no match |
| S19 | Columns | list header context menu | pick and order columns | checklist, up/down, width | |
| S20 | Toolbar customize | toolbar context menu | pick and order buttons | two lists, add/remove/up/down, size, style | |
| S21 | Export / import | Tasks menu | move lists in/out | file picker, `.ef2` or text, selection scope | |
| S22 | Grabber wizard (4 steps) | toolbar Grabber | set up a site crawl | start page + login + template; save options; explore filters; file filters | each step, back/next, finish |
| S23 | Grabber main window | wizard finish, saved project | crawl and pick files | site tree, file list, explore/stop, check/uncheck, download checked, stats | exploring, stopped, done |
| S24 | Grabber settings / scheduler | grabber window | concurrency, project schedule | numeric fields, schedule like S11 | |
| S25 | Video download panel | in the browser, over a playing video | download the stream | floating button, format/quality list, mini mode, close | detecting, formats listed, protected |
| S26 | Selected links panel | browser, after selecting text with links | download the selected links | floating button | |
| S27 | Drop target | View or tray | drag links onto a small box | floating box with progress tint | idle, dragging, downloading |
| S28 | Tray menu | tray icon right-click | quick actions | menu | |
| S29 | Speed limiter | Downloads menu, tray | global limit on/off and value | menu + value dialog | on, off |
| S30 | Quick update / about | Help menu | updates and version | progress, changelog | up to date, available, failed |
| S31 | Tip of the day | Help menu, startup | tips | text, next/previous, show at startup | |
| S32 | Takeover failed notice | extension could not hand over | explain, offer fix | message, don't show again | |
| S33 | Registration / trial nag | trial ends | buy a license | serial fields | (skip: ODM is free) |

## Flows

```
F01 Download a file clicked in the browser (the core loop)
    browser click -> S03 Download File Info -> (S04 progress) -> S05 complete
    happy path clicks: 2 (the link, Start). IDM starts transferring while S03 is open.
    edge: file exists (S15), server needs login (S17), no resume support, 403 after a pause,
          size unknown, file > disk space, user holds the "prevent" key, site in exceptions

F02 Download a video playing in a page
    page with player -> S25 panel -> pick quality -> S03 -> S05
    happy path clicks: 3
    edge: HLS/DASH stream, separate audio+video (mux), TS only (convert to MP4), DRM
          (refuse, "protected"), panel missing on a site, 403 on a signed URL

F03 Add a download by address
    S01 -> S02 Add URL -> S03 -> S05
    happy path clicks: 3 (Add URL, OK, Start)
    edge: invalid URL, clipboard has a URL (prefilled), login needed

F04 Queue downloads and run them at night, then shut down
    S01 select files -> context "Add to queue" -> S11 Scheduler -> start at 02:00,
    when done turn off computer -> S12 countdown
    happy path clicks: ~8
    edge: computer asleep at start time, app closed (needs tray), a file fails (retries,
          moves to end), stopped manually (no done action)

F05 Keep files in sync with a server
    S11 -> new synchronization queue -> add completed files -> periodic every N hours
    edge: server file unchanged (skip), changed (re-download, replace), server gone

F06 Download every link on a page
    browser context menu -> S14 -> filter by type -> check -> save to category -> S01
    happy path clicks: 4
    edge: hundreds of links, duplicates, web pages vs files, relative links

F07 Download a numbered series
    S13 batch -> template + range -> preview -> add to queue
    edge: zero-padded numbers, letters, gaps (404s)

F08 Crawl a site for all its PDFs
    S22 wizard (4 steps, template "all files") -> S23 explore -> check -> download
    happy path clicks: ~7
    edge: site needs login, robots/rate limits, infinite calendars, other domains,
          JavaScript-built links

F09 Organize: categories, columns, find
    S01 categories tree -> S10 edit category (types, folder) -> S19 columns -> S18 find
    edge: file matches two categories, moving a category's folder

F10 Limit bandwidth
    S29 speed limiter on -> per-download limit on S04 -> quota in S07 Connection
    edge: quota reached mid-download (pause, resume later), several downloads share the limit

F11 Move from IDM
    S21 import `.ef2` from IDM -> list appears in S01
    edge: half-finished IDM downloads (restart, IDM's temp parts are not readable)
```

## Components

| component | variants | states | used on |
| --- | --- | --- | --- |
| Dialog window (fixed size, native-feeling) | modal, modeless, tool | focused, background | all except S01 |
| Menu bar + menus | top-level, submenu, checked item, radio item | enabled, disabled | S01 |
| Toolbar button | large, small; with drop-down | enabled, disabled, pressed | S01 |
| Categories tree | category, queue, grabber project | selected, drop target | S01 |
| Download list (virtualized table) | sortable columns, Q column icons, file icons | empty, selected, multi-select, row states by job status | S01, S11, S14, S23 |
| Context menu | list, tree, header, toolbar, tray | | S01, S28 |
| Segment progress bar | per-connection segments | downloading, paused, error | S04 |
| Plain progress bar | | indeterminate, determinate | S04, S30, S27 |
| Tabs | | active, hidden tab | S04, S07, S11 |
| Text field / URL field | single-line, password, number with spinner | default, focus, invalid, disabled | S02, S03, S07 |
| Folder picker field | field + browse button | | S03, S07, S10 |
| Checkbox, radio group | | checked, unchecked, disabled | everywhere |
| Editable table | new/edit/delete buttons | empty, filled | S07, S09 |
| Ordered list editor | add/remove/up/down | | S19, S20, S11 files |
| Time/day picker | time, days of week | | S11, S24 |
| Countdown dialog | | | S12 |
| Browser overlay panel | full, mini | detecting, ready, protected | S25, S26 |
| Toast / tray balloon | | | complete, failed, update |
| Status bar | | | S01 |

## Inferred data model

```
Download   id, url, final_url, referer, parent_page, file_name, save_dir, category_id,
           size (nullable), downloaded, status (queued|downloading|paused|complete|error),
           resumable, connections, speed_limit, description, login (user, pw),
           added_at, last_try_at, completed_at, server_modified_at, queue_id (nullable),
           cookies/user_agent for the request
           evidence: list columns (S01), Properties (S06), arrange-by menu, help #4 #7
           confidence: high

Segment    download_id, start, end, done, connection index
           evidence: S04 segment bar and connections table
           confidence: high (exact persisted shape is a guess)

Category   id, name, file_types[], sites[], folder, remember_last_folder, built_in
           evidence: S10, help #4 ("*.googlevideo.com" site restriction)
           confidence: high

Queue      id, name, kind (download|synchronization), built_in (main / main sync),
           start_at, days[], repeat_every, stop_at, start_on_launch, max_parallel,
           retries, done_action (none|open|exit|shutdown|force_shutdown), done_open_path,
           files (ordered download_ids)
           evidence: S11, help #10 #11
           confidence: high

ServerConnectionException  host pattern, max_connections
           evidence: S09; confidence: high

SiteLogin  url pattern (server + path), user, password
           evidence: S07 Site logins; confidence: high

CaptureRules  file_types[], site_exceptions[], address_exceptions[], keys (force/prevent),
           min_size for player panel, panel sites exceptions
           evidence: S07 General/File types, S08; confidence: high

Quota      amount MB, per N hours, warn before stopping
           evidence: help #10 download limits; confidence: high

GrabberProject  name, start_url, template, login, manual_login_page, logout_pages[],
           save_mode, relative_subfolders, convert_links, overwrite, explore_levels
           (this site / other sites), ignore_popups, no_parent, main_domain, run_js,
           include/exclude paths, file filters (include/exclude, min/max size),
           this_site_only, hide_duplicates, schedule
           evidence: S22, S24, help #12; confidence: high
GrabberItem project_id, url, parent_url, depth, type, size, checked, status
           confidence: guess

Settings   one row of everything else (startup, clipboard, sounds per event, proxy,
           temp dir, UA, antivirus path+args, theme, font, language, toolbar layout,
           columns, dialog toggles)
           confidence: high (shape is a guess)
```

Relationships: Category 1-n Download, Queue n-n Download (ordered; IDM lets a file be in
one download queue, completed files only in sync queues), Download 1-n Segment,
GrabberProject 1-n GrabberItem, GrabberItem 0-1 Download.

Already in SwiftHarryDM: `Job` (Download), segments, categories, queues and scheduler,
quota, capture rules, site logins, settings in SQLite (see
[docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md)). Missing entities: synchronization queue
kind, GrabberProject/GrabberItem, sounds.

## Feature matrix

See `features.csv`. Must: 39, should: 31, could: 17, skip: 6.
Clone column on 2026-10-25: yes 47, partial 23, no 17. Parity 72.2, must-haves 26 of 39.

## Out of scope (cannot or should not be cloned)

- IDM's name, icon, toolbar artwork and skins ("3D", "Neon"), dialog wording, help text.
- Registration, serial numbers, trial nag, "Tell a friend" (ODM is free; no service behind them).
- Dial-up/VPN tab and "hang up modem": no dial-up modems on modern Windows.
- IE monitor module, Opera/IE plugins, "Add browser" for IE: those browsers are gone.
- Downloading DRM-protected streams: IDM refuses, so do we (legal line, not a gap).
- MMS protocol: dead.
- IDM's site-specific deals or workarounds for file hosts ("sharing sites" FAQ): we use
  yt-dlp's public extractors, not private APIs.

## Size

Screens 33 (1 skipped), flows 11, entities 10. About 60% is built already.

Hard parts:
1. **Video capture that keeps working.** IDM's changelog is 16 builds of "missing panel,
   403, TS→MP4" in four months. It is a treadmill: yt-dlp updates, stream sniffing in the
   extension, muxing with FFmpeg.
2. **The site grabber.** A crawler with depth rules, filters, login, link conversion for
   offline browsing, projects and schedules. A new subsystem, nothing exists yet.
3. **Synchronization queues and the scheduler's full rules** (periodic re-checks of
   finished files, shutdown with countdown, retries moving files to the end), plus a
   Firefox add-on for the extension.

Size remaining: **L** (a quarter), most of it the grabber and the Options depth.
