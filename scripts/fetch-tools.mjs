// Download the pinned, SHA-256-verified media tools into tools/bin (git-ignored).
//   yt-dlp.exe  official standalone release
//   ffmpeg.exe / ffprobe.exe  BtbN LGPL build (no GPL-only components)
// Usage: node scripts/fetch-tools.mjs [destDir]
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, rmSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DEST = resolve(process.argv[2] || join(ROOT, 'tools', 'bin'));

const TOOLS = [
  {
    name: 'yt-dlp',
    url: 'https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe',
    sha256: '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a',
    files: { 'yt-dlp.exe': null },
  },
  {
    name: 'ffmpeg',
    url: 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n8.1.3-9-g29e619e767-win64-lgpl-8.1.zip',
    sha256: '4a7642b2264c03e8a0ce8a3825b933ee5580656f45695a086fe7e294045ffc0a',
    zipRoot: 'ffmpeg-n8.1.3-9-g29e619e767-win64-lgpl-8.1',
    files: { 'ffmpeg.exe': 'bin/ffmpeg.exe', 'ffprobe.exe': 'bin/ffprobe.exe', 'FFMPEG-LICENSE.txt': 'LICENSE.txt' },
  },
];

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

async function fetchTool(t) {
  const stamp = join(DEST, `${t.name}.version`);
  if (existsSync(stamp) && readFileSync(stamp, 'utf8').trim() === t.sha256 &&
      Object.keys(t.files).every((f) => existsSync(join(DEST, f)))) {
    console.log(`${t.name}: already present`);
    return;
  }
  console.log(`${t.name}: downloading ${t.url}`);
  const res = await fetch(t.url);
  if (!res.ok) throw new Error(`${t.name}: HTTP ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  const got = sha256(buf);
  if (got !== t.sha256) throw new Error(`${t.name}: SHA-256 mismatch (got ${got}, expected ${t.sha256})`);
  console.log(`${t.name}: sha256 verified`);
  if (!t.zipRoot) {
    writeFileSync(join(DEST, Object.keys(t.files)[0]), buf);
  } else {
    const work = mkdtempSync(join(tmpdir(), 'sdm-tools-'));
    try {
      const zip = join(work, 'a.zip');
      writeFileSync(zip, buf);
      const members = Object.values(t.files).map((m) => `${t.zipRoot}/${m}`);
      // Windows 10+ ships bsdtar, which extracts zip archives; only the named members are taken.
      execFileSync('tar', ['-xf', zip, '-C', work, ...members], { stdio: 'inherit' });
      for (const [out, member] of Object.entries(t.files)) {
        copyFileSync(join(work, t.zipRoot, member), join(DEST, out));
      }
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  }
  writeFileSync(stamp, t.sha256 + '\n');
  console.log(`${t.name}: installed to ${DEST}`);
}

mkdirSync(DEST, { recursive: true });
for (const t of TOOLS) {
  await fetchTool(t);
}
