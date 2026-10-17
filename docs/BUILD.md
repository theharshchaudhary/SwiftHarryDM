# Building and testing SwiftHarryDM

## Prerequisites

- Windows 10/11 x64.
- Python 3.11 (CI uses 3.11).
- Optional: Inno Setup 6, to build the installer.
- Optional: Chrome and Node.js 22+, for the browser-extension end-to-end test.

## Development setup

```powershell
python -m venv .venv
.venv\Scripts\python -m pip install --require-hashes -r requirements.lock
.venv\Scripts\python -m pip install --require-hashes -r requirements-dev.lock
.venv\Scripts\python packaging\fetch_tools.py
```

`fetch_tools.py` downloads FFmpeg `n8.1.3` (BtbN LGPL build). It verifies the pinned
SHA-256 and extracts only `ffmpeg.exe`, `ffprobe.exe` and the license into
`packaging\tools\ffmpeg` (git-ignored).

## Updating dependencies

1. Edit `requirements.in` or `requirements-dev.in`.
2. Regenerate the lock files:
   ```powershell
   .venv\Scripts\python -m piptools compile --generate-hashes --allow-unsafe --strip-extras -o requirements.lock requirements.in
   .venv\Scripts\python -m piptools compile --generate-hashes --allow-unsafe --strip-extras -o requirements-dev.lock requirements-dev.in
   ```
3. Run the full test suite, including `-m live`, before merging.

yt-dlp changes often. Users can update it in-app (*Help > Update yt-dlp*): the download is
SHA-256 checked against the release's `SHA2-256SUMS`, and the previous copy is kept.

## Test suites

| Command | What it covers |
|---|---|
| `pytest -m "not live"` | PR gate: unit, integration, UI (real Tk), API security, Chrome extension E2E, full-app E2E |
| `pytest -m slow` | 200 forced-kill DB test, 100 randomized kill/fault download runs |
| `pytest -m live` | Live CC-licensed YouTube media (1080p + audio, MP3, pause/resume) |
| `SWIFTDM_PROP_EXAMPLES=10000 pytest tests/core/test_manager_property.py` | Full property-test run |
| `python bench/bench_segments.py --out bench/results.json` | Throughput (1 vs 8 segments) and 4 GiB memory benchmark |

GUI tests need an interactive desktop session (GitHub's `windows-latest` runners have one).

## Release build

```powershell
.venv\Scripts\python packaging\build.py --zip            # one-folder app + zip + .sha256
.venv\Scripts\python packaging\build.py --zip --installer  # also Inno Setup installer
```

Output goes to `packaging\dist\`. The build smoke-checks the bundled yt-dlp and FFmpeg.

### Code signing

Never commit certificates or passwords. `.gitignore` blocks `*.pfx`, `*.p12`, `*.key` and
similar files. To sign, import the certificate into the Windows certificate store on the
build machine (or use a cloud HSM), then set `SIGNTOOL_CERT_THUMBPRINT` before running
`build.py`.
