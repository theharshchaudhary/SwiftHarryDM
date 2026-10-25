# Build log

| date | screen / feature | status | notes |
| --- | --- | --- | --- |
| 2026-10-25 | shell + vertical slice (S01–S05, F01) | done | already built before the Replica run |
| 2026-10-25 | audit of open must-haves | done | the IDM-PARITY checklist was stale: menus, File types, Save to, schedule times, countdown, DRM refusal were already built |
| 2026-10-25 | S10 category sites filter | done | core `categorize` takes the host; site categories win; Options > Save to edits them |
| 2026-10-25 | S25 player download button rules | done | `/api/settings/player`; mini, min length, skip sites, × per player. e2e real_chrome_extension passes. Harder than expected: none, but the e2e suite was red on CI before this run (flaky timing; passes 2/2 locally) — for /replica-test |
| 2026-10-25 | TS to MP4 | done | verified with a trimmed public HLS stream through swiftdm-cli: MP4, h264+aac, full length |
