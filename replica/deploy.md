# Deploy: Keepfetch

Keepfetch has no server: deploying means (1) the landing page on GitHub Pages, and (2) a
signed release on GitHub, which installed copies update to by themselves. No database,
Stripe, OAuth or email to set up (`backend.md`).

## Preflight (2026-10-27)

| check | result |
| --- | --- |
| core and shell tests (`cargo test -p swiftdm-core -p keepfetch`) | **140 passed, 0 failed** |
| end-to-end, real app + extension in Chrome (`node tests/e2e/run.mjs`) | **12 of 12 scenarios passed**, including axe-core WCAG 2.1 AA scans of every window |
| CI on the last pushes | green on `6de1975`; earlier runs failed intermittently on GitHub's slower runners (timing; causes fixed in `a4504e5`, `f1ee370`) |
| feature parity (`parity.py`) | **96.0**, must-haves **39 of 39** |
| open bugs | S1 0, S2 0 (`bugs.md`) |
| rebrand sweep (`sweep.py` on every tracked folder) | **clean** (11 of 11) |
| old name left (`git grep -i swiftharrydm`) | only the kept identifiers and the migration code (`brand.md`) |
| store listing (Chrome Web Store) | `listing.py` cannot read it (Apple/Google only); checked by hand: within limits (20/75, 90/132, 976/16000), no avoided names, no claims, no emoji |
| production build (`tauri build --bundles nsis`, updater signing off locally) | **passes**: `Keepfetch_3.0.0_x64-setup.exe`, 7.0 MB; the installer hooks compile |
| memory, release build | 10 MB in the tray, 231 MB with the main window open |
| privacy page | `docs/privacy.html`, lists every outside service |
| terms, cookie banner, account deletion | n/a: no accounts, no cookies on the site, no service |
| favicon, titles, OG image | Keepfetch icon, "Keepfetch — free download manager for Windows", the main window screenshot |

**Not checked, needs a person:** the upgrade from an installed SwiftHarryDM to Keepfetch
on a spare Windows account (`launch-plan.md` step 3).

## 1. Landing page: GitHub Pages

Source: branch `main`, folder `/docs` (`index.html`, `privacy.html`, `images/`). The
Markdown files in `docs/` are served too, rendered as pages.

- URL until the repository is renamed:
  `https://theharshchaudhary.github.io/SwiftHarryDM/`
- After the rename to `keepfetch`: `https://theharshchaudhary.github.io/keepfetch/`
  (GitHub does not redirect Pages URLs after a rename).
- Enable: Settings › Pages › Deploy from a branch › `main` / `docs`, or
  `gh api -X POST repos/<owner>/<repo>/pages -f "source[branch]=main" -f "source[path]=/docs"`.

### With your own domain (after you buy it)

For `keepfetch.app` (or `.com`) at any registrar:

| record | name | value |
| --- | --- | --- |
| A | @ | 185.199.108.153 |
| A | @ | 185.199.109.153 |
| A | @ | 185.199.110.153 |
| A | @ | 185.199.111.153 |
| CNAME | www | `theharshchaudhary.github.io` |
| TXT | `_github-pages-challenge-theharshchaudhary` | the value GitHub shows under Settings › Pages › Verified domains |

Then:
- Settings › Pages › Custom domain `keepfetch.app`, and tick **Enforce HTTPS** once the
  certificate is issued.
- Apex canonical, www redirects (GitHub does that redirect itself).
- Add `docs/CNAME` containing `keepfetch.app`.
- `.app` domains are HTTPS-only (HSTS preloaded), which Pages supports.
- No email is sent from the domain, so no SPF/DKIM/DMARC are needed. If you add a contact
  address, set `v=spf1 -all` and `v=DMARC1; p=reject` on it, so nobody can send mail in
  its name.

## 2. Release: v3.1.0

1. **Add the repository secrets** `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (Settings › Secrets and variables › Actions). The
   key must be the one whose public half is in `tauri.conf.json`, or installed copies will
   refuse the update. **None are set today**, so a tag would fail at signing.
2. Bump the version to `3.1.0` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and
   `extension/manifest.json`.
3. `git tag v3.1.0 && git push origin v3.1.0`. `release.yml` builds FFmpeg and the
   installer, signs them and publishes `latest.json`.
4. Check the release page: `Keepfetch_3.1.0_x64-setup.exe`, its `.sig` and `latest.json`.
5. Install it on a clean Windows user, then do the core flow: click a link in Chrome,
   the info window opens, Start, the complete window shows, and the file opens.

The installer is not code-signed (no Authenticode certificate), so Windows SmartScreen
will warn on first downloads ("Windows protected your PC"). The warning goes away with
reputation, or at once with a code-signing certificate (yours to buy, about $200–400 a
year; or a free open-source one from SignPath). The landing page does not claim
otherwise.

## 3. Watch the first week

No telemetry, by design. Watch:
- **downloads:** `gh release view v3.1.0 --json assets --jq '.assets[] | "\(.name) \(.downloadCount)"'`;
- **issues:** label `bug` versus `request`, and reply within a day;
- **update errors:** an issue saying "update failed" usually means the signing key
  differs. Do not delete `latest.json`; publish 3.1.1;
- **SmartScreen:** reports of the warning, to decide on a certificate;
- **the landing page:** uptime is GitHub's. Check it after every push to `docs/`.

## Status

| step | state |
| --- | --- |
| preflight | done, all passing |
| Pages enabled | waiting for your go |
| repository renamed | yours to do (Settings › General) |
| domain | yours to buy (optional) |
| signing secrets | yours to add |
| v3.1.0 tagged and released | after the secrets |
