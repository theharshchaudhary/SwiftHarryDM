# IDM parity checklist

Built from IDM 6.43 itself: its menus, dialogs and option labels (resources of `IDMan.exe`)
and its help files (`idman.chm`, `grabber.chm`). Each item is a behavior of IDM that
SwiftHarryDM should have, written and drawn by us (no IDM text, code or images are copied).

`[x]` done and tested · `[ ]` to do · `n/a` not applicable today (reason given)

## 1. Download flow

- [x] Download File Info: the download starts while the window is shown (IDM default), with an option to turn this off
- [x] Download File Info: "Remember this path for <category>" checkbox
- [x] Download File Info: Zip preview (list the archive's files before downloading, read by ranges)
- [x] Download File Info: "Download later" offers the queue choice (option: show queue selection panel)
- [x] Download File Info: opens in front of the browser and takes the keyboard
- [x] Duplicate link dialog: numbered name / overwrite / show complete or resume; remember choice
- [x] Twice-cancelled automatic download: offer to add the site or the address to exceptions
- [x] Download complete: Open, Open with, Open folder, Close, "Don't show again", drag the file out
- [x] Download progress: tabs Download status / Speed limiter / Options on completion, hide-tab buttons, hide details
- [x] Download progress: per-file speed limit remembered across stop/resume
- [x] Download progress: options on completion (show complete dialog, exit, turn off computer, force processes to close)
- [x] Delete confirmation: "also delete downloaded files from disk", "Don't show again"; separate dialog for unfinished downloads
- [ ] "IDM could not take over the download" notice with "Don't show again"
- [ ] Authentication dialog when a server asks for a login, with "Save this password"

## 2. Main window

- [ ] Menus with IDM's structure: Tasks, File, Downloads, View, Help (items listed in the menu resource)
- [ ] Tasks: Export / Import to and from the IDM export file (`.ef2`) and text file; export dialog (queue / selected / all)
- [ ] File: Stop download, Remove, Download now, Redownload
- [ ] Downloads: Pause all, Stop all, Delete all completed, Find, Find next, Scheduler, Start/Stop queue submenus, Speed limiter submenu, Options
- [ ] View: Hide categories; Arrange files by order of addition, name, size, status, time left, rate, last try, description, save path, referer, parent web page
- [ ] View: Toolbar customize (which buttons, order), large/small buttons
- [ ] View: tray icon style (3D / classic / don't show), customize URL list (columns dialog with widths), dark mode, font choose/reset, language
- [ ] Help: help contents, tip of the day, home page, check for updates, about
- [ ] Columns dialog: show/hide, move up/down, width in pixels, reset
- [ ] Find dialog: in name / description / site, link, parent page, referer; match case; whole string
- [ ] Find bar at the bottom of the list
- [ ] Q column with queue icons (main download queue, synchronization queue, other queues; clock when scheduled)
- [ ] Drag downloads onto a queue in the categories tree to move them
- [ ] Categories tree context menu: add, edit, delete, properties; queue context menu: start, stop, edit
- [ ] Category dialog: file types, "only files from these sites", folder, remember last save path
- [ ] Toolbar: Start queue / Stop queue drop-down to pick a queue
- [ ] Tip of the day on startup (can be turned off)
- [ ] Problem report / tell a friend: n/a (no service behind them)

## 3. Options

- [ ] General: start on startup; clipboard monitoring; capture from browsers list; keys dialog (prevent: Alt/Shift/Ctrl; force: Alt/Shift/Ctrl/Ins/Del; left click check; ignore pages/images while holding keys)
- [ ] General: customize context menu items (Download with, Download all links, video items)
- [ ] General: download panel on web players: file types list, minimum size, exceptions (sites), full/mini mode, "don't capture from web players automatically", "show panel for protected content"
- [ ] General: panel for selected links (any / none / these sites only; full/mini)
- [ ] File types: list with Default button; sites not to take over (with Default); address exceptions list editor with "show dialog for twice-cancelled downloads"
- [ ] Connection: default max connections; per-server exceptions table (new/edit/delete); download limits with "show warning before stopping"
- [ ] Save to: category table, "General" types and folder, change folder on last selected, set file date from the server, temporary directory
- [ ] Downloads: customize progress dialog (show speed limiter tab, options tab, hide-tab buttons, start view normal/minimized/don't show); show start dialog; only add to queue; show complete dialog; start immediately while showing info; queue panel on Later; queue panel after batch; ignore modification time changes; duplicate link action; User-Agent for manual downloads; virus checking program and parameters
- [ ] Proxy: none / system / automatic configuration script / manual; per protocol HTTP, HTTPS, FTP; SOCKS 4/5 with proxy DNS; get system; bypass list; use the browser's proxy on errors
- [x] Site logins: list with new / edit / remove (server and path, user, password)
- [ ] Sounds: per-event sounds (download complete, failed, queue done, ...) with browse and play
- n/a Dial-up / VPN tab and "hang up modem when done": Windows dial-up modems are gone; VPN connections are managed by Windows
- n/a "Run module for IE-based browsers", "Add browser" for IE, Opera plugins: those browsers are gone; Chrome, Edge, Brave (and Firefox, see 5) are covered
- n/a Use TLS 1.3 from Microsoft: our HTTP stack (rustls) supports TLS 1.3 already

## 4. Scheduler and queues

- [ ] Main download queue and main synchronization queue, plus any number of added queues
- [ ] Schedule: one-time downloading or periodic synchronization; start at time, once or daily with days; start again every N hours/min; stop at time
- [ ] Number of retries for each file; start download on IDM startup
- [ ] When done: open a file, exit, turn off computer, force processes to terminate; shut-down countdown dialog with "Shut down now" and Cancel
- [ ] Files in the queue tab: files at the same time, list with move up/down, remove from queue
- [ ] "Keep IDM running in the tray for the schedule" notice
- [ ] Synchronization: re-check finished files and download them again if they changed on the server

## 5. Browser integration

- [x] Chrome, Edge, Brave extension that takes over downloads by type, size and exceptions
- [x] Native messaging host registered on install (the extension finds the app)
- [ ] Firefox add-on (IDM has one)
- [ ] Download panel on players: full and mini modes, file-type and size rules, exceptions
- [ ] Download panel for selected text with links
- [ ] Context menu items configurable from the app
- [ ] Keys to force or prevent take-over honored by the extension
- [ ] Store-hosted extension offered automatically after install (needs store IDs)

## 6. Site grabber

- [ ] Wizard step 1: project name, start page, template (all images, all audio, all video, all files, complete site, custom), login, manual login page, logout pages
- [ ] Step 2: save to by category / one category / one folder; original relative subfolders; convert links for offline browsing; overwrite; add checked files to the main list on close
- [ ] Step 3: explore the whole site or N levels (this site / other sites); ignore popups; don't explore parent directories; explore the main domain; process JavaScript; include/exclude paths
- [ ] Step 4: file type filters (include / exclude, edit filters), this site only, hide duplicates, start downloading at once, size limits, include/exclude paths
- [ ] Main grabber window: site tree, file list, explore / stop, check / uncheck all, download / stop checked, add checked to the main list, statistics window
- [ ] Projects: save, load, recent, templates from current settings; listed under "Grabber projects" in the categories tree
- [ ] Project schedule: one-time or periodic synchronization; explore only / explore and download / download checked; when done actions
- [ ] Grabber settings: files to explore / download at the same time; link texts as descriptions

## 7. Command line and system

- [x] `/d URL`, `/p`, `/f`, `/n`, `/a`
- [ ] `/s` start the queue, `/q` exit after the download, `/h` (n/a hang-up)
- [ ] Shell: "Download with SwiftHarryDM" for links dragged onto the tray / taskbar
- [ ] Single tray menu like IDM's (show, add URL, start/stop queue, speed limiter, exit)
- [ ] Warning when running as administrator (browsers cannot reach an elevated app the same way)
