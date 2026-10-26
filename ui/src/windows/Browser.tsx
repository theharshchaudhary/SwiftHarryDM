import { useEffect, useState } from "react";
import { api, onCore, type ApiStatus, type ExtensionSetup } from "../api";
import { Dialog } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

/** First run and Help: add the browser extension. It connects to the app by itself. */
export default function Browser() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const [st, setSt] = useState<ApiStatus | null>(null);
  const [setup, setSetup] = useState<(ExtensionSetup & { browser: string }) | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    const load = () => api.apiStatus().then(setSt);
    load();
    const t = setInterval(load, 2000);
    const un = onCore((e) => e.type === "extension_changed" && load());
    return () => {
      clearInterval(t);
      un.then((f) => f());
    };
  }, []);
  const connected = (st?.paired.length ?? 0) > 0;

  const go = async (browser: "chrome" | "edge") => {
    setErr("");
    try {
      setSetup({ ...(await api.extensionSetup(browser)), browser });
    } catch (e) {
      setErr(String(e));
    }
  };
  const name = setup?.browser === "edge" ? "Edge" : "Chrome";
  const page = setup?.browser === "edge" ? "edge://extensions" : "chrome://extensions";

  return (
    <Dialog
      left={
        <label>
          <input type="checkbox" onChange={(e) => { try { localStorage.setItem("sdm.browserSetupDone", e.target.checked ? "1" : ""); } catch { /* ignore */ } }} />
          Don’t show this at startup
        </label>
      }
      foot={<button className="btn primary" onClick={() => api.closeSelf()}>{connected ? "Done" : "Close"}</button>}
    >
      <div className="row" style={{ gap: 12 }}>
        <div style={{ color: connected ? "var(--ok)" : "var(--accent)" }}>
          <Icon name={connected ? "finished" : "link"} size={36} />
        </div>
        <div>
          <div style={{ fontSize: 16, fontWeight: 600 }}>{connected ? "Your browser is connected" : "Connect your browser"}</div>
          <div className="muted">
            {connected
              ? "Downloads, links and videos from the browser now go to Keepfetch."
              : "Add the Keepfetch extension so downloads, links and videos from your browser go to the app. It connects by itself, with nothing to type."}
          </div>
        </div>
      </div>
      {!connected && (
        <div className="row">
          <button className="btn" onClick={() => go("chrome")}>Add to Chrome</button>
          <button className="btn" onClick={() => go("edge")}>Add to Edge</button>
        </div>
      )}
      {setup?.mode === "store" && !connected && <div className="hint">The extension page opened in {name}. Click “Add”, then come back here.</div>}
      {setup?.mode === "manual" && !connected && (
        <ol style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }}>
          <li>{setup.browser_found ? <>{name} opened its extensions page.</> : <>Open <span className="mono">{page}</span> in {name}.</>} Turn on <b>Developer mode</b> (top right).</li>
          <li>Click <b>Load unpacked</b> and choose this folder (it opened in Explorer):<div className="mono selectable" style={{ marginTop: 2 }}>{setup.folder}</div></li>
          <li>That’s it: the extension connects to Keepfetch by itself and this window turns green.</li>
        </ol>
      )}
      {err && <div className="err">{err}</div>}
      {st && !st.port && <div className="err">The browser connection is not running: {st.error}</div>}
    </Dialog>
  );
}
