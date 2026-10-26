# Parity: ODM against IDM 6.43 (2026-10-26)

| | score |
| --- | --- |
| features | **95.4 / 100** (87 features counted, must-haves 39 of 39) |
| layout | **not measured**: no IDM reference screenshots in `replica/screens/` |
| open bugs | none (S1 0, S2 0; 7 fixed this run, see `bugs.md`) |

**Verdict: shippable.** All must-haves are done, the feature score is over 80, and there
is no open S1 or S2. It is not yet "better than the original": that needs the fixes from
`/replica-entrepreneur`.

## Feature score by area

| area | score | features |
| --- | --- | --- |
| grabber | 71.4 | 5 |
| engine | 78.6 | 6 |
| browser | 84.4 | 7 |
| options | 95.5 | 10 |
| main window | 98.9 | 20 |
| video, capture, flow, system, limits, scheduler | 100 | 39 |

## Missing, in build order

1. [should] Firefox add-on. Pairing trusts the Chrome extension's fixed id; Firefox
   gives each install a random id, so pairing needs a design change. Firefox is not
   installed on the build machine to test it.
2. [should] FTP downloads.
3. [should] Windows sign-in for intranet servers (NTLM, Negotiate, Kerberos). Basic
   login works.
4. [could] Grabber: offline copies (rewrite links in saved pages).
5. [could] Grabber: run a project on a schedule.
6. [could] A sound per event (only "download finished" today).
7. [could] Floating "download selected links" button (the context menu item works).
8. [could] Languages other than English.

## Layout

Not scored. To score it, take screenshots of your own IDM at the same window sizes as
`replica/clone-screens/` (main window with a few downloads, Download File Info, progress,
Options > Connection, Scheduler, Grabber), save them in `replica/screens/` with the same
S-ID names, and run:

```bash
python imgdiff.py replica/screens/S01-main.png replica/clone-screens/S01-main.png --json > replica/diffs/S01.json
python parity.py replica/features.csv --visual replica/diffs/*.json --markdown
```

The layout score should not reach 100 on purpose: the trade dress changes at
`/replica-brand`.

## Behaviour differences

From IDM's help pages (recon sources 4–12) against ODM as tested.

| flow | IDM does | ODM does | fix or keep |
| --- | --- | --- | --- |
| F01 core loop | 2 clicks (link, Start); downloads while the info window is open | same 2 clicks, same early start | keep |
| F01 duplicate address | asks: numbered / overwrite / resume | same three choices, "remember" option | keep |
| F01 web page with Insert | sends the link to IDM | same (Insert by default, other keys possible) | keep |
| F02 video | panel over players; TS to MP4 a recurring bug in its changelog | button over players with real format list; any stream ends as MP4 (verified with HLS) | keep |
| F02 short clips | panel appears on every player above a minimum file size | minimum *length* in seconds (a page knows a player's length, not its size) | keep, documented |
| F04 failed file in a queue | moves to the end, retried every 30 s | same, plus a per-queue retry count | keep |
| F04 end of queue | turn off, hang up modem, force close, open a file | exit, sleep, hibernate, shut down, with a cancellable countdown; no "open a file" | could add "open a file" |
| F05 sync queue | periodic re-check, "start again every N hours" | re-check on every start; schedule is daily, no "every N hours" | add "every N hours" |
| F08 grabber | wizard, projects, templates, schedule, offline link conversion | wizard, projects, templates; no schedule, no link conversion | build later (could) |
| F08 politeness | not documented | robots.txt by default, 2 pages at a time, 250 ms pause | keep (better) |
| F10 proxy | none / system / PAC / manual per protocol, browser's proxy on errors | the same, chosen per address; yt-dlp uses it too; no "browser's proxy on errors" | keep |
| F11 move from IDM | — | imports IDM export files | keep (better) |
| everything | 30-day trial, then $11.95 a year or $24.95 lifetime per PC | free, no trial, no nag | keep (the point) |
| accessibility | not documented | every window passes an axe-core WCAG 2.1 AA scan in each test run | keep (better) |

## Top five to build next

1. "Every N hours" for synchronization queues and "open a file" when a queue finishes
   (small, closes two behaviour gaps).
2. Windows sign-in (NTLM/Negotiate) for company intranets.
3. FTP.
4. Firefox add-on, starting with a pairing design that does not depend on a fixed
   extension id.
5. The fixes `/replica-entrepreneur` finds in IDM's reviews.

---

Generated part (`parity.py --markdown`):

## Parity: 95.4 / 100

features 95.4  (87 counted, must-haves 39 of 39 done)

## By area, weakest first
- grabber                       71.4  (5 features)
- engine                        78.6  (6 features)
- browser                       84.4  (7 features)
- options                       95.5  (10 features)
- main window                   98.9  (20 features)
- video                        100.0  (6 features)
- capture                      100.0  (5 features)
- flow                         100.0  (14 features)
- system                       100.0  (6 features)
- limits                       100.0  (1 features)
- scheduler                    100.0  (7 features)

## Missing, in build order
- [should] browser: Firefox add-on, no  (deferred: Firefox not installed here to test; the API trusts the Chrome extension ID and Firefox IDs are random per install, so pairing needs a design change)
- [should] engine: FTP downloads, no  (IDM takes over FTP; PASV option)
- [should] engine: Basic NTLM Negotiate Kerberos auth, partial  (Basic done via login prompt; Windows auth missing)
- [could] grabber: Grabber convert links for offline browsing, no  (not built: 'whole site' saves pages as they are)
- [could] grabber: Grabber schedule, no  (not built: projects are explored by hand)
- [could] options: Sounds per event, no
- [could] browser: Download panel for selected links, partial  (context menu 'Download selected links' works; no floating button)
- [could] main window: Language selection, partial

## Left out on purpose (not scored)
- Registration and trial: ODM is free and open source
- Tell a friend: no service behind it
- Dial-up and VPN tab: no dial-up on modern Windows
- IE monitor and Opera/IE plugins: those browsers are gone
- MMS protocol: dead protocol
- Toolbar skins and IDM artwork: IDM's own assets; ODM draws its own
