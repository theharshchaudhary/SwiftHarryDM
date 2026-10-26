// A real Chrome with the unpacked Keepfetch extension, driven over the DevTools pipe
// (branded Chrome ignores --load-extension; Extensions.loadUnpacked works over the pipe).
import { spawn } from "node:child_process";
import { sleep } from "./webview.mjs";

export class Chrome {
  constructor(exe, extDir, profile, downloads) {
    this.exe = exe;
    this.extDir = extDir;
    this.profile = profile;
    this.downloads = downloads;
    this.id = 0;
    this.pending = new Map();
    this.buf = "";
  }

  async start() {
    this.proc = spawn(
      this.exe,
      ["--remote-debugging-pipe", "--enable-unsafe-extension-debugging", "--headless=new", `--user-data-dir=${this.profile}`,
        "--no-first-run", "--no-default-browser-check", "--disable-gpu", "about:blank"],
      { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
    );
    this.proc.stdio[4].on("data", (chunk) => {
      this.buf += chunk.toString("utf8");
      let i;
      while ((i = this.buf.indexOf("\0")) >= 0) {
        const msg = JSON.parse(this.buf.slice(0, i));
        this.buf = this.buf.slice(i + 1);
        const p = msg.id && this.pending.get(msg.id);
        if (p) {
          this.pending.delete(msg.id);
          msg.error ? p.reject(new Error(msg.error.message)) : p.resolve(msg.result);
        }
      }
    });
    const { id } = await this.send("Extensions.loadUnpacked", { path: this.extDir });
    this.extensionId = id;
    await this.send("Browser.setDownloadBehavior", { behavior: "allow", downloadPath: this.downloads, eventsEnabled: true });
    this.sw = await this.attachWorker();
    return this;
  }

  send(method, params = {}, sessionId) {
    const id = ++this.id;
    const msg = { id, method, params };
    if (sessionId) msg.sessionId = sessionId;
    this.proc.stdio[3].write(JSON.stringify(msg) + "\0");
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      setTimeout(() => {
        if (this.pending.delete(id)) reject(new Error(`timeout ${method}`));
      }, 30000);
    });
  }

  async attachWorker() {
    for (let i = 0; i < 150; i++) {
      const { targetInfos } = await this.send("Target.getTargets");
      const sw = targetInfos.find((t) => t.type === "service_worker" && t.url.startsWith(`chrome-extension://${this.extensionId}/`));
      if (sw) return (await this.send("Target.attachToTarget", { targetId: sw.targetId, flatten: true })).sessionId;
      await sleep(100);
    }
    throw new Error("extension service worker not found");
  }

  async evalIn(session, expr) {
    const r = await this.send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }, session);
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  }

  /** Evaluate in the extension's service worker. */
  worker(expr) {
    return this.evalIn(this.sw, expr);
  }

  /** Open a tab and return its session. */
  async tab(url) {
    const { targetId } = await this.send("Target.createTarget", { url });
    const { sessionId } = await this.send("Target.attachToTarget", { targetId, flatten: true });
    await this.send("Runtime.enable", {}, sessionId);
    await sleep(1200);
    this.targets = { ...(this.targets ?? {}), [sessionId]: targetId };
    return sessionId;
  }

  async shot(session, path) {
    const r = await this.send("Page.captureScreenshot", { format: "png" }, session);
    (await import("node:fs")).writeFileSync(path, Buffer.from(r.data, "base64"));
  }

  async closeTab(session) {
    await this.send("Target.closeTarget", { targetId: this.targets[session] });
  }

  async stop() {
    try {
      await this.send("Browser.close");
    } catch {
      /* already gone */
    }
    this.proc.kill();
  }
}
