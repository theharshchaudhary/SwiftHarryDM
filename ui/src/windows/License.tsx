import { useEffect, useState } from "react";
import { api, onCore, type LicenseStatus } from "../api";
import { Dialog } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

/** License status, activation with a key, and the stored license details. */
export default function License() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const [st, setSt] = useState<LicenseStatus | null>(null);
  const [info, setInfo] = useState<Record<string, unknown> | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    api.licenseStatus().then((s) => (s ? setSt(s) : api.licenseCheck().then(setSt)));
    api.licenseInfo().then(setInfo);
    const un = onCore((e) => e.type === "license" && setSt(e.license));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const activate = async () => {
    setBusy(true);
    setMsg(null);
    const r = await api.licenseActivate(key);
    setMsg({ ok: r.allowed, text: r.detail || r.text });
    setBusy(false);
    if (r.allowed) {
      setKey("");
      api.licenseInfo().then(setInfo);
    }
  };

  const hardware = st?.action === "hardware_changed";
  return (
    <Dialog
      left={
        <button className="btn" disabled={busy} onClick={async () => { setBusy(true); setSt(await api.licenseCheck()); setBusy(false); }}>
          Check again
        </button>
      }
      foot={<button className="btn primary" onClick={() => api.closeSelf()}>Close</button>}
    >
      <div className="row" style={{ gap: 12 }}>
        <div style={{ color: st?.allowed ? "var(--ok)" : "var(--warn)" }}>
          <Icon name={st?.allowed ? "finished" : "warning"} size={34} />
        </div>
        <div>
          <div style={{ fontSize: 16, fontWeight: 600 }}>{st?.text ?? "Checking…"}</div>
          {st && !st.allowed && <div className="muted">Downloads are paused until a license or trial is active.</div>}
        </div>
      </div>
      {hardware ? (
        <div className="banner warn">
          <Icon name="warning" size={18} />
          <div style={{ flex: 1 }}>
            This computer’s hardware changed since the license was activated. Reactivate it with your license key.
            <div style={{ marginTop: 6 }}>
              <button className="btn small" onClick={async () => setSt(await api.licenseReset())}>Reactivate</button>
            </div>
          </div>
        </div>
      ) : (
        <fieldset>
          <legend>Enter a license key</legend>
          <div className="row">
            <input
              type="text"
              className="grow mono"
              placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && key.trim() && activate()}
            />
            <button className="btn primary" disabled={busy || !key.trim()} onClick={activate}>Activate</button>
          </div>
          {msg && <div className={msg.ok ? "okc" : "err"} style={{ marginTop: 6 }}>{msg.text}</div>}
        </fieldset>
      )}
      {info && (
        <div className="kv">
          {Object.entries(info)
            .filter(([, v]) => v != null && v !== "")
            .map(([k, v]) => (
              <span key={k} style={{ display: "contents" }}>
                <span>{k.replace(/_/g, " ").replace(/^\w/, (c) => c.toUpperCase())}</span>
                <span className="mono">{String(v)}</span>
              </span>
            ))}
        </div>
      )}
    </Dialog>
  );
}
