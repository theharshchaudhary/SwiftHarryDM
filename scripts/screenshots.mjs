// README screenshots: starts the debug app with a throwaway profile, downloads a few public
// files for a few seconds, captures windows into docs/images, then removes
// the downloads and the profile.
// Usage: node scripts/screenshots.mjs   (build first: tauri build --debug --no-bundle)
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { App, sleep, until } from "../tests/e2e/lib/webview.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "docs", "images");
const CDP_PORT = 9342;
const work = mkdtempSync(join(tmpdir(), "sdm-shots-"));
const DL = join(work, "dl");
mkdirSync(join(work, "home"), { recursive: true });
mkdirSync(OUT, { recursive: true });

const FILES = {
  ubuntu: "https://releases.ubuntu.com/24.04/ubuntu-24.04.5.1-desktop-amd64.iso",
  sintel: "https://download.blender.org/durian/movies/Sintel.2010.1080p.mkv",
  vlc: "https://download.videolan.org/pub/videolan/vlc/last/win64/vlc-3.0.24-win64.exe",
  pdf: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
  video: "https://www.youtube.com/watch?v=aqz-KE-bpKQ", // Big Buck Bunny (CC BY 3.0)
};

writeFileSync(
  join(work, "home", "settings3.json"),
  JSON.stringify({ download_dir: DL, proxy_mode: "none", clipboard_monitor: false, sound_on_complete: false, theme: "light", tips_on_startup: false }),
);
const proc = spawn(join(ROOT, "target", "debug", "swiftharrydm.exe"), [], {
  env: { ...process.env, SWIFTDM_HOME: join(work, "home"), SWIFTDM_DEVTOOLS_PORT: String(CDP_PORT) },
  stdio: "ignore",
});

const shot = async (w, name) => {
  await sleep(400);
  await w.shot(join(OUT, name));
  console.log(`saved docs/images/${name}`);
};

const app = new App(CDP_PORT);
try {
  await until(() => app.targets().then((t) => t.length > 0), 60000, 300, "the app to start");
  const main = await app.window("SwiftHarryDM");
  const spec = (url, extra = {}) => ({ url, dest_dir: DL, source: "batch", ...extra });
  await main.invoke("submit", { spec: spec(FILES.pdf), mode: "now" });
  await sleep(3000);
  await main.invoke("submit", { spec: spec(FILES.video, { kind: "media", format_label: "720", media: null }), mode: "now" }).catch((e) => console.log(`video skipped: ${e.message}`));
  await main.invoke("submit", { spec: spec(FILES.sintel), mode: "now" });
  await main.invoke("submit", { spec: spec(FILES.vlc), mode: "later" });
  // The ISO opens its progress window, like a download started by hand.
  await main.invoke("submit", { spec: spec(FILES.ubuntu, { source: "ui" }), mode: "now" });
  await sleep(12000);

  await shot(main, "main.png");
  const prog = await app.window((t) => t.title.includes("ubuntu"), 20000);
  await shot(prog, "progress.png");

  // Dark theme
  const s = (await main.invoke("get_state")).settings;
  await main.invoke("save_settings", { settings: { ...s, theme: "dark" } });
  await sleep(800);
  await shot(main, "main-dark.png");
  await main.invoke("save_settings", { settings: { ...s, theme: "light" } });

  // The "Download file info" window for an address typed by hand
  await main.click("Add URL");
  const add = await app.window("Add download");
  await add.fill("#u", "https://download.blender.org/release/Blender4.2/blender-4.2.0-windows-x64.msi");
  await add.click("OK");
  const info = await app.window("Download file info");
  await until(async () => !(await info.text()).includes("Checking the address"), 60000, 250, "the address check");
  await shot(info, "file-info.png");
  await info.click("Cancel");

  for (const [kind, params, title, name] of [
    ["scheduler", {}, "Scheduler", "scheduler.png"],
  ]) {
    await main.invoke("open_window", { kind, params });
    const w = await app.window(title);
    await shot(w, name);
    await w.invoke("close_self").catch(() => {});
  }

  const ids = (await main.invoke("get_state")).jobs.map((j) => j.id);
  await main.invoke("remove", { ids, deleteFiles: true });
  await sleep(1500);
  await main.invoke("exit_app").catch(() => {});
} finally {
  // Wait for the app (and the video tools it started) to exit before removing the profile.
  const exited = new Promise((r) => (proc.exitCode !== null ? r() : proc.once("exit", r)));
  await Promise.race([exited, sleep(5000)]);
  if (proc.exitCode === null) {
    spawn("taskkill", ["/pid", String(proc.pid), "/t", "/f"], { stdio: "ignore" });
    await exited;
  }
  await sleep(1000);
  rmSync(work, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
}
