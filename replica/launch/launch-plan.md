# Launch plan: Keepfetch

Order matters: each step needs the one before.

## Before anyone sees it

1. **Rename the GitHub repository** to `keepfetch` (Settings › General). GitHub redirects
   the old address, so installed copies keep finding updates. Then change the URLs in
   `tauri.conf.json` (updater), `README.md`, `docs/index.html`, `docs/privacy.html` and
   the Help menu, in one commit.
2. **Register `keepfetch.com` or `keepfetch.app`** if you want a domain (both
   unregistered on 2026-10-26). Run the trademark searches in `replica/brand.md` first.
3. **Upgrade test on a spare Windows account or VM** (not done yet): install the last
   SwiftHarryDM release, add a few downloads and pair a browser, then install Keepfetch.
   Expected: the old program is removed, the list, settings and pairing are still there,
   and it starts with Windows if it did before.
4. **Error reports without telemetry:** there is no crash reporting, by design. Make the
   issue template ask for the log file (`%LOCALAPPDATA%\Keepfetch\logs`).

## Release day

5. **Release v3.1.0**: bump the version in `tauri.conf.json`, `Cargo.toml` and
   `extension/manifest.json`, tag `v3.1.0`. `.github/workflows/release.yml` builds and
   signs the installer and `latest.json`. Existing SwiftHarryDM installs then update to
   Keepfetch by themselves.
6. **Landing page live** (`/replica-deploy`: GitHub Pages from `docs/`).
7. **Extension in the stores**: Chrome Web Store and Edge Add-ons, with
   `replica/launch/listing.json`, screenshots from `docs/images/`, the privacy page URL,
   and "single purpose: send downloads to the Keepfetch desktop app". Then put the store ids
   in `browser.rs` (`CHROME_STORE_ID`, `EDGE_STORE_ID`), so the app offers the store version.

## Where the people are (from the research, `fixes.md`)

People asking free download managers for the features that keep them on paid ones:
- the issue trackers of AB Download Manager, Motrix, XDM, Persepolis and Gopeed (do not
  advertise in other projects' issues; answer only where someone asks for exactly what
  Keepfetch does, and say who you are);
- Hacker News threads about download managers (Show HN below);
- Windows and piracy-alternative communities that ask "free download manager like ...".

## The posts

- **Show HN:** "Show HN: Keepfetch – a free, open-source Windows download manager with
  video capture and a site grabber". Lead with the fixes, not the parity: expired links
  continue, ~15 MB in the tray, the grabber respects robots.txt. Link the source and the
  test suite (WCAG scans and SHA-checked downloads are good HN material).
- **Product Hunt** (optional): same lead, the main window screenshot, "free, MIT".
- **Never** name the original in titles, ads or the listing. In conversation, "an
  alternative to paid download managers" is enough.

## The first 10 users

Talk to them by hand, before the posts:
- 5 people you know who pay for, or crack, a download manager on Windows;
- 5 people who filed the IDM-feature requests in the trackers above and are still active
  (ask what is missing, not whether they like it).

Ask each: what did you try to download this week, what did Keepfetch get wrong, what made
you go back to your old one. Write the answers down as rows in a new `replica/beta.csv`
with the same columns as `reviews.csv`, and rerun `/replica-entrepreneur` on them.

## Measure (without telemetry)

- GitHub release download counts (`gh release view --json assets`);
- extension installs in the store dashboards;
- issues opened, and how many are bugs versus requests.
