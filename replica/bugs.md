# Bugs

Only bugs reproduced in this run (2026-10-25/26). All have a test that failed before the
fix and passes after it, unless noted.

Open: S1 0, S2 0, S3 0, S4 0. Fixed: S1 1, S2 2, S3 4.

### BUG-001: the app cannot start after an interrupted database upgrade

- Severity: S1 (core flow blocked: the app does not open)
- Flow / case: X-UPG
- Build: before ae5e7ae

Steps
1. A database at version N is upgraded; the backup `state3.db.bak-vN` is written.
2. The upgrade does not complete (the transaction rolls back, or the file is restored).
3. Start the app again.

Expected: it backs up again and upgrades.
Actual: "output file already exists" from `VACUUM INTO`; the app refuses to start, every time.
Evidence: `store::tests::upgrades_a_version_1_database`
Status: fixed in ae5e7ae (each backup takes the next free name)

### BUG-002: the info window shows the duplicate-link prompt for its own download

- Severity: S2 (the browser download is blocked behind a prompt that makes no sense)
- Flow / case: F01-H2, F01-E7
- Screen: S03, S15

Steps
1. "Start downloading while the info window is open" is on (the default).
2. Send a new address from the browser.
3. While the window is open, the video components' status finishes loading (or a site
   login is saved).

Expected: no prompt; the address was not in the list.
Actual: "This address is already in your list: cut.bin (0.00%)", the window's own early
download. Depends on timing; it made the e2e suite fail on CI.
Evidence: e2e `extension_protocol` screenshot (replica run, 2026-10-26)
Suspected cause: the duplicate check ran again whenever the address check re-ran.
Status: fixed in a4504e5 (checked once per address)

### BUG-003: video downloads ignore the proxy setting

- Severity: S2 (behind a proxy, video downloads fail or bypass the company proxy)
- Flow / case: F02-E4
- Screen: S07 Proxy, S25

Steps
1. Options > Proxy: a manual proxy.
2. Download a video.

Expected: yt-dlp connects through the proxy.
Actual: yt-dlp was never given a proxy (it used its own environment).
Evidence: `ytdlp_gets_the_proxy_from_options` (fake yt-dlp requires `--proxy`)
Status: fixed in bfdf274

### BUG-004: a progress window stays open at 100%

- Severity: S3
- Flow / case: F01-H1
- Screen: S04

Steps
1. Start a small download from the UI (the progress window is set to open).
2. The download finishes before the window has finished opening.

Expected: the window closes (the complete window takes over).
Actual: "100% secret.bin" stays open until closed by hand.
Evidence: e2e failure screenshots, `login_when_asked` followed by `extension_protocol`
Status: fixed in a4504e5 (the window closes itself when its download ends)

### BUG-005: pages opened before pairing keep the old extension rules

- Severity: S3 (Insert-click and the player button rules do nothing until the page is reloaded)
- Flow / case: F01-E6, F02-E3
- Screen: browser

Steps
1. Open a page in Chrome, then pair the extension (or change Options > Browser).
2. Insert-click a link on that page.

Expected: the link is sent to the app.
Actual: the browser follows the link; the page still had the defaults.
Evidence: e2e `real_chrome_extension` (Insert+click step)
Status: fixed in 98865c2 (rules pushed to open pages; refreshed on pairing)

### BUG-006: unlabeled fields and unreachable tables for screen-reader and keyboard users

- Severity: S3
- Flow / case: X-A11Y
- Screen: S07, S11, S13, S14, S06, S04, S23

Steps
1. Scan each window with axe-core (WCAG 2.1 AA).

Expected: no serious or critical problems.
Actual: 20 fields without a name (Options, Scheduler, Batch, Links, Properties), two
selects without a name, a scrolling table not reachable by keyboard (progress), low
contrast on dimmed scheduler labels, an empty listbox (grabber).
Evidence: e2e scans; now enforced in every run
Status: fixed in 430710d

### BUG-007: status colours and input borders below WCAG contrast

- Severity: S3
- Flow / case: X-DARK
- Screen: all

Actual: green and amber status text (4.25:1, 3.46:1), input borders (1.8:1) in light
mode; white text on the light-blue dark-mode button (2.96:1).
Evidence: `contrast.py replica/design/tokens*.json`
Status: fixed in 97609e8

## Test suite problems (not product bugs, fixed)

- The e2e harness clicked and typed before a window was ready, so clicks on briefly
  disabled buttons and typing into still-loading windows were lost (a4504e5).
- Windows left open by one scenario were found by the next (a4504e5: closed between
  scenarios; failures screenshot every window).
- The links-count check read the list before each address's type was known.

## To check (not reproduced)

- `/q`: exits through the usual countdown window; IDM exits without asking. Decide which.
- Sleep at the scheduled start time: the queue starts on wake if the minute was missed?
  (scheduler ticks compare the previous and current minute; a long sleep may skip it.)
- The machine this ran on has a SwiftHarryDM startup entry pointing at
  `%TEMP%\sdm-install-test\swiftharrydm.exe` (left by an earlier install test). Not an
  app bug, but it starts a second copy at login that takes port 5001.
