# Backend: ODM

ODM is a desktop app with no server of its own: no accounts, no payments, no email, no
cloud database. The Replica backend steps that assume a web app (auth, Stripe, Resend,
OAuth) do not apply. What is ODM's "backend" is listed here.

| part | what it is | status |
| --- | --- | --- |
| database | SQLite in the user's profile (WAL, synchronous=FULL), forward-only migrations in `store.rs` | v2 adds the site grabber tables; upgrade tested from a v1 file (`upgrades_a_version_1_database`) |
| backups | a copy of the database before every migration (`state3.db.bak-vN`, numbered if one exists) | fixed 2026-10-26: an old backup used to block every later start |
| settings | JSON file, every field defaulted, so older files keep working | |
| local API | axum on 127.0.0.1:5001/5002 for the browser extension | checklist below |
| jobs | the manager actor's timers: scheduler, retries, quota, sync checks; yt-dlp update weekly | |
| updates | Tauri updater, signed; GitHub Releases host `latest.json` and the installer | |
| integrations | none with accounts. yt-dlp's public extractors; Chrome/Edge stores for the extension | |

Outside services the app contacts (for the privacy page, `/replica-launch`):
- `github.com/<owner>/<repo>/releases` (app updates; the owner changes at `/replica-brand`)
- `github.com/yt-dlp/yt-dlp/releases` (yt-dlp updates, checked against SHA2-256SUMS)
- the Chrome Web Store and Edge Add-ons (only to install the extension)
- the sites the user downloads from, and their proxy if one is set
No telemetry, no analytics, no crash reporting.

## Security checklist

- [x] secrets: none in the repo. Signing keys are ignored (`*.key`, `*.pem`, `*.pfx`,
      `*.p12`) and live in GitHub Actions secrets; the extension's manifest `key` and the
      updater key in `tauri.conf.json` are public keys
- [x] input validated on every route: URLs (`validate_url`: http/https only), format ids,
      body sizes, cookie size (16 KB), JSON shape
- [x] authorisation on every route: bearer token bound to the extension id, from native
      messaging or a 6-digit pairing code; Host header must be 127.0.0.1/localhost with the
      port (stops DNS rebinding); a web page's Origin is refused; no CORS
- [x] rate limits: read routes, downloads (60/min), pairing
- [x] no user data in logs: cookies are never logged and stored DPAPI-encrypted, as are
      site-login and proxy passwords
- [x] new route `/api/settings/extension` goes through the same guard and token check
      (tested: 401 without a token)
- [x] dependencies: `npm audit` 2026-10-26 had 5 findings, all in dev tools. vite updated
      to 5.4.21, unused vitest removed. Left: esbuild/vite dev-server advisories
      (GHSA-67mh-4wv8-2f99 and one more), which only matter while running `npm run dev`;
      the fix is vite 6+, a major upgrade
- [ ] Rust dependencies not audited: `cargo-audit` is not installed here
      (`cargo install cargo-audit`, then `cargo audit`; or add it to CI)
- [ ] privacy page listing the services above (`/replica-launch`)
- n/a webhooks, uploads, payments, email, OAuth: none
