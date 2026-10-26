# Brand: Keepfetch

Angle (from `fixes.md`, option A): for people who left IDM for a free download manager
and keep missing what it did, Keepfetch is free and open source and does it all.

## Name

The user first chose **ODM (Open Download Manager)**. The screening on 2026-10-26 found
that exact name already in use in the same category:
- [ODM — Open Download Manager](https://alternativeto.net/software/odm--open-download-manager/about/)
  ([github.com/muzammilijaz/odm](https://github.com/muzammilijaz/odm)): a free, AGPL Windows
  download manager built with Rust, Tauri and React, with a Chrome/Edge extension. Created
  2026-08-31, described as "Best IDM alternative".
- [Open Download Manager (ODLM)](https://sourceforge.net/p/odlm): an older GPLv2 project on
  SourceForge.
- 19 GitHub repositories match "open download manager".
- `opendownloadmanager.com` (2013), `.org` (2026-09-08), `odm.app` and `getodm.com` are all
  registered.

The user then picked **Keepfetch**: "keep" what you "fetch". It says what the app does
(gets files and keeps the list, the queue, the copy in sync) and does not echo IDM's name.

| check | Keepfetch | Grabwell | Downhaul |
| --- | --- | --- | --- |
| GitHub repos with the name (2026-10-26) | 0 | 0 | 2 |
| web search "name + download software" (2026-10-26) | no product found | no product found | to run |
| `.com` (RDAP, 2026-10-26) | not registered | registered | registered |
| `.app` (RDAP, 2026-10-26) | not registered | not registered | not registered |
| US trademark (tmsearch.uspto.gov, classes 9 and 42) | **to run** | to run | to run |
| EU trademark (TMview) | **to run** | to run | to run |
| WIPO Global Brand Database | **to run** | to run | to run |
| Microsoft Store, Chrome Web Store, Edge Add-ons | **to run** | to run | to run |
| handles (GitHub org, X) | **to run** | to run | to run |

These are screening checks, not legal clearance. Register `keepfetch.com` / `.app` soon
if you want them (unregistered names can be taken at any time), and have a trademark
lawyer search before spending money on the name.

## What was renamed, and what was kept on purpose

Renamed everywhere a person sees it:
- window titles, menus, tray, notifications, the installer and Start menu entry;
- the executable (`keepfetch.exe`) and the startup entry ("Keepfetch");
- the User-Agent (`Keepfetch/3`), the browser extension's name and texts;
- README, help, notices;
- the data folder (`%LOCALAPPDATA%\Keepfetch`).

Kept, because existing installs and browsers depend on them (nobody sees them):

| identifier | why it stays |
| --- | --- |
| `np.com.harshchaudhary.swiftharrydm` (bundle id, native messaging host) | the extension finds the app by it; changing it breaks every connected browser |
| `SwiftHarryDM/secrets/v1` (DPAPI entropy) | changing it makes saved cookies and passwords unreadable |
| `github.com/theharshchaudhary/SwiftHarryDM` | the repository; the update check uses it. Renaming the repository is the owner's step (GitHub redirects the old address, so updates keep working) |
| crate names `swiftdm-*`, `SWIFTDM_HOME`, `.sdmpart`, `sdm-observe` | internal; the temp suffix is on disk in unfinished downloads |
| the extension's key (its id) | the same extension keeps working |

Moving from SwiftHarryDM, done automatically:
- **On first start**, Keepfetch copies the download list (a consistent SQLite copy, safe
  even if the old app is still running), settings, browser pairings, window positions and
  updated tools from `%LOCALAPPDATA%\SwiftHarryDM`. It copies only once and never deletes
  the old folder. Test: `appdata::tests::migrates_the_previous_folder_once`.
- **The installer** carries over the "start with Windows" choice, closes and silently
  uninstalls the old SwiftHarryDM program (its uninstaller removes only its own files), and
  removes the old startup entry. *Not run yet on a real machine:* it needs an installer
  build and an old install to upgrade (see `test-plan.md`, manual pass).
- The extension accepts both names from the app during the change.

## Palette

The primary hue moves from SwiftHarryDM's blue (and away from IDM's blue and green) to
**violet**. Roles are unchanged; values are in `replica/design/tokens.json` and
`tokens.dark.json`, live in `ui/src/styles.css` and the extension.

| role | light | dark |
| --- | --- | --- |
| accent | `#6D28D9` | `#A78BFA` |
| accent-hover | `#7C3AED` | `#C4B5FD` |
| on-accent | `#FFFFFF` | `#170B33` |
| accent-soft | `#EDE9FE` | `#2E2350` |
| selection / on-selection | `#E4DCFB` / `#1E1033` | `#3B2D6B` / `#FFFFFF` |
| progress bar / track | `#7C3AED` / `#E6E1F2` | `#A78BFA` / `#363044` |

`contrast.py`: 21 pairs per theme, 0 below AA (2026-10-26). The neutrals, the status
colours and the secondary amber are unchanged.

## Logo brief

- **Idea:** an arrow dropping into a pocket. You fetch it, and it is kept.
- **Mark:** a symbol (white arrow over an open-topped rounded pocket) on a violet rounded
  square, plus a plain wordmark "Keepfetch" in the UI font. No monogram.
- **Sizes:** must read at 16 px (tray, favicon) and as a 1024 px app icon.
- **Deliverables:** SVG, a 1024×1024 icon with no transparency, an `.ico` with
  16/32/48/256, PNGs 16/48/128 for the extension, a 1200×630 social image.
- **Must not resemble IDM's mark:** no globe, no green arrow, and no blue-and-green pair.
  Check the drafts side by side with IDM's icon.
- **Working version, drawn here:** `packaging/brand/` (`keepfetch-1024.png`,
  `-1024-opaque.png`, `16`–`256`, `preview.png`). It is now the app and extension icon. It
  is a generic "download to tray" glyph; a designer should give it something of its own,
  for example a pocket shape with a fold.

## Voice

**Plain, calm, exact.**
- *plain*, not childish: everyday words, but not baby talk;
- *calm*, not cold: failures are explained, never dramatised;
- *exact*, not technical: say what happened and what to do, without protocol names.

| do | don't |
| --- | --- |
| "The server stopped answering. Keepfetch will try again in 30 seconds." | "ERR_CONNECTION_RESET (-101)" |
| "Start download" | "Commence transfer" |
| "This video is protected (DRM) and can't be downloaded." | "Unsupported content" |
| "Nothing here yet." | "Oops! Looks like it's empty 😢" |
| name the next step: "Check it before opening the file." | leave the user guessing what to do |

The ten most-seen strings, checked against the voice. All are written fresh, none echoes
IDM's phrasing, and only one was changed:
1. Empty list: **changed** to "Nothing here yet. Click Add URL (Ctrl+N) or drop a link on
   this window. Downloads you start in your browser land here too."
2. Main button "Start download" — kept.
3. "Download later" — kept.
4. File info banner: "Sent from your browser. Check the details and choose how to download it." — kept.
5. Complete window: "Open", "Open folder", "Don't show this again" — kept.
6. Failure notice "Download failed" + the reason — kept.
7. Duplicate prompt: "This address is already in your list" — kept.
8. Login: "<site> asks for a user name and password." — kept.
9. Player button: "Download this video" — kept.
10. Queue done: "All downloads in “<queue>” are done." — kept.

## Sweep

`python sweep.py <each tracked folder> --config replica/brand.json` checks IDM's name, the
acronym, `IDMan` and its domain. It is clean on 2026-10-26.
- The `target/` build folder is left out: its random file names happen to contain "idm".
- The company name is not in the list: as a substring it matches ordinary identifiers.
- Import and export say "`.ef2` export file", without IDM's name.
- The old placeholder name is checked separately:
  `git grep -i swiftharrydm` shows only the kept identifiers listed above.
