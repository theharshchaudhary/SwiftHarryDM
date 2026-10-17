# Migration from the legacy engine to v2, and rollback

## Current stage: beta (P8)

- **Default engine:** v2 (`Settings.engine = "v2"`).
- **Legacy engine:** still shipped and selectable. Legacy security hotfixes from P0 apply:
  no wildcard CORS, Host and Origin checks, TLS verification on.
- **Engine precedence:** `--engine legacy|v2` > `SWIFTDM_ENGINE` > Settings.

## Data

- Legacy persisted nothing (in-memory queue only), so there is nothing to migrate.
- v2 state lives in `%LOCALAPPDATA%\SwiftHarryDM\state.db`. Schema migrations are
  forward-only, and a backup `state.db.bak-v<N>` is written first. An older app refuses to
  modify a newer schema.
- Legacy and v2 share the licensing token (`~/.swift_dm_token.json`), so switching engines
  doesn't affect licensing.

## Rollback

| Problem | Action |
|---|---|
| A v2 bug that blocks users | Set the engine to `legacy` (Settings, `--engine legacy`, or `SWIFTDM_ENGINE=legacy`). No reinstall needed. |
| A bad commit | `git revert` it. Each phase is a separate commit on `v2-core`. |
| A bad yt-dlp update | Delete `%LOCALAPPDATA%\SwiftHarryDM\tools\yt-dlp.exe` (the bundled copy is used) or rename `yt-dlp.exe.prev` back. |
| A bad schema change | Restore `state.db.bak-v<N>`. |

Partial v2 downloads (`.<name>.<id>.sdmpart` files, `.<id>.sdmtmp` folders) can't be resumed
by the legacy engine. They can be deleted safely.

## When the legacy engine can be removed

All of the following must be true:

1. v2 has been the default for at least one release cycle (at least 4 weeks of real use).
2. 14 consecutive green nightly CI runs, live media tests included.
3. Zero open P0/P1 v2 bugs.
4. No beta report that needed the legacy fallback.
5. Harsh signs off `docs/PARITY.md`: every legacy capability is either present in v2 or
   deliberately dropped.

**None of these can be met yet.** v2 became the default on 2026-10-17. Removal will be a
single revertible PR, tagged `legacy-final` first. It deletes `UniversalDownloader` and the
legacy GUI in `main.py`, `download_window.py`, `src/*`, `utils.py`, and the legacy-only
`tqdm` dependency.
