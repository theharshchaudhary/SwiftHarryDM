import { useEffect, useMemo, useState } from "react";
import { api, windowParams } from "../api";
import { Check, Dialog, Loading } from "../components/Dialog";
import { useLive, useTheme } from "../store";

const TITLES: Record<string, string> = {
  clipboard: "Addresses found in the clipboard",
  import: "Addresses in the imported file",
  drop: "Dropped addresses",
  typed: "Addresses you entered",
  extension: "Links sent from the browser",
};

/** Choose which of several addresses to download (clipboard, import, drop, browser page). */
export default function Links() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const source = params.get("source") ?? "typed";
  const [urls, setUrls] = useState<string[]>(() => {
    try {
      return JSON.parse(params.get("urls") ?? "[]");
    } catch {
      return [];
    }
  });
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [dir, setDir] = useState("");
  const [byCategory, setByCategory] = useState(true);
  const [queueId, setQueueId] = useState("main");

  useEffect(() => {
    if (source === "clipboard" && urls.length === 0) api.clipboardUrls().then(setUrls);
  }, [source, urls.length]);
  useEffect(() => setChecked(new Set(urls)), [urls]);
  useEffect(() => {
    if (live && !dir) setDir(live.settings.download_dir);
  }, [live, dir]);

  const shown = useMemo(() => urls.filter((u) => !filter || u.toLowerCase().includes(filter.toLowerCase())), [urls, filter]);
  if (!live) return <Loading />;
  const picked = urls.filter((u) => checked.has(u));

  const go = async (mode: "now" | "later") => {
    const s = live.settings;
    const specs = picked.map((url) => {
      let dest = dir;
      if (byCategory) {
        const name = decodeURIComponent(new URL(url).pathname.split("/").pop() ?? "");
        const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
        const cat = s.categories.find((c) => c.extensions.includes(ext));
        if (cat && s.use_categories) dest = /^[a-zA-Z]:[\\/]/.test(cat.folder) ? cat.folder : `${s.download_dir.replace(/[\\/]+$/, "")}\\${cat.folder}`;
        else dest = s.download_dir;
      }
      return { url, dest_dir: dest, source: source === "extension" ? "extension" : "batch", queue_id: mode === "later" ? queueId : null };
    });
    await api.submitMany(specs, mode);
    api.closeSelf();
  };

  return (
    <Dialog
      left={<span className="hint">{picked.length} of {urls.length} selected</span>}
      foot={
        <>
          <button className="btn primary" disabled={!picked.length} onClick={() => go("now")}>Download now</button>
          <button className="btn" disabled={!picked.length} onClick={() => go("later")}>Add to queue</button>
          <select value={queueId} onChange={(e) => setQueueId(e.target.value)} style={{ height: 30 }}>
            {live.queues.map((q) => <option key={q.id} value={q.id}>{q.name}</option>)}
          </select>
          <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <b>{TITLES[source] ?? "Addresses"}</b>
      <div className="row">
        <input type="text" className="grow" placeholder="Filter, e.g. .zip" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <button className="btn small" onClick={() => setChecked(new Set([...checked, ...shown]))}>Check shown</button>
        <button className="btn small" onClick={() => setChecked(new Set([...checked].filter((u) => !shown.includes(u))))}>Uncheck shown</button>
      </div>
      <div className="tbl" style={{ flex: 1, minHeight: 160 }}>
        <table>
          <tbody>
            {shown.map((u) => (
              <tr key={u}>
                <td>
                  <label>
                    <input
                      type="checkbox"
                      checked={checked.has(u)}
                      onChange={(e) => {
                        const s = new Set(checked);
                        e.target.checked ? s.add(u) : s.delete(u);
                        setChecked(s);
                      }}
                    />
                    <span className="mono">{u}</span>
                  </label>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="row">
        <Check checked={byCategory} onChange={setByCategory}>Save each file to its category folder</Check>
      </div>
      {!byCategory && (
        <div className="row">
          <span>Save all to</span>
          <input type="text" className="grow" value={dir} onChange={(e) => setDir(e.target.value)} />
          <button className="btn small" onClick={async () => { const d = await api.pickFolder(dir); if (d) setDir(d); }}>Browse…</button>
        </div>
      )}
    </Dialog>
  );
}
