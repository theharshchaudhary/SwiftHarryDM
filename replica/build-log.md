# Build log

| date | screen / feature | status | notes |
| --- | --- | --- | --- |
| 2026-10-25 | shell + vertical slice (S01–S05, F01) | done | already built before the Replica run |
| 2026-10-25 | audit of open must-haves | done | the IDM-PARITY checklist was stale: menus, File types, Save to, schedule times, countdown, DRM refusal were already built |
| 2026-10-25 | S10 category sites filter | done | core `categorize` takes the host; site categories win; Options > Save to edits them |
| 2026-10-25 | S25 player download button rules | done | `/api/settings/player`; mini, min length, skip sites, × per player. e2e real_chrome_extension passes. Harder than expected: none, but the e2e suite was red on CI before this run (flaky timing; passes 2/2 locally) — for /replica-test |
| 2026-10-25 | TS to MP4 | done | verified with a trimmed public HLS stream through swiftdm-cli: MP4, h264+aac, full length |
| 2026-10-25 | S18 find options | done | look in / match case / whole text in the find bar; new e2e scenario find_bar |
| 2026-10-25 | S01 Q column | done | queue icon, clock when scheduled, tooltip; covered in find_bar |
| 2026-10-25 | S11 queue retries, failed file to end | done | Queue.retries; test fails without the fix, passes 5/5 with it |
| 2026-10-26 | virus check | done | already built (I missed it in the audit); added a notification when the scanner reports a problem — compile-checked, not e2e |
| 2026-10-26 | S07/S08 keys and context menu | done | /api/settings/extension; rules pushed to open pages when they change (pages loaded before pairing kept stale rules). e2e Insert+click |
| 2026-10-26 | S11 synchronization queues (F05) | done | no schema change needed (queues are JSON; replacement is a new job with spec.replaces) so replica/schema.sql's sync_checks table is not used. Completed stays terminal. Repeat-every-N-hours not built |
| 2026-10-26 | S07 proxy (PAC, system auto-config, bypass, per protocol) | done | Resolver via reqwest Proxy::custom; yt-dlp never got a proxy before (fixed). Test kit serves proxy-form requests |
| 2026-10-26 | e2e reliability | done | 3 real causes: info window counted its own early download as a duplicate; progress windows opened after completion stayed at 100%; harness clicked/filled before windows were ready. Stray windows closed between scenarios; failures screenshot every window. 5/5 full runs pass |
| 2026-10-26 | Firefox add-on | deferred | needs Firefox to test, and pairing is bound to the Chrome extension ID |
| 2026-10-26 | S22-S24 site grabber | done | engine (html5ever tokenizer, depth/folder/robots/size rules), DB migration 2, window with 4 steps; no schedule or offline link conversion. Found: a migration backup left by an earlier attempt blocked every later start (fixed) |
| 2026-10-26 | /s /q, administrator warning | done | /s e2e-tested; /q not (it exits the app) |
