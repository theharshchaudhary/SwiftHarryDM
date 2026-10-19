// Development helper: download the pinned, SHA-256-verified yt-dlp.exe into tools/bin
// (git-ignored), which debug builds use. Installed copies download it on first use.
// FFmpeg is built from source by scripts/ffmpeg/build.sh into tools/ffmpeg-slim.
// Usage: node scripts/fetch-tools.mjs [destDir]
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DEST = resolve(process.argv[2] || join(ROOT, 'tools', 'bin'));

// Keep in step with YTDLP_PINNED in crates/swiftdm-core/src/media/tools.rs.
const YTDLP = {
  url: 'https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe',
  sha256: '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a',
};

mkdirSync(DEST, { recursive: true });
const exe = join(DEST, 'yt-dlp.exe');
const stamp = join(DEST, 'yt-dlp.version');
if (existsSync(exe) && existsSync(stamp) && readFileSync(stamp, 'utf8').trim() === YTDLP.sha256) {
  console.log('yt-dlp: already present');
} else {
  console.log(`yt-dlp: downloading ${YTDLP.url}`);
  const res = await fetch(YTDLP.url);
  if (!res.ok) throw new Error(`yt-dlp: HTTP ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  const got = createHash('sha256').update(buf).digest('hex');
  if (got !== YTDLP.sha256) throw new Error(`yt-dlp: SHA-256 mismatch (got ${got}, expected ${YTDLP.sha256})`);
  writeFileSync(exe, buf);
  writeFileSync(stamp, YTDLP.sha256 + '\n');
  console.log(`yt-dlp: verified and installed to ${DEST}`);
}
if (!existsSync(join(ROOT, 'tools', 'ffmpeg-slim', 'ffmpeg.exe'))) {
  console.log('FFmpeg: not built yet. Run scripts/ffmpeg/build.sh (see docs/DEVELOPMENT.md).');
}
