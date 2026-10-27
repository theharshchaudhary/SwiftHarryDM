# Landing page

Built: [`docs/index.html`](../../docs/index.html), served by GitHub Pages from `docs/`
(see `/replica-deploy`). Screenshots: `docs/images/` from `scripts/screenshots.mjs` (real
app, real public downloads). Colours: the brand tokens (AA in both themes,
`contrast.py` 2026-10-26). No testimonials, user counts, ratings or press logos. No
reviewer quotes. The original's name does not appear.

| section | content | source |
| --- | --- | --- |
| hero | "The free download manager that doesn't make you give anything up" + Download for Windows + main window screenshot | angle A |
| problem | the good ones cost money, so people crack them; the free ones leave things out | fixes.md: hate #2, list 2 |
| how it works | install + extension, click as usual, it runs and resumes | F01 |
| features | the 4 fixes first (expired links, ~15 MB in the tray, tray progress, sounds), then 10 parity lines | fixes.md fix plan 1–4 |
| price | Free | pricing.md |
| FAQ | free?, import (.ef2), browsers (no Firefox yet), DRM, data, Mac/Linux | the real objections |
| CTA | Download for Windows | |

Claims checked against the product (2026-10-26):
- "about 15 MB" in the tray: measured at 11–14 MB on the debug build (`build-log.md`).
- Chrome, Edge and Brave: native messaging is registered for all three (`browser.rs`).
- "checks GitHub for updates": the only outside services (`backend.md`).
