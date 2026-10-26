import { useEffect, useMemo, useState } from "react";
import { api, windowParams, type Settings } from "../api";
import * as F from "../format";
import { Check, Dialog, Loading } from "../components/Dialog";
import { useLive, useTheme } from "../store";

const TITLES: Record<string, string> = {
  clipboard: "Addresses found in the clipboard",
  import: "Addresses in the imported file",
  drop: "Dropped addresses",
  typed: "Addresses you entered",
  extension: "Links on the web page",
};

const HTML = new Set(["", "htm", "html", "shtml", "xhtml", "php", "asp", "aspx", "jsp", "cgi", "pl", "cfm"]);
const IMAGES = new Set(["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "ico", "avif", "tif", "tiff"]);

type Row = {
  url: string;
  name: string;
  ext: string;
  text: string;
  /** undefined while checking, null when the server did not say */
  size?: number | null;
  type?: string;
  page?: boolean;
};

function json<T>(s: string | null, fallback: T): T {
  try {
    return s ? (JSON.parse(s) as T) : fallback;
  } catch {
    return fallback;
  }
}

function extOf(name: string) {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

function categoryFolder(s: Settings, cat: string | null): string {
  const c = cat ? s.categories.find((x) => x.name === cat) : null;
  if (!c || !s.use_categories) return s.download_dir;
  return /^[a-zA-Z]:[\\/]|^\\\\/.test(c.folder) ? c.folder : `${s.download_dir.replace(/[\\/]+$/, "")}\\${c.folder}`;
}

/** "Download all links": choose which of several addresses to download, with their types
 * and sizes, filters, and where to save them. Also used for the clipboard, imports and drops. */
export default function Links() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const source = params.get("source") ?? "typed";
  const texts = useMemo(() => json<Record<string, string>>(params.get("texts"), {}), [params]);
  const referers = useMemo(() => json<Record<string, string>>(params.get("referers"), {}), [params]);
  const [rows, setRows] = useState<Row[]>(() =>
    json<string[]>(params.get("urls"), []).map((url) => {
      const name = F.nameFromUrl(url);
      return { url, name, ext: extOf(name), text: texts[url] ?? "" };
    }),
  );
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [hideHtml, setHideHtml] = useState(true);
  const [hideImages, setHideImages] = useState(source === "extension");
  const [hideRepeated, setHideRepeated] = useState(true);
  const [useTexts, setUseTexts] = useState(true);
  const [saveMode, setSaveMode] = useState<"category" | "onecat" | "onedir">("category");
  const [oneCat, setOneCat] = useState("");
  const [dir, setDir] = useState("");
  const [pattern, setPattern] = useState("");
  const [queueId, setQueueId] = useState("main");

  // The clipboard list is read when the window opens.
  useEffect(() => {
    if (source === "clipboard" && rows.length === 0)
      api.clipboardUrls().then((urls) => setRows(urls.map((url) => ({ url, name: F.nameFromUrl(url), ext: extOf(F.nameFromUrl(url)), text: "" }))));
  }, [source, rows.length]);

  // Check every address for its real name, type and size, a few at a time.
  const urlsKey = rows.map((r) => r.url).join("\n");
  useEffect(() => {
    let alive = true;
    const queue = rows.filter((r) => r.size === undefined).map((r) => r.url);
    const worker = async () => {
      for (let u = queue.shift(); u && alive; u = queue.shift()) {
        const url = u;
        const info = await api.inspectUrl(url).catch(() => null);
        if (!alive) return;
        setRows((rs) =>
          rs.map((r) =>
            r.url !== url
              ? r
              : info
                ? { ...r, name: info.file_name || r.name, ext: extOf(info.file_name || r.name), size: info.size, type: info.content_type?.split(";")[0] ?? "", page: info.is_web_page }
                : { ...r, size: null },
          ),
        );
      }
    };
    for (let i = 0; i < 4; i++) worker();
    return () => {
      alive = false;
    };
  }, [urlsKey]);

  useEffect(() => setChecked(new Set(rows.map((r) => r.url))), [urlsKey]);
  useEffect(() => {
    if (live && !dir) setDir(live.settings.download_dir);
  }, [live, dir]);

  const shown = useMemo(() => {
    const seen = new Set<string>();
    return rows.filter((r) => {
      if (hideHtml && (r.page || (r.page === undefined && HTML.has(r.ext)))) return false;
      if (hideImages && IMAGES.has(r.ext)) return false;
      if (hideRepeated) {
        const k = r.name.toLowerCase();
        if (seen.has(k)) return false;
        seen.add(k);
      }
      const f = filter.toLowerCase();
      return !f || r.url.toLowerCase().includes(f) || r.name.toLowerCase().includes(f) || r.text.toLowerCase().includes(f);
    });
  }, [rows, hideHtml, hideImages, hideRepeated, filter]);

  if (!live) return <Loading />;
  const s = live.settings;
  const picked = shown.filter((r) => checked.has(r.url));
  const checking = rows.filter((r) => r.size === undefined).length;

  const go = async (mode: "now" | "later") => {
    const width = String(picked.length).length;
    const specs = picked.map((r, i) => {
      const cat = saveMode === "onecat" ? oneCat || null : s.categories.find((c) => c.extensions.includes(r.ext))?.name ?? null;
      const dest = saveMode === "onedir" ? dir : categoryFolder(s, cat);
      // "Replace file names using the asterisk pattern": pic*.jpg -> pic01.jpg, pic02.jpg, ...
      const filename = pattern.includes("*") ? pattern.replace("*", String(i + 1).padStart(width, "0")) : null;
      return {
        url: r.url,
        dest_dir: dest,
        filename,
        category: cat,
        description: useTexts && r.text ? r.text : null,
        referer: referers[r.url] ?? params.get("referer"),
        source: source === "extension" ? "extension" : "batch",
        queue_id: mode === "later" ? queueId : null,
      };
    });
    await api.submitMany(specs, mode);
    api.closeSelf();
  };

  return (
    <Dialog
      left={<span className="hint">{picked.length} of {shown.length} checked{checking ? ` · checking ${checking}…` : ""}</span>}
      foot={
        <>
          <button className="btn primary" disabled={!picked.length} onClick={() => go("now")}>Download now</button>
          <button className="btn" disabled={!picked.length} onClick={() => go("later")}>Add to queue</button>
          <select aria-label="Queue for Download later" value={queueId} onChange={(e) => setQueueId(e.target.value)} style={{ height: 30 }}>
            {live.queues.map((q) => <option key={q.id} value={q.id}>{q.name}</option>)}
          </select>
          <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <b>{TITLES[source] ?? "Addresses"}</b>
      <div className="row">
        <input type="text" className="grow" placeholder="Filter, e.g. .zip" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <button className="btn small" onClick={() => setChecked(new Set([...checked, ...shown.map((r) => r.url)]))}>Check all</button>
        <button className="btn small" onClick={() => setChecked(new Set([...checked].filter((u) => !shown.some((r) => r.url === u))))}>Uncheck all</button>
      </div>
      <div className="tbl" style={{ flex: 1, minHeight: 160 }}>
        <table>
          <thead>
            <tr>
              <th style={{ width: 26 }} />
              <th>File name</th>
              <th style={{ width: 70 }}>Type</th>
              <th style={{ width: 80, textAlign: "right" }}>Size</th>
              <th>Description</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.url} title={r.url}>
                <td>
                  <input
                    type="checkbox"
                    checked={checked.has(r.url)}
                    onChange={(e) => {
                      const c = new Set(checked);
                      e.target.checked ? c.add(r.url) : c.delete(r.url);
                      setChecked(c);
                    }}
                  />
                </td>
                <td className="ellipsis">{r.name}</td>
                <td className="muted">{r.ext || (r.type ?? "")}</td>
                <td className="muted" style={{ textAlign: "right" }}>{r.size === undefined ? "…" : r.size == null ? "?" : F.size(r.size)}</td>
                <td className="ellipsis muted">{r.text}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="row" style={{ flexWrap: "wrap", gap: 14 }}>
        <Check checked={hideHtml} onChange={setHideHtml}>Hide web pages</Check>
        <Check checked={hideImages} onChange={setHideImages}>Hide images</Check>
        <Check checked={hideRepeated} onChange={setHideRepeated}>Hide repeated files</Check>
        <Check checked={useTexts} onChange={setUseTexts}>Use link texts as descriptions</Check>
      </div>
      <fieldset>
        <legend>Save to</legend>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <label className="row"><input type="radio" checked={saveMode === "category"} onChange={() => setSaveMode("category")} /> Each file to the folder of its category</label>
          <label className="row">
            <input type="radio" checked={saveMode === "onecat"} onChange={() => setSaveMode("onecat")} /> All files to one category:
            <select value={oneCat} disabled={saveMode !== "onecat"} onChange={(e) => setOneCat(e.target.value)}>
              <option value="">General</option>
              {s.categories.map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}
            </select>
          </label>
          <label className="row">
            <input type="radio" checked={saveMode === "onedir"} onChange={() => setSaveMode("onedir")} /> All files to one folder:
            <input type="text" className="grow" disabled={saveMode !== "onedir"} value={dir} onChange={(e) => setDir(e.target.value)} />
            <button className="btn small" disabled={saveMode !== "onedir"} onClick={async () => { const d = await api.pickFolder(dir); if (d) setDir(d); }}>Browse…</button>
          </label>
        </div>
      </fieldset>
      <div className="row">
        <span>Rename with a pattern</span>
        <input type="text" className="grow" placeholder="Optional, e.g. photo*.jpg gives photo01.jpg, photo02.jpg, …" value={pattern} onChange={(e) => setPattern(e.target.value)} />
      </div>
    </Dialog>
  );
}
