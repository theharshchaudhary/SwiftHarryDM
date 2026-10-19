// Drive the app's WebView2 windows over the DevTools HTTP endpoint (the app must be started
// with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>).
import { writeFileSync } from "node:fs";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function until(fn, timeout = 30000, step = 250, what = "condition") {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = String(e);
    }
    await sleep(step);
  }
  throw new Error(`timed out waiting for ${what}; last=${JSON.stringify(last)}`);
}

export class Page {
  constructor(target) {
    this.t = target;
    this.id = 0;
    this.pending = new Map();
  }
  async open() {
    this.ws = new WebSocket(this.t.webSocketDebuggerUrl);
    await new Promise((res, rej) => {
      this.ws.onopen = res;
      this.ws.onerror = () => rej(new Error("cannot attach to window"));
    });
    this.ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      const p = d.id && this.pending.get(d.id);
      if (p) {
        this.pending.delete(d.id);
        p(d);
      }
    };
    // A closed window must fail pending calls instead of hanging.
    this.ws.onclose = () => {
      for (const p of this.pending.values()) p({ error: { message: "window closed" } });
      this.pending.clear();
    };
    return this;
  }
  send(method, params = {}) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      if (this.ws.readyState !== WebSocket.OPEN) return rej(new Error("window closed"));
      this.pending.set(id, (d) => (d.error ? rej(new Error(d.error.message)) : res(d.result)));
      this.ws.send(JSON.stringify({ id, method, params }));
      setTimeout(() => {
        if (this.pending.delete(id)) rej(new Error(`timeout ${method}`));
      }, 20000);
    });
  }
  async eval(expr) {
    const r = await this.send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  }
  invoke(cmd, args = {}) {
    return this.eval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`);
  }
  async shot(path) {
    const r = await this.send("Page.captureScreenshot", { format: "png" });
    writeFileSync(path, Buffer.from(r.data, "base64"));
  }
  /** Click a button by its text. A button that closes its own window counts as clicked. */
  async click(text, selector = "button") {
    try {
      return await this.eval(`(() => { const b = [...document.querySelectorAll(${JSON.stringify(selector)})].find(e => e.textContent.trim() === ${JSON.stringify(text)} && !e.disabled);
        if (!b) return false; b.click(); return true; })()`);
    } catch (e) {
      if (String(e).includes("window closed")) return true;
      throw e;
    }
  }
  fill(selector, value) {
    return this.eval(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return false;
      const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
      el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true })); return true; })()`);
  }
  text() {
    return this.eval("document.body.innerText");
  }
  close() {
    try {
      this.ws.close();
    } catch {
      /* already closed */
    }
  }
}

export class App {
  constructor(port) {
    this.base = `http://127.0.0.1:${port}`;
  }
  async targets() {
    const r = await fetch(`${this.base}/json`);
    return (await r.json()).filter((t) => t.type === "page" && t.url.startsWith("http://tauri.localhost"));
  }
  /** Wait for a window by its title (string) or a predicate on the DevTools target. */
  async window(match, timeout = 15000) {
    const t = await until(
      async () => (await this.targets()).find((x) => (typeof match === "string" ? x.title === match : match(x))),
      timeout,
      200,
      `window ${match}`,
    );
    const p = await new Page(t).open();
    await until(() => p.eval("document.readyState === 'complete' && (document.getElementById('root')?.childElementCount ?? 0) > 0"), 10000, 100, "window content");
    return p;
  }
  async hasWindow(title) {
    return (await this.targets()).some((t) => t.title === title);
  }
}
