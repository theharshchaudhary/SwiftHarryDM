# Development

## Setup

- Rust stable (MSVC toolchain) and the Visual Studio 2022 C++ build tools with a Windows SDK
- Node.js 22 or newer
- `npm ci --prefix ui`
- Optional: `node scripts/fetch-tools.mjs` puts pinned, checksum-verified yt-dlp and FFmpeg
  into `tools\bin`, which debug builds use. Installed copies download them on first use.

## Run

```powershell
ui\node_modules\.bin\tauri dev                       # hot-reloading UI
ui\node_modules\.bin\tauri build --debug --no-bundle  # target\debug\swiftharrydm.exe
```

Run all `tauri` commands from the repository root.

`SWIFTDM_HOME=<folder>` makes the app keep its database, settings and logs in that folder
instead of `%LOCALAPPDATA%\SwiftHarryDM` (useful for a clean profile). In debug builds,
`SWIFTDM_SKIP_LICENSE=1` skips the license check.

## Tests

| Command | What it covers |
|---|---|
| `cargo test -p swiftdm-core` | State machine, store, manager (incl. randomized crash test), HTTP fault matrix, API security and pairing, media engine with a stand-in yt-dlp, licensing against a mock server |
| `cargo test -p swiftharrydm` | App shell helpers (native messaging framing) |
| `node tests/e2e/run.mjs` | The built debug app with a throwaway profile, driven over WebView2 DevTools, and the extension in a real headless Chrome; every download is checked by SHA-256 |

The end-to-end tests need `target\debug\swiftharrydm.exe` and `target\debug\sdm-observe.exe`
(`cargo build -p swiftdm-testkit --bin sdm-observe`). Use `--only <scenario>` and `--keep`
to keep screenshots.

`target\debug\sdm-observe.exe <port> <folder>` serves the same fixed scenarios for manual
testing (large file, no resume support, dropped connection, stall, changed file, attachment
name, temporary errors, a page with links and a page with a video).

## Release

1. Set the version in `Cargo.toml` (`workspace.package.version`), `src-tauri/tauri.conf.json`
   and `ui/package.json`.
2. Push a tag `vX.Y.Z`. The release workflow runs the tests, builds the installer, signs it
   for the updater and publishes it with `latest.json`. Installed copies then offer the
   update with one click.

The workflow needs two repository secrets: `TAURI_SIGNING_PRIVATE_KEY` (the contents of the
updater private key) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The matching public key is in
`src-tauri/tauri.conf.json`. Keep the private key safe: without it, installed copies cannot
be updated.

To build a signed installer locally:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw <path-to-updater.key>
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<password>"
ui\node_modules\.bin\tauri build
```

## Browser extension

The extension's ID is fixed by the public `key` in `extension/manifest.json`; the app's
native messaging host only answers that ID (and the store IDs in `src-tauri/src/browser.rs`).
After publishing to the Chrome Web Store or Edge Add-ons, put the store IDs into
`CHROME_STORE_ID` / `EDGE_STORE_ID`: the app then registers them so the browsers offer the
extension automatically.
