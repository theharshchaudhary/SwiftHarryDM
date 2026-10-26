# Test plan: ODM

ODM is a Tauri desktop app, so the browser-only Playwright setup in the Replica pack does
not fit. The same ideas are covered by what the repo already runs:

| suite | what | where |
| --- | --- | --- |
| core unit + integration | engine, manager, store, grabber, proxy, against a fault-injecting HTTP server | `cargo test -p swiftdm-core` (~150 tests) |
| e2e | the built app driven over WebView2 DevTools, plus the extension in real headless Chrome; every download checked by SHA-256; axe-core WCAG 2.1 AA scan of each window; screenshots of every window on failure | `node tests/e2e/run.mjs` (11 scenarios) |
| kill test | random TerminateProcess during downloads, then resume and verify | `swiftdm-cli kill-test` |

Selectors in the e2e harness are button texts, ARIA labels and roles, not CSS classes,
except for a few structural checks (`.tbl tbody tr`).

`A` = automated (test name), `M` = manual, `—` = does not apply.

## F01 Download a file clicked in the browser

| case | | covered by |
| --- | --- | --- |
| F01-H1 | click a link in Chrome, the info window opens, Start, the complete window shows, file intact | A `real_chrome_extension` |
| F01-H2 | download starts while the info window is open; changing the folder moves it | A `starts_while_info_is_open` |
| F01-E1 | file already in the list: duplicate choices | A core `submit_or_reuse`, e2e twice-cancelled |
| F01-E2 | server asks for a login (401): login window, saved, retried | A `login_when_asked` |
| F01-E3 | server without range support, then a cut connection | A `cuts_and_stalls_resume`, `norange` |
| F01-E4 | file changed on the server mid-download (ETag) | A `etag_change_mid_download_restarts_with_new_content` |
| F01-E5 | not enough disk space / disk full mid-write | A `insufficient_space_and_disk_full_mid_write` |
| F01-E6 | Alt-click keeps it in the browser; Insert-click forces a web page | A `real_chrome_extension` |
| F01-E7 | cancelled twice: offer to add an exception | A `extension_protocol` |
| F01-E8 | name with accents, emoji, Japanese; 300 characters; forbidden characters; CON | A `accented_emoji_and_overlong_names_are_saved` |
| F01-E9 | app killed at a random moment, then resumed | A `swiftdm-cli kill-test` (not in CI) |
| F01-E10 | double click on Start | M: Start disables while the download is set up (seen in the e2e race, now waited for) |
| F01-N1 | a web page tries to call the local API | A core API tests (Origin and Host refused) |
| F01-N2 | the extension without a token | A `extension_rules_follow_the_settings` (401) |
| F01-K1 | keyboard only: Tab to Start, Enter | M |

## F02 Download a video playing in a page

| case | | covered by |
| --- | --- | --- |
| F02-H1 | hover a player, the button, the panel lists real formats, pick one, info window | A `real_chrome_extension` |
| F02-H2 | a site yt-dlp does not know: the stream the player loaded (HLS) is offered | A `real_chrome_extension` |
| F02-E1 | HLS made of .ts segments ends up as MP4 (H.264 + AAC, full length) | M, done 2026-10-26 with a public stream |
| F02-E2 | DRM, private, live, playlist refused with a clear message | A `refuses_private_drm_playlists_and_live` |
| F02-E3 | button rules: off, mini, minimum length, sites without it, × per player | A rules route; M in Chrome for mini and length |
| F02-E4 | proxy reaches yt-dlp | A `ytdlp_gets_the_proxy_from_options` |
| F02-E5 | YouTube's bot check (needs cookies) | M (real YouTube; not automated on purpose) |

## F03 Add a download by address

| F03-H1 | Add URL, OK, info window, Start | A `small_file` |
| --- | --- | --- |
| F03-E1 | command line `/d /n`, `/s` | A `drop_target_and_command_line` |
| F03-E2 | invalid addresses (ftp:, file:, javascript:) refused | A `remove_running_job_and_bad_urls` |
| F03-E3 | `/q` exits after the download | M |

## F04 Queue downloads for the night, then shut down

| F04-H1 | Download later, queue limit 1, start, all complete, "queue finished" with its action | A `download_later_and_queue_start_with_limit_and_finish_event` |
| --- | --- | --- |
| F04-E1 | scheduled start and stop times | A `scheduled_start_and_stop` |
| F04-E2 | a file fails: goes to the end, its own retry count | A `queue_moves_a_failed_file_to_the_end_and_caps_its_tries` |
| F04-E3 | stop the queue: running downloads wait in it | A `stop_queue_requeues_running_jobs` |
| F04-E4 | countdown before shutdown, cancel | M (a real shutdown) |
| F04-E5 | computer asleep at the start time | M |
| F04-E6 | daylight saving change on a scheduled night | M (local wall-clock times are used; no UTC conversion) |

## F05 Keep files in sync with a server

| F05-H1 | changed file downloaded again over the old copy, unchanged left alone, second run does nothing | A `synchronization_queue_downloads_changed_files_again_in_place` |
| --- | --- | --- |
| F05-E1 | finished files refused by download queues | A same test + `find_bar` |
| F05-E2 | server gone during the check | A (logged as a warning; covered by the error path) — M to see the log text |

## F06 Download every link on a page

| F06-H1 | context menu, the link chooser lists the page's files | A `real_chrome_extension` |
| --- | --- | --- |
| F06-E1 | hundreds of links, duplicates, pages hidden | A core `links` route dedupe; M for 500+ links |

## F07 Numbered series

| F07-H1 | batch window: template, range, preview | A core `batch` unit tests; window opens in `every_window_opens` |
| --- | --- | --- |

## F08 Crawl a site for its files

| F08-H1 | the 4 steps, explore, list, download checked, project kept | A `site_grabber` (e2e) |
| --- | --- | --- |
| F08-E1 | link levels, folder, other sites, robots.txt (pages and files), sizes, templates, duplicates, server-named files, site folders | A `site_grabber_explores_within_its_rules_and_downloads_the_files` |
| F08-E2 | a large real site (max pages, politeness delay) | M |

## F09 Organize: categories, columns, find

| F09-H1 | category by type; category limited to sites wins | A `categorizes_by_extension`, `site_categories_win_and_only_take_their_sites` |
| --- | --- | --- |
| F09-H2 | find: where to look, match case, whole text; Q column marks | A `find_bar` |

## F10 Limit bandwidth

| F10-H1 | speed limit shared by downloads, held downloads keep their share | A `speed_limit_and_refresh_address` |
| --- | --- | --- |
| F10-E1 | quota reached, resumes later | A core quota tests |
| F10-E2 | proxy: PAC script per address, manual per protocol, bypass list | A `pac_script_sends_chosen_sites_through_the_proxy`, `proxy::tests` |

## F11 Move from IDM

| F11-H1 | import an IDM export file | A core `ef2` tests |
| --- | --- | --- |
| F11-E1 | a real export from the user's IDM | M |

## Everywhere

| case | | covered by |
| --- | --- | --- |
| X-A11Y | no serious or critical WCAG 2.1 AA problems in any window | A axe-core in the e2e run (main, info, complete, progress, login, options, scheduler, batch, links, properties, about, tips, browser, grabber) |
| X-DARK | dark theme contrast | A token check (`contrast.py tokens.dark.json`); M visual pass |
| X-UPG | database from the previous version upgrades with a backup | A `upgrades_a_version_1_database` |
| X-WIDTH | "mobile width" | — desktop windows have fixed minimum sizes |
| X-USERS | a second user's data | — one local user; the API is bound to one extension id |

## Manual pass still to do

F01-K1, F02-E3 (mini and length in Chrome), F02-E5, F03-E3, F04-E4, F04-E5, F04-E6,
F05-E2, F06-E1, F08-E2, F11-E1, X-DARK, plus the installer on a clean Windows.
