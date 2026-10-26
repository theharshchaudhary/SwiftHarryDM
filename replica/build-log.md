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
