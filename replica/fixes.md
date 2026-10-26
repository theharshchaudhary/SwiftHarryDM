# What IDM's users hate, miss and cannot get, and what ODM does about it

**Sample: 108 posts from 2 sources, collected 2026-10-26.**
- Hacker News: 14 comments, through the official Algolia API.
- GitHub issues: 94, through the official search API, from five open-source download
  managers (AB Download Manager, Motrix, XDM, Persepolis, Gopeed).
- None has a star rating.
- Every quote is verbatim and linked in `reviews.csv`. The ranking by the review tool is
  in `feedback.md`. The lists below were checked by hand, because the tool's keyword
  matching put some praise under complaints.

Not collected:
- Trustpilot and G2: their terms forbid automated collection, and reading them by hand
  needs a person in a browser.
- Reddit: its API needs keys the user creates.
- IDM is not in an app store, so there are no store reviews.

So this is a small, skewed sample. Most posts are by people who already left IDM, or
want to, and are asking a free alternative for IDM's features. That is useful: it says
what IDM users will not give up. It is weak on why people stay with IDM.

## 1. What they hate about IDM

| # | problem | posts | sources | evidence |
| --- | --- | --- | --- | --- |
| 1 | It is Windows only | 6 | 2 | "Only available for Windows, never saw a proper alternative for Linux/MacOS with the same reliability and browser integration." [HN](https://news.ycombinator.com/item?id=45858270) · "I switched from Windows to Linux and these are the two IDM features I miss most." [Motrix#1788](https://github.com/agalwood/Motrix/issues/1788) |
| 2 | It costs money, so people crack it or leave | 4 | 1 (3 projects) | "The days of using cracked versions of IDM are long gone!" [Persepolis#987](https://github.com/persepolisdm/persepolis/issues/987) · "it's free and the chrome extension works great without having the same dumb problems IDM had on browsers." [ABDM#1464](https://github.com/amir1376/ab-download-manager/issues/1464). Against: "The pricing looks very straight forward to me." [HN](https://news.ycombinator.com/item?id=24168990) |
| 3 | The tray icon is useless (thin) | 1 | 1 | "Currently, IDM only shows a static icon with no progress bar. And it just shows many of those for each download individually — making it completely useless." [ABDM#877](https://github.com/amir1376/ab-download-manager/issues/877) |
| 4 | It looks dated (thin) | 1 | 1 | "Compared to IDM, its sleek and modern UI has won me over" [ABDM#1169](https://github.com/amir1376/ab-download-manager/issues/1169) |

## 2. What they miss when they leave (the bar a replacement must clear)

These are requests to *other* apps, naming IDM as the model. ODM's status is in the
right-hand column.

| # | feature | posts | sources | evidence | ODM |
| --- | --- | --- | --- | --- | --- |
| 1 | Detect and download web video | 18 | 2 | "I believe video sniffing is a crucial feature for any downloader." [ABDM#1251](https://github.com/amir1376/ab-download-manager/issues/1251) | has it |
| 2 | Speed equal to IDM's | 14 | 2 | "idm always use my full bandwidth but AB doesn't" [ABDM#809](https://github.com/amir1376/ab-download-manager/issues/809) | segmented, dynamic splitting; benchmark exists |
| 3 | Take over browser downloads and start the app when needed | 11 | 2 | "When the program is not open and we want to download a file from the browser, the program does not open automatically" [ABDM#1270](https://github.com/amir1376/ab-download-manager/issues/1270) | has it (native messaging starts it) |
| 4 | Refresh an expired link: open the page it came from | 6 | 1 (4 projects) | "If resume fails, IDM opens the originating page so you can re-request the file." [Motrix#1788](https://github.com/agalwood/Motrix/issues/1788) · "Please Add Automatic opening of expired link , like idm" [XDM#190](https://github.com/subhra74/xdm/issues/190) | **partial**: you can paste a new address, but ODM cannot open the page, or catch the new link for you |
| 5 | Use the system proxy | 7 | 1 (4 projects) | "Can we add an option to use system proxy in proxy settings like IDM?" [ABDM#278](https://github.com/amir1376/ab-download-manager/issues/278) | has it (now with PAC) |
| 6 | Start downloading before you click Start | 3 | 1 | "in IDM, before clicking the "Start Download" button, IDM has already started downloading files." [ABDM#297](https://github.com/amir1376/ab-download-manager/issues/297) | has it |
| 7 | Categories and remembered folders | 7 | 1 | "the option to Remember the selected path for a category as IDM does" [ABDM#858](https://github.com/amir1376/ab-download-manager/issues/858) | has it |
| 8 | Batch, grabber | 5 | 2 | "maybe you could add a feature like the Grabber in IDM." [ABDM#819](https://github.com/amir1376/ab-download-manager/issues/819) · "While IDM doesn't seem to be able to crawl like this" [HN](https://news.ycombinator.com/item?id=16865887) | has both |
| 9 | Low memory use (thin) | 3 | 1 | "IDM uses 6-7 MB of RAM while downloading. This app uses 250+ MB of RAM." [ABDM#645](https://github.com/amir1376/ab-download-manager/issues/645) | **no**: about 207 MB with the window open (12 MB is the app, the rest is WebView2); measured 2026-10-26 on the debug build |
| 10 | Sounds for finished and failed downloads (thin) | 2 | 1 | "having audio feedback—especially for failed downloads—allows users to quickly notice when an action is needed" [XDM#1377](https://github.com/subhra74/xdm/issues/1377) | **partial**: one beep when a download finishes |
| 11 | Antivirus check, shutdown after the queue, per-download limit, Alt+click | 1–2 each (thin) | 1 | [ABDM#1478](https://github.com/amir1376/ab-download-manager/issues/1478), [ABDM#382](https://github.com/amir1376/ab-download-manager/issues/382), [ABDM#449](https://github.com/amir1376/ab-download-manager/issues/449), [ABDM#757](https://github.com/amir1376/ab-download-manager/issues/757) | has all |
| 12 | Firefox (thin) | 2 | 1 | "I can't find ABDM in link context menu on Firefox. I need this option like IDM." [ABDM#906](https://github.com/amir1376/ab-download-manager/issues/906) | **no** |

## 3. What is unsolved (positioning)

1. **A free download manager that does not make you give anything up.** People leave IDM
   for free apps, then file issue after issue asking for what IDM had (list 2: dozens of
   posts across five projects). "it was the only FOSS download manager that could compete
   with IDM and FDM" [XDM#1218](https://github.com/subhra74/xdm/issues/1218), "the best
   open source application to replace IDM" [ABDM#186](https://github.com/amir1376/ab-download-manager/issues/186).
   No free app covers the whole list today; ODM covers almost all of it.
2. **IDM's workflow on Linux and macOS** (6 posts, 2 sources). ODM is Windows only too,
   so this is unsolved by ODM as well.

## Fix plan

Ranked by evidence times how cheap it is. All but the last are `/replica-build` work.

| # | fix | size | evidence |
| --- | --- | --- | --- |
| 1 | **Expired links:** a failed download offers "Open the page it came from". When the browser then sends the same file again, ODM offers to give the failed download the new address and resume it | M | miss #4: 6 posts, 4 projects |
| 2 | **Memory:** when the main window is closed to the tray, release its WebView, and keep the same for other windows. Target: under 30 MB while sitting in the tray | M | miss #9 (thin), plus our own measurement |
| 3 | **Tray icon shows progress:** one icon, the overall percentage drawn on it, the tooltip with the count and speed | S | hate #3 (thin) |
| 4 | **Sounds:** a sound each for finished, failed and queue done, chosen or switched off in Options | S | miss #10 (thin); also an IDM feature (`features.csv`) |
| 5 | **Speed through a proxy:** add the proxy path to the benchmark, so ODM never ends up at ABDM's "500kb/s by using a manual proxy" | S | miss #2: [ABDM#399](https://github.com/amir1376/ab-download-manager/issues/399) |
| 6 | **Firefox add-on**, with pairing that does not rely on a fixed extension id | L | miss #12 (thin), parity gap |
| 7 | **Linux build** (later): Tauri runs there, but DPAPI, WinHTTP, Job Objects, the registry and native-messaging paths need Linux versions | L | unsolved #2 |

Price is not a fix to build: ODM is free (`/replica-launch` says so plainly).

## The angle

```
A. For people who left IDM for a free download manager and keep missing what IDM did,
   ODM is free and open source and does it all: video, take-over, grabber, queues.
   Evidence: list 2 (dozens of requests across 5 projects) + hate #2 (4 posts).

B. For people who use a cracked IDM because they will not pay a yearly licence,
   ODM is the same workflow, free, with no crack, no fake serial and no nag.
   Evidence: hate #2 (4 posts, 1 source; thin, and HN partly disagrees).

C. For people who want a download manager that stays out of the way,
   ODM sits in the tray, starts itself when the browser needs it and shows progress
   where you look.
   Evidence: miss #3 (11 posts, 2 sources) + hate #3 (thin). Needs fixes 2 and 3 first.
```

**Recommended: A.** It has the most evidence (the request lists are long and come from
five projects), it matches what ODM already does, and it does not need ODM to name IDM
in its own name or ads. B is the loudest motive, but its evidence is thin and it leans
on talk of cracks, which is a bad place to build a brand. C becomes true once fixes 2 and
3 ship.
