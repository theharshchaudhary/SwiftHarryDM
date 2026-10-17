// Drives a real Chrome over the DevTools protocol (pipe transport) for extension E2E tests.
// Usage: node cdp_driver.mjs <chrome.exe> <extension_dir> <profile_dir>
// Reads one JSON command per stdin line, writes one JSON result per stdout line:
//   {"cmd":"sw_eval","expr":"..."}            evaluate in the extension service worker
//   {"cmd":"page_open","url":"..."}           open a tab
//   {"cmd":"page_eval","expr":"..."}          evaluate in that tab
//   {"cmd":"quit"}
import { spawn } from 'node:child_process';
import readline from 'node:readline';

const [chromePath, extDir, profile] = process.argv.slice(2);
const chrome = spawn(chromePath, [
  '--remote-debugging-pipe', '--enable-unsafe-extension-debugging', '--headless=new',
  `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-gpu',
  'about:blank',
], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });

const toChrome = chrome.stdio[3];
const fromChrome = chrome.stdio[4];
let nextId = 1;
const pending = new Map();
let buf = '';
fromChrome.on('data', (chunk) => {
  buf += chunk.toString('utf8');
  let i;
  while ((i = buf.indexOf('\0')) >= 0) {
    const msg = JSON.parse(buf.slice(0, i));
    buf = buf.slice(i + 1);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    }
  }
});

function send(method, params = {}, sessionId) {
  const id = nextId++;
  const msg = { id, method, params };
  if (sessionId) msg.sessionId = sessionId;
  toChrome.write(JSON.stringify(msg) + '\0');
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error(`timeout ${method}`)); } }, 30000);
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let swSession = null;
let pageSession = null;
let extensionId = null;

async function attachServiceWorker() {
  for (let i = 0; i < 100; i++) {
    const { targetInfos } = await send('Target.getTargets');
    const sw = targetInfos.find((t) => t.type === 'service_worker' && t.url.startsWith(`chrome-extension://${extensionId}/`));
    if (sw) {
      const { sessionId } = await send('Target.attachToTarget', { targetId: sw.targetId, flatten: true });
      return sessionId;
    }
    await sleep(100);
  }
  throw new Error('extension service worker not found');
}

async function evaluate(sessionId, expr) {
  const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }, sessionId);
  if (r.exceptionDetails) {
    return { ok: false, error: r.exceptionDetails.exception?.description || r.exceptionDetails.text };
  }
  return { ok: true, value: r.result.value };
}

async function main() {
  const loaded = await send('Extensions.loadUnpacked', { path: extDir });
  extensionId = loaded.id;
  swSession = await attachServiceWorker();
  process.stdout.write(JSON.stringify({ ready: true, extensionId }) + '\n');
  const rl = readline.createInterface({ input: process.stdin });
  for await (const line of rl) {
    if (!line.trim()) continue;
    const c = JSON.parse(line);
    let out;
    try {
      if (c.cmd === 'sw_eval') {
        out = await evaluate(swSession, c.expr);
      } else if (c.cmd === 'page_open') {
        const { targetId } = await send('Target.createTarget', { url: c.url });
        const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
        pageSession = sessionId;
        await send('Runtime.enable', {}, pageSession);
        await sleep(1000);
        out = { ok: true };
      } else if (c.cmd === 'page_eval') {
        out = await evaluate(pageSession, c.expr);
      } else if (c.cmd === 'quit') {
        process.stdout.write(JSON.stringify({ ok: true }) + '\n');
        break;
      } else {
        out = { ok: false, error: 'unknown command' };
      }
    } catch (e) {
      out = { ok: false, error: String(e) };
    }
    process.stdout.write(JSON.stringify(out) + '\n');
  }
  try { await send('Browser.close'); } catch (e) { /* ignore */ }
  chrome.kill();
  process.exit(0);
}

main().catch((e) => {
  process.stdout.write(JSON.stringify({ ready: false, error: String(e) }) + '\n');
  chrome.kill();
  process.exit(1);
});
