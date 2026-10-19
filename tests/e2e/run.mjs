// End-to-end tests of the built desktop app (and the browser extension in a real Chrome).
//
//   cargo build -p swiftdm-testkit --bin sdm-observe
//   ui\node_modules\.bin\tauri build --debug --no-bundle
//   node tests/e2e/run.mjs [--only name,name] [--keep]
//
// Starts the local test server and the app with a throwaway profile (your real downloads and
// settings are never touched), drives the app's windows over WebView2 DevTools, and checks
// every downloaded file's SHA-256 against what the server sent. Screenshots go to the
// temporary folder printed at the start (kept with --keep).
import { execSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { App, sleep, until } from "./lib/webview.mjs";
import { Chrome } from "./lib/chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const APP_EXE = join(ROOT, "target", "debug", "swiftharrydm.exe");
const OBSERVE_EXE = join(ROOT, "target", "debug", "sdm-observe.exe");
const CHROME = [
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
  join(process.env.LOCALAPPDATA ?? "", "Google\\Chrome\\Application\\chrome.exe"),
].find(existsSync);
const SRV_PORT = 18093;
const CDP_PORT = 9341;
const EXT_ORIGIN = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

const args = process.argv.slice(2);
const only = args.includes("--only") ? args[args.indexOf("--only") + 1].split(",") : null;
const keep = args.includes("--keep");

const work = mkdtempSync(join(tmpdir(), "sdm-e2e-"));
const DL = join(work, "dl");
const SHOTS = join(work, "shots");
for (const d of [DL, SHOTS, join(work, "home"), join(work, "observe")]) mkdirSync(d, { recursive: true });

const sha256 = (p) =>
  new Promise((res, rej) => {
    const h = createHash("sha256");
    createReadStream(p).on("data", (c) => h.update(c)).on("end", () => res(h.digest("hex"))).on("error", rej);
  });
const expected = () => JSON.parse(readFileSync(join(work, "observe", "expected.json"), "utf8"));
const URL_ = (path) => `http://127.0.0.1:${SRV_PORT}${path}`;
function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}
function findFile(dir, name) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name === name) return p;
  }
  return null;
}
async function assertSha(name, path) {
  const f = findFile(DL, name);
  assert(f, `${name} was not saved`);
  const got = await sha256(f);
  assert(got === expected()[path].sha256, `${name}: SHA-256 differs from what the server sent`);
}

// ------------------------------------------------------------------ processes
const procs = [];
function start(exe, argv, env = {}) {
  const p = spawn(exe, argv, { env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] });
  procs.push(p);
  return p;
}
function cleanup() {
  for (const p of procs) {
    try {
      p.kill();
    } catch {
      /* gone */
    }
  }
}
process.on("exit", cleanup);

async function setup() {
  for (const exe of [APP_EXE, OBSERVE_EXE]) assert(existsSync(exe), `${exe} is missing; build it first (see the top of this file)`);
  const srv = start(OBSERVE_EXE, [String(SRV_PORT), join(work, "observe")]);
  await new Promise((res, rej) => {
    srv.stdout.on("data", (d) => String(d).includes("ready") && res());
    srv.on("exit", (c) => rej(new Error(`sdm-observe exited (${c}); is port ${SRV_PORT} free?`)));
  });
  writeFileSync(
    join(work, "home", "settings3.json"),
    JSON.stringify({ download_dir: DL, proxy_mode: "none", clipboard_monitor: false, sound_on_complete: false }),
  );
  // SWIFTDM_SKIP_LICENSE only works in debug builds: tests must not depend on the license server.
  const proc = start(APP_EXE, [], {
    SWIFTDM_HOME: join(work, "home"),
    SWIFTDM_SKIP_LICENSE: "1",
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
  });
  let output = "";
  let exited = null;
  proc.stdout.on("data", (d) => (output += d));
  proc.stderr.on("data", (d) => (output += d));
  proc.on("exit", (c) => (exited = c));
  const app = new App(CDP_PORT);
  try {
    await until(() => app.targets().then((t) => t.length > 0), 60000, 300, "the app to start");
  } catch (e) {
    // Say why: whether the app is still running, what it printed, and its log.
    const logs = join(work, "home", "logs");
    const log = existsSync(logs) ? readdirSync(logs).map((f) => readFileSync(join(logs, f), "utf8")).join("\n") : "(no log folder)";
    const sh = (cmd) => {
      try {
        return execSync(cmd, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
      } catch (err) {
        return String(err.stdout || err.message);
      }
    };
    const webviews = sh(`powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \\"Name='msedgewebview2.exe'\\" | Select-Object -First 3 -ExpandProperty CommandLine"`);
    const ports = sh("netstat -ano -p tcp").split("\n").filter((l) => l.includes("LISTENING") && l.includes("127.0.0.1")).join("\n");
    throw new Error(
      `${e.message}\napp ${exited === null ? "still running" : `exited with code ${exited}`}\n--- app output ---\n${output.slice(-4000)}\n--- app log ---\n${log.slice(-4000)}` +
        `\n--- WebView2 processes ---\n${webviews.slice(0, 3000) || "(none)"}\n--- listening on 127.0.0.1 ---\n${ports}`,
    );
  }
  return app;
}

// ------------------------------------------------------------------ helpers
async function addThroughUi(app, main, url) {
  await main.click("Add URL");
  const add = await app.window("Add download");
  await add.fill("#u", url);
  await add.click("OK");
  const info = await app.window("Download file info");
  await until(async () => !(await info.text()).includes("Checking the address"), 60000, 250, "the address check");
  return info;
}

/** The "Download complete" window for one file (a previous one may still be closing). */
async function completeFor(app, name, timeout = 90000) {
  return until(async () => {
    for (const t of (await app.targets()).filter((x) => x.title === "Download complete")) {
      const w = await app.window((x) => x.id === t.id, 3000).catch(() => null);
      if (w && (await w.text().catch(() => "")).includes(name)) return w;
      w?.close();
    }
    return null;
  }, timeout, 300, `the complete window for ${name}`);
}

async function apiBase() {
  for (const p of [5001, 5002]) {
    try {
      const r = await fetch(`http://127.0.0.1:${p}/health`);
      if ((await r.json()).app === "SwiftHarryDM") return `http://127.0.0.1:${p}`;
    } catch {
      /* next */
    }
  }
  throw new Error("the extension API is not listening");
}

// ------------------------------------------------------------------ scenarios
const scenarios = {
  async small_file(app) {
    const main = await app.window("SwiftHarryDM");
    const info = await addThroughUi(app, main, URL_("/s6/get"));
    const t = await info.text();
    assert(t.includes("300,000 bytes"), "the info window shows the size");
    assert((await info.eval("document.querySelector('select').value")) === "Documents", "a PDF goes to Documents");
    await info.shot(join(SHOTS, "info.png"));
    await info.click("Start download");
    const done = await completeFor(app, "report 2026.pdf", 30000);
    await done.shot(join(SHOTS, "complete.png"));
    await done.click("Close");
    await assertSha("report 2026.pdf", "/s6/get");
  },

  async pause_resume(app) {
    const main = await app.window("SwiftHarryDM");
    const s = await main.invoke("get_state");
    await main.invoke("save_settings", { settings: { ...s.settings, speed_limit: 6 * 1024 * 1024 } });
    const info = await addThroughUi(app, main, URL_("/s4/stall.bin"));
    await info.click("Start download");
    const prog = await app.window((t) => t.title.includes("stall.bin") || t.title === "Download progress", 20000);
    await sleep(2500);
    const segs = await prog.eval("document.querySelectorAll('.segmap .seg').length");
    assert(segs >= 4, `the segment map shows the connections (${segs})`);
    await prog.shot(join(SHOTS, "progress.png"));
    await main.shot(join(SHOTS, "main.png"));
    assert(await prog.click("Pause"), "Pause is available");
    await until(async () => (await prog.text()).includes("Resume"), 15000, 200, "pause");
    await prog.click("Resume");
    await main.invoke("save_settings", { settings: { ...(await main.invoke("get_state")).settings, speed_limit: 0 } });
    const done = await completeFor(app, "stall.bin");
    await done.click("Close");
    await assertSha("stall.bin", "/s4/stall.bin");
  },

  async every_window_opens(app) {
    const main = await app.window("SwiftHarryDM");
    const job = (await main.invoke("get_state")).jobs[0];
    const cases = [
      ["options", {}, "Options", "Connection"],
      ["scheduler", {}, "Scheduler", "Start this queue automatically"],
      ["batch", {}, "Batch download", "Replace * with"],
      ["links", { source: "typed", urls: JSON.stringify(["https://a.example/1.zip"]) }, "Choose files to download", "a.example"],
      ["about", {}, "About SwiftHarryDM", "Version"],
      ["license", {}, "License", "License check skipped"],
      ["browser", {}, "Connect your browser", "extension"],
      ["options", { tab: "video" }, "Options", "yt-dlp"],
    ];
    if (job) cases.push(["properties", { id: job.id }, "Properties", "File name"]);
    for (const [kind, params, title, text] of cases) {
      if (await app.hasWindow(title)) {
        const old = await app.window(title);
        await old.invoke("close_self").catch(() => {});
        await until(async () => !(await app.hasWindow(title)), 5000, 100, `${title} to close`);
      }
      await main.invoke("open_window", { kind, params });
      const w = await app.window(title);
      assert((await w.text()).includes(text), `${title} shows its content`);
      await w.shot(join(SHOTS, `window-${kind}${params.tab ? "-" + params.tab : ""}.png`));
      await w.invoke("close_self").catch(() => {});
      await until(async () => !(await app.hasWindow(title)), 5000, 100, `${title} to close`);
    }
  },

  async drop_target_and_command_line(app) {
    const main = await app.window("SwiftHarryDM");
    await main.invoke("open_window", { kind: "drop", params: {} });
    const drop = await app.window("Drop target");
    await drop.shot(join(SHOTS, "drop-target.png"));
    await main.invoke("close_window", { label: "drop" });
    await until(async () => !(await app.hasWindow("Drop target")), 5000, 100, "the drop target to close");

    // A second start with /d <url> /n hands the download to the running app, no window.
    const url = URL_("/p/alt.pdf");
    const before = (await main.invoke("get_state")).jobs.filter((j) => j.url === url).length;
    const second = spawn(APP_EXE, ["/d", url, "/n"], { env: { ...process.env, SWIFTDM_HOME: join(work, "home"), SWIFTDM_SKIP_LICENSE: "1" } });
    await new Promise((res) => second.on("exit", res));
    const job = await until(async () => {
      const jobs = (await main.invoke("get_state")).jobs.filter((j) => j.url === url);
      return jobs.length > before && jobs.find((j) => j.state === "COMPLETED");
    }, 30000, 300, "the command-line download");
    assert(!(await app.hasWindow("Download file info")), "/n starts without asking");
    assert(job.file_name === "alt.pdf", `named from the server (${job.file_name})`);
    await assertSha("alt.pdf", "/p/alt.pdf");
  },

  async extension_protocol(app) {
    const base = await apiBase();
    const call = async (method, path, body, token) => {
      const headers = { Origin: EXT_ORIGIN };
      if (body) headers["Content-Type"] = "application/json";
      if (token) headers.Authorization = `Bearer ${token}`;
      const r = await fetch(base + path, { method, headers, body: body ? JSON.stringify(body) : undefined });
      return [r.status, await r.json().catch(() => null)];
    };
    const [s, v] = await call("POST", "/api/pair");
    assert(s === 202, `pairing starts (${s})`);
    const pw = await app.window("Connect browser extension");
    const code = (await pw.eval("document.querySelector('.countdown').innerText")).replace(/\s/g, "");
    const [s2, v2] = await call("POST", "/api/pair/confirm", { pairing_id: v.pairing_id, code });
    assert(s2 === 200 && v2.token, "the code from the app window pairs the extension");
    const [s3, v3] = await call("POST", "/api/download", { url: URL_("/s7/flaky.bin"), fileName: "flaky.bin", pageUrl: "https://example.org/", mode: "now" }, v2.token);
    assert(s3 === 202 && v3.status === "pending_confirmation", "browser downloads wait for confirmation");
    const info = await app.window("Download file info");
    await until(async () => !(await info.text()).includes("Checking the address"), 30000);
    assert((await info.text()).includes("Sent from your browser"), "the info window says where it came from");
    await info.click("Start download");
    const done = await completeFor(app, "flaky.bin");
    await done.click("Close");
    await assertSha("flaky.bin", "/s7/flaky.bin");
  },

  async real_chrome_extension(app) {
    if (!CHROME) {
      console.log("    (skipped: Chrome is not installed)");
      return;
    }
    const chromeDl = join(work, "chrome-dl");
    mkdirSync(chromeDl, { recursive: true });
    const chrome = await new Chrome(CHROME, join(ROOT, "extension"), join(work, "chrome-profile"), chromeDl).start();
    try {
      // The extension connects by itself through the app's native messaging host: no code.
      assert(chrome.extensionId === "eogdkmmfefponkkkcfgdlgnngmdjckhe", `the extension ID is fixed (${chrome.extensionId})`);
      const st = await until(async () => {
        const s = await chrome.worker("backgroundManager.status()");
        return s.connected && s.paired ? s : null;
      }, 20000, 500, "the extension to connect by itself");
      assert(st.paired, "connected without a pairing code");
      assert(!(await app.hasWindow("Connect browser extension")), "no pairing code was needed");

      // A normal click on a PDF link: the app takes it over.
      const tab = await chrome.tab(URL_("/p/links.html"));
      await chrome.evalIn(tab, "document.getElementById('report').click()");
      const info = await app.window("Download file info", 20000);
      assert((await info.text()).includes("Sent from your browser"), "the captured download opens the info window");
      await info.shot(join(SHOTS, "chrome-capture.png"));
      await until(async () => !(await info.text()).includes("Checking the address"), 30000);
      await info.click("Start download");
      const done = await completeFor(app, "report 2026", 30000);
      await done.click("Close");
      const left = await chrome.worker("chrome.downloads.search({}).then(l => l.map(d => d.state + ':' + d.filename))");
      assert(left.length === 0, `the browser's own download was cancelled and removed (${left})`);

      // Alt+click keeps the download in the browser.
      await until(() => chrome.evalIn(tab, "!!document.getElementById('alt') || location.href").then((v) => v === true || (() => { throw new Error('links tab is at ' + v); })()), 10000, 250, 'the links page');
      await chrome.evalIn(tab, "document.getElementById('alt').dispatchEvent(new MouseEvent('mousedown', { bubbles: true, altKey: true }))");
      await sleep(400);
      await chrome.evalIn(tab, "document.getElementById('alt').click()");
      await until(() => existsSync(join(chromeDl, "alt.pdf")), 20000, 250, "Chrome to save alt.pdf itself");
      assert(!(await app.hasWindow("Download file info")), "an Alt-clicked download is not taken over");

      // Hovering a player shows "Download this video"; its panel lists the app's formats.
      const vtab = await chrome.tab(URL_("/p/video.html"));
      const shadow = "document.querySelector('swiftharrydm-overlay').shadowRoot";
      await until(async () => {
        await chrome.evalIn(vtab, "document.getElementById('v').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))");
        return chrome.evalIn(vtab, `!!document.querySelector('swiftharrydm-overlay') && ${shadow}.querySelector('.pill').style.display === 'flex'`);
      }, 15000, 300, "the Download this video button");
      await chrome.evalIn(vtab, `${shadow}.querySelector('.pill').click()`);
      const nFormats = await until(
        () => chrome.evalIn(vtab, `${shadow}.querySelectorAll('optgroup[label="Exact formats"] option').length`),
        120000, 500, "the app's format list in the page panel");
      assert(nFormats >= 1, `the panel lists the video's formats (${nFormats})`);
      await chrome.shot(vtab, join(SHOTS, "chrome-video-panel.png"));
      await chrome.evalIn(vtab, `(() => { const s = ${shadow}.querySelector('.format-select');
        s.value = s.querySelector('optgroup[label="Exact formats"] option').value; })()`);
      await chrome.evalIn(vtab, `${shadow}.querySelector('.download-now-btn').click()`);
      const vinfo = await app.window("Download file info", 20000);
      assert((await vinfo.text()).includes("Sent from your browser"), "the chosen video format reaches the app");
      await vinfo.click("Cancel");
      await chrome.closeTab(vtab);

      // On a site the video downloader does not know, the panel offers the HLS stream the
      // player loaded, and the toolbar badge counts it.
      const htab = await chrome.tab(URL_("/p/hls.html"));
      const streams = await until(async () => {
        const all = await chrome.worker("chrome.storage.session.get(null)");
        return Object.values(all).find((l) => Array.isArray(l) && l.some((x) => x.kind === "hls")) ?? null;
      }, 15000, 300, "the player's stream to be noticed");
      assert(!streams.some((x) => x.url.endsWith(".ts")), "stream fragments are not listed");
      await until(async () => {
        await chrome.evalIn(htab, "document.getElementById('v').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))");
        return chrome.evalIn(htab, `!!document.querySelector('swiftharrydm-overlay') && ${shadow}.querySelector('.pill').style.display === 'flex'`);
      }, 15000, 300, "the Download this video button on the stream page");
      await chrome.evalIn(htab, `${shadow}.querySelector('.pill').click()`);
      const picked = await until(
        () => chrome.evalIn(htab, `(() => { const s = ${shadow}.querySelector('.format-select'); return s.value.startsWith('s:') ? s.selectedOptions[0].textContent : null; })()`),
        120000, 500, "the stream to be offered and selected");
      assert(picked.includes("HLS"), `the HLS stream is offered (${picked})`);
      await chrome.shot(htab, join(SHOTS, "chrome-stream-panel.png"));
      await chrome.evalIn(htab, `${shadow}.querySelector('.download-now-btn').click()`);
      const sinfo = await app.window("Download file info", 20000);
      const sText = await sinfo.text();
      const sUrl = await sinfo.eval("document.querySelector('.grid2 input').value");
      assert(sText.includes("Sent from your browser") && sUrl.endsWith("/v/stream.m3u8"), `the stream reaches the app (${sUrl})`);
      await sinfo.shot(join(SHOTS, "stream-info.png"));
      await sinfo.click("Cancel");
      await chrome.closeTab(htab);

      // "Download all links": the app shows the link chooser.
      // Like the context menu: the extension has no "tabs" permission, so tab URLs are hidden;
      // only the page with the content script answers.
      await chrome.worker(`chrome.tabs.query({}).then(t => Promise.all(t.map(x => chrome.tabs.sendMessage(x.id, { action: 'collectLinks', selectionOnly: false }).catch(() => null))))`);
      const links = await app.window("Choose files to download", 15000);
      const n = await until(() => links.eval("document.querySelectorAll('.tbl tbody tr').length"), 10000, 200, "the links");
      assert(n === 3, `all three links arrive (${n})`);
      await links.shot(join(SHOTS, "chrome-links.png"));
      await links.click("Cancel");
    } finally {
      await chrome.stop();
    }
  },
};

// ------------------------------------------------------------------ main
console.log(`work folder: ${work}`);
let failed = 0;
try {
  const app = await setup();
  for (const [name, fn] of Object.entries(scenarios)) {
    if (only && !only.includes(name)) continue;
    const t0 = Date.now();
    try {
      await fn(app);
      console.log(`PASS ${name} (${((Date.now() - t0) / 1000).toFixed(1)} s)`);
    } catch (e) {
      failed++;
      console.log(`FAIL ${name}: ${e.message}`);
    }
  }
} catch (e) {
  failed++;
  console.log(`FAIL setup: ${e.message}`);
}
cleanup();
await sleep(500);
if (!keep && failed === 0) rmSync(work, { recursive: true, force: true });
console.log(failed ? `${failed} scenario(s) failed; files kept in ${work}` : "all scenarios passed");
process.exit(failed ? 1 : 0);
