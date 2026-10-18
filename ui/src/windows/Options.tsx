import { useEffect, useState } from "react";
import { api, windowParams, type Category, type Settings } from "../api";
import { Check, Loading, Num, Tabs } from "../components/Dialog";
import { useLive, useTheme } from "../store";

type Tab = "general" | "types" | "downloads" | "saveto" | "connection" | "proxy" | "logins" | "complete" | "browser" | "appearance";
const TABS: [Tab, string][] = [
  ["general", "General"],
  ["types", "File types"],
  ["downloads", "Downloads"],
  ["saveto", "Save to"],
  ["connection", "Connection"],
  ["proxy", "Proxy"],
  ["logins", "Site logins"],
  ["complete", "After download"],
  ["browser", "Browser"],
  ["appearance", "Appearance"],
];

/** Speed presets for "Connection type": a starting point for the number of connections. */
const PRESETS: [string, number][] = [
  ["Slow (mobile 3G, satellite)", 2],
  ["Medium (DSL, mobile 4G)", 4],
  ["Fast (cable, fibre, Wi-Fi)", 8],
  ["Very fast (gigabit fibre)", 16],
];

const lines = (v: string[]) => v.join("\n");
const unlines = (t: string) => t.split(/[\n\s]+/).map((x) => x.trim()).filter(Boolean);

export default function Options() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const [tab, setTab] = useState<Tab>((windowParams().get("tab") as Tab) ?? "general");
  const [s, setS] = useState<Settings | null>(null);
  const [err, setErr] = useState("");
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    if (live && !s) setS(live.settings);
  }, [live, s]);
  if (!live || !s) return <Loading />;

  const set = (patch: Partial<Settings>) => {
    setS({ ...s, ...patch });
    setSaved(false);
  };
  const apply = async () => {
    setErr("");
    try {
      setS(await api.saveSettings(s));
      setSaved(true);
      return true;
    } catch (e) {
      setErr(String(e));
      return false;
    }
  };

  return (
    <div className="dlg">
      <div className="split-pane">
        <Tabs vertical tabs={TABS} value={tab} onChange={setTab} />
        <div className="pane">
          {tab === "general" && <General s={s} set={set} />}
          {tab === "types" && <Types s={s} set={set} />}
          {tab === "downloads" && <Downloads s={s} set={set} />}
          {tab === "saveto" && <SaveTo s={s} set={set} />}
          {tab === "connection" && <Connection s={s} set={set} />}
          {tab === "proxy" && <Proxy s={s} set={set} />}
          {tab === "logins" && <Logins s={s} set={set} />}
          {tab === "complete" && <AfterDownload s={s} set={set} />}
          {tab === "browser" && <Browser s={s} />}
          {tab === "appearance" && <Appearance s={s} set={set} />}
        </div>
      </div>
      <div className="dlg-foot">
        <div className="left">{err ? <span className="err">{err}</span> : saved ? <span className="okc">Saved.</span> : null}</div>
        <button className="btn primary" onClick={async () => (await apply()) && api.closeSelf()}>OK</button>
        <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        <button className="btn" onClick={apply}>Apply</button>
      </div>
    </div>
  );
}

type P = { s: Settings; set: (p: Partial<Settings>) => void };

function General({ s, set }: P) {
  return (
    <>
      <fieldset>
        <legend>Startup</legend>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <Check checked={s.start_with_windows} onChange={(v) => set({ start_with_windows: v })}>Start SwiftHarryDM when Windows starts</Check>
          <Check checked={s.start_minimized} onChange={(v) => set({ start_minimized: v })}>Start minimized to the notification area</Check>
          <Check checked={s.minimize_to_tray} onChange={(v) => set({ minimize_to_tray: v })}>Closing the main window keeps SwiftHarryDM running in the notification area</Check>
        </div>
      </fieldset>
      <fieldset>
        <legend>Clipboard</legend>
        <Check checked={s.clipboard_monitor} onChange={(v) => set({ clipboard_monitor: v })}>
          Offer to download links I copy (if their file type is in the list on the “File types” page)
        </Check>
      </fieldset>
      <fieldset>
        <legend>Resuming</legend>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <Check checked={s.auto_resume} onChange={(v) => set({ auto_resume: v })}>Resume interrupted downloads when SwiftHarryDM starts</Check>
          <div className="row">
            <span>Retries for a failed download</span>
            <Num value={s.retries} min={0} max={20} onChange={(v) => set({ retries: v })} width={70} />
          </div>
        </div>
      </fieldset>
    </>
  );
}

function Types({ s, set }: P) {
  return (
    <>
      <div>
        <div>Take over browser downloads of these file types:</div>
        <textarea rows={5} style={{ width: "100%" }} className="mono" value={s.capture_extensions.join(" ")} onChange={(e) => set({ capture_extensions: unlines(e.target.value) })} />
        <div className="hint">Extensions separated by spaces, without dots. Hold the force key in the browser to take over any link.</div>
      </div>
      <div className="row">
        <span>Leave files smaller than</span>
        <Num value={s.capture_min_size_kb} min={0} onChange={(v) => set({ capture_min_size_kb: v })} />
        <span>KB to the browser (0 = any size)</span>
      </div>
      <div>
        <div>Never take over downloads from these sites:</div>
        <textarea rows={3} style={{ width: "100%" }} className="mono" value={lines(s.capture_skip_sites)} onChange={(e) => set({ capture_skip_sites: unlines(e.target.value) })} />
        <div className="hint">One per line. “example.com” also covers its subdomains; * works as a wildcard (dl*.example.com).</div>
      </div>
      <div>
        <div>Never take over downloads from these addresses:</div>
        <textarea rows={3} style={{ width: "100%" }} className="mono" value={lines(s.capture_skip_urls)} onChange={(e) => set({ capture_skip_urls: unlines(e.target.value) })} />
        <div className="hint">One pattern per line, with * as a wildcard, e.g. https://*.example.com/*.exe</div>
      </div>
    </>
  );
}

function Downloads({ s, set }: P) {
  return (
    <>
      <fieldset>
        <legend>Windows</legend>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <Check checked={s.show_download_info_dialog} onChange={(v) => set({ show_download_info_dialog: v })}>Show the “Download file info” window before a download starts</Check>
          <Check checked={s.show_progress_window} onChange={(v) => set({ show_progress_window: v })}>Show a progress window for each download I start</Check>
          <Check checked={s.show_complete_dialog} onChange={(v) => set({ show_complete_dialog: v })}>Show the “Download complete” window</Check>
        </div>
        <div className="hint" style={{ marginTop: 6 }}>Downloads started by a queue never open windows.</div>
      </fieldset>
      <fieldset>
        <legend>When the same address is added again</legend>
        {(
          [
            ["ask", "Ask me"],
            ["number", "Add it again with a numbered file name"],
            ["resume", "Resume or show the existing download"],
          ] as const
        ).map(([k, l]) => (
          <div key={k}>
            <label>
              <input type="radio" checked={s.duplicate_policy === k} onChange={() => set({ duplicate_policy: k })} /> {l}
            </label>
          </div>
        ))}
      </fieldset>
      <fieldset>
        <legend>When a file with the same name exists</legend>
        <label><input type="radio" checked={s.collision_policy === "rename"} onChange={() => set({ collision_policy: "rename" })} /> Keep both (name (1).ext)</label>
        <br />
        <label><input type="radio" checked={s.collision_policy === "overwrite"} onChange={() => set({ collision_policy: "overwrite" })} /> Replace the existing file</label>
      </fieldset>
      <div className="row">
        <span>Downloads at the same time (outside queues)</span>
        <Num value={s.max_concurrent} min={1} max={16} onChange={(v) => set({ max_concurrent: v })} width={70} />
      </div>
    </>
  );
}

function SaveTo({ s, set }: P) {
  const [cur, setCur] = useState(0);
  const cats = s.categories;
  const c = cats[cur];
  const upd = (patch: Partial<Category>) => set({ categories: cats.map((x, i) => (i === cur ? { ...x, ...patch } : x)) });
  return (
    <>
      <div className="grid2">
        <span>Download folder</span>
        <div className="row">
          <input type="text" className="grow" value={s.download_dir} onChange={(e) => set({ download_dir: e.target.value })} />
          <button className="btn small" onClick={async () => { const d = await api.pickFolder(s.download_dir); if (d) set({ download_dir: d }); }}>Browse…</button>
        </div>
      </div>
      <Check checked={s.use_categories} onChange={(v) => set({ use_categories: v })}>Sort files into category folders inside it</Check>
      <div className="row" style={{ alignItems: "stretch", opacity: s.use_categories ? 1 : 0.5 }}>
        <div className="tbl" style={{ width: 170, height: 230 }}>
          <table>
            <tbody>
              {cats.map((x, i) => (
                <tr key={i} className={i === cur ? "sel" : ""} onMouseDown={() => setCur(i)}>
                  <td>{x.name || "(no name)"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8 }}>
          {c ? (
            <>
              <div className="grid2">
                <span>Name</span>
                <input type="text" value={c.name} onChange={(e) => upd({ name: e.target.value })} />
                <span>Folder</span>
                <div className="row">
                  <input type="text" className="grow" value={c.folder} onChange={(e) => upd({ folder: e.target.value })} />
                  <button className="btn small" onClick={async () => { const d = await api.pickFolder(null); if (d) upd({ folder: d }); }}>…</button>
                </div>
              </div>
              <div>File types</div>
              <textarea rows={4} className="mono" value={c.extensions.join(" ")} onChange={(e) => upd({ extensions: unlines(e.target.value).map((x) => x.replace(/^\./, "").toLowerCase()) })} />
              <div className="hint">A plain folder name is created inside the download folder; a full path is used as it is.</div>
            </>
          ) : (
            <div className="muted">No category selected.</div>
          )}
          <div className="row">
            <button className="btn small" onClick={() => { set({ categories: [...cats, { name: "New category", extensions: [], folder: "New category" }] }); setCur(cats.length); }}>Add</button>
            <button className="btn small danger" disabled={!c} onClick={() => { set({ categories: cats.filter((_, i) => i !== cur) }); setCur(0); }}>Delete</button>
          </div>
        </div>
      </div>
    </>
  );
}

function Connection({ s, set }: P) {
  const preset = PRESETS.find(([, n]) => n === s.max_segments);
  const limitOn = s.speed_limit > 0;
  return (
    <>
      <div className="grid2">
        <span>Connection type</span>
        <select value={preset ? String(preset[1]) : ""} onChange={(e) => e.target.value && set({ max_segments: Number(e.target.value) })}>
          {!preset && <option value="">Custom</option>}
          {PRESETS.map(([l, n]) => <option key={n} value={n}>{l}</option>)}
        </select>
        <span>Connections per download</span>
        <div className="row">
          <Num value={s.max_segments} min={1} max={32} onChange={(v) => set({ max_segments: v })} width={70} />
          <span className="hint">More connections help on fast lines; some servers limit how many they accept.</span>
        </div>
      </div>
      <div>
        <div>Fewer connections for these servers:</div>
        <div className="tbl" style={{ maxHeight: 140 }}>
          <table>
            <thead><tr><th>Server (* allowed)</th><th style={{ width: 110 }}>Connections</th><th style={{ width: 40 }} /></tr></thead>
            <tbody>
              {s.server_limits.map((l, i) => (
                <tr key={i}>
                  <td><input type="text" value={l.host} onChange={(e) => set({ server_limits: s.server_limits.map((x, k) => (k === i ? { ...x, host: e.target.value } : x)) })} /></td>
                  <td><Num value={l.max_connections} min={1} max={32} onChange={(v) => set({ server_limits: s.server_limits.map((x, k) => (k === i ? { ...x, max_connections: v } : x)) })} width={70} /></td>
                  <td><button className="btn small" onClick={() => set({ server_limits: s.server_limits.filter((_, k) => k !== i) })}>×</button></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <button className="btn small" style={{ marginTop: 6 }} onClick={() => set({ server_limits: [...s.server_limits, { host: "", max_connections: 2 }] })}>Add server</button>
      </div>
      <fieldset>
        <legend>Speed limiter (all downloads together)</legend>
        <div className="row">
          <Check checked={limitOn} onChange={(v) => set({ speed_limit: v ? 500 * 1024 : 0 })}>Limit to</Check>
          <Num value={Math.round(s.speed_limit / 1024)} min={1} disabled={!limitOn} onChange={(v) => set({ speed_limit: v * 1024 })} width={100} />
          <span>KB/s</span>
        </div>
      </fieldset>
      <fieldset>
        <legend>Download quota</legend>
        <div className="row">
          <Check checked={s.quota_mb > 0} onChange={(v) => set({ quota_mb: v ? 1024 : 0 })}>Download no more than</Check>
          <Num value={s.quota_mb} min={1} disabled={s.quota_mb === 0} onChange={(v) => set({ quota_mb: v })} width={90} />
          <span>MB every</span>
          <Num value={s.quota_hours} min={1} max={744} disabled={s.quota_mb === 0} onChange={(v) => set({ quota_hours: v })} width={70} />
          <span>hours</span>
        </div>
        <div className="hint">When the quota is reached, downloads stop and continue by themselves once enough time has passed.</div>
      </fieldset>
    </>
  );
}

function Proxy({ s, set }: P) {
  return (
    <>
      <fieldset>
        <legend>Proxy</legend>
        {(
          [
            ["system", "Use the Windows proxy settings"],
            ["none", "No proxy"],
            ["manual", "Use this proxy:"],
          ] as const
        ).map(([k, l]) => (
          <div key={k}>
            <label><input type="radio" checked={s.proxy_mode === k} onChange={() => set({ proxy_mode: k })} /> {l}</label>
          </div>
        ))}
        <input type="text" style={{ width: "100%", marginTop: 6 }} disabled={s.proxy_mode !== "manual"} placeholder="http://host:port  or  socks5://user:pass@host:port" value={s.proxy_url} onChange={(e) => set({ proxy_url: e.target.value })} />
      </fieldset>
      <div className="grid2">
        <span>Browser identification</span>
        <input type="text" className="mono" value={s.user_agent} onChange={(e) => set({ user_agent: e.target.value })} />
        <span />
        <span className="hint">The User-Agent sent to servers. Leave the default unless a site requires something else.</span>
      </div>
    </>
  );
}

function Logins({ s, set }: P) {
  const [host, setHost] = useState("");
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [err, setErr] = useState("");
  const add = async () => {
    setErr("");
    try {
      const enc = await api.protectPassword(pass);
      set({ site_logins: [...s.site_logins.filter((l) => l.host !== host.trim()), { host: host.trim(), username: user, password_enc: enc }] });
      setHost("");
      setUser("");
      setPass("");
    } catch (e) {
      setErr(String(e));
    }
  };
  return (
    <>
      <div>User names and passwords for servers that ask for them. Passwords are encrypted with your Windows account.</div>
      <div className="tbl" style={{ maxHeight: 200 }}>
        <table>
          <thead><tr><th>Server</th><th>User name</th><th style={{ width: 40 }} /></tr></thead>
          <tbody>
            {s.site_logins.map((l) => (
              <tr key={l.host}>
                <td>{l.host}</td>
                <td>{l.username}</td>
                <td><button className="btn small" onClick={() => set({ site_logins: s.site_logins.filter((x) => x.host !== l.host) })}>×</button></td>
              </tr>
            ))}
            {!s.site_logins.length && <tr><td colSpan={3} className="muted">No logins saved.</td></tr>}
          </tbody>
        </table>
      </div>
      <fieldset>
        <legend>Add a login</legend>
        <div className="grid2">
          <span>Server</span>
          <input type="text" placeholder="files.example.com or *.example.com" value={host} onChange={(e) => setHost(e.target.value)} />
          <span>User name</span>
          <input type="text" value={user} onChange={(e) => setUser(e.target.value)} />
          <span>Password</span>
          <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} />
        </div>
        <button className="btn small" style={{ marginTop: 8 }} disabled={!host.trim() || !user} onClick={add}>Add</button>
        {err && <div className="err">{err}</div>}
      </fieldset>
    </>
  );
}

function AfterDownload({ s, set }: P) {
  return (
    <>
      <Check checked={s.sound_on_complete} onChange={(v) => set({ sound_on_complete: v })}>Play a sound when a download finishes</Check>
      <fieldset>
        <legend>Virus check</legend>
        <Check checked={s.av_scan_enabled} onChange={(v) => set({ av_scan_enabled: v })}>Scan each finished file with this program</Check>
        <input type="text" className="mono" style={{ width: "100%", marginTop: 6 }} disabled={!s.av_scan_enabled} value={s.av_scan_command} onChange={(e) => set({ av_scan_command: e.target.value })} />
        <div className="hint">{"{file}"} is replaced with the path of the downloaded file. The default uses Microsoft Defender.</div>
      </fieldset>
    </>
  );
}

function Browser({ s }: { s: Settings }) {
  return (
    <>
      <div>
        The SwiftHarryDM browser extension sends downloads, links and videos from Chrome and Edge to this app. It connects to
        port <b>{s.api_port}</b> on this computer and must be paired once.
      </div>
      <ol style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }}>
        <li>Install the SwiftHarryDM extension in your browser.</li>
        <li>Click its toolbar button and choose <b>Connect to app</b>.</li>
        <li>Type the 6-digit code that SwiftHarryDM shows.</li>
      </ol>
      <div className="hint">Which downloads are taken over is set on the “File types” page.</div>
    </>
  );
}

function Appearance({ s, set }: P) {
  return (
    <fieldset>
      <legend>Theme</legend>
      {(
        [
          ["system", "Same as Windows"],
          ["light", "Light"],
          ["dark", "Dark"],
        ] as const
      ).map(([k, l]) => (
        <div key={k}>
          <label><input type="radio" checked={s.theme === k} onChange={() => set({ theme: k })} /> {l}</label>
        </div>
      ))}
    </fieldset>
  );
}
