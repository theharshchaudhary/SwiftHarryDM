import { useEffect, useMemo, useState } from "react";
import { api, windowParams, type Job, type UrlInfo } from "../api";
import * as F from "../format";
import { Dialog, Loading } from "../components/Dialog";
import { Icon, categoryIcon } from "../icons";
import { useLive, useTheme } from "../store";

function joinPath(dir: string, name: string) {
  return dir.replace(/[\\/]+$/, "") + "\\" + name;
}
function splitPath(p: string): [string, string] {
  const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
  return i < 0 ? ["", p] : [p.slice(0, i), p.slice(i + 1)];
}

/** "Download File Info": confirm name, category and folder, then start now or later. */
export default function FileInfo() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const [url, setUrl] = useState(params.get("url") ?? "");
  const [info, setInfo] = useState<UrlInfo | null>(null);
  const [probeErr, setProbeErr] = useState("");
  const [probing, setProbing] = useState(true);
  const [category, setCategory] = useState<string>("");
  const [path, setPath] = useState("");
  const [pathTouched, setPathTouched] = useState(false);
  const [desc, setDesc] = useState(params.get("description") ?? "");
  const [queueId, setQueueId] = useState("main");
  const rid = params.get("rid");
  // A browser request that is closed without a decision counts as cancelled.
  useEffect(() => {
    if (!rid) return;
    const off = () => api.rejectRequest(rid);
    window.addEventListener("beforeunload", off);
    return () => window.removeEventListener("beforeunload", off);
  }, [rid]);
  const [dups, setDups] = useState<Job[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  // Inspect the address (size, name, type) and look for duplicates.
  useEffect(() => {
    let alive = true;
    setProbing(true);
    setProbeErr("");
    const t = setTimeout(() => {
      api
        .inspectUrl(url)
        .then((i) => alive && setInfo(i))
        .catch((e) => alive && (setInfo(null), setProbeErr(String(e))))
        .finally(() => alive && setProbing(false));
      api.findDuplicates(url).then((d) => alive && setDups(d));
    }, 300);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [url]);

  const settings = live?.settings;
  const name = params.get("filename") || info?.file_name || "";
  const folderFor = useMemo(
    () => (cat: string) => {
      if (!settings) return "";
      const base = settings.download_dir;
      const c = settings.categories.find((x) => x.name === cat);
      if (!c || !settings.use_categories) return base;
      return /^[a-zA-Z]:[\\/]|^\\\\/.test(c.folder) ? c.folder : joinPath(base, c.folder);
    },
    [settings],
  );

  // Default category and path once the name is known (unless the user changed them).
  useEffect(() => {
    if (!settings || pathTouched) return;
    const cat = info?.category ?? "";
    setCategory(cat);
    if (name) setPath(joinPath(cat ? folderFor(cat) : settings.download_dir, name));
  }, [info, name, settings, folderFor, pathTouched]);

  if (!live || !settings) return <Loading />;

  const changeCategory = (c: string) => {
    setCategory(c);
    const [, n] = splitPath(path || name);
    setPath(joinPath(c ? folderFor(c) : settings.download_dir, n || name));
  };

  const browse = async () => {
    const [dir] = splitPath(path);
    const picked = await api.pickFolder(dir || settings.download_dir);
    if (picked) {
      setPath(joinPath(picked, splitPath(path)[1] || name));
      setPathTouched(true);
    }
  };

  const go = async (mode: "now" | "later") => {
    setErr("");
    const [dir, file] = splitPath(path);
    if (!dir) {
      setErr("Choose a folder to save to.");
      return;
    }
    setBusy(true);
    try {
      if (rid) {
        await api.resolveRequest(rid, {
          dest_dir: dir,
          filename: file || null,
          category: category || null,
          description: desc.trim() || null,
          mode,
          queue_id: mode === "later" ? queueId : null,
        });
        api.closeSelf();
        return;
      }
      await api.submit(
        {
          url,
          dest_dir: dir,
          filename: file && file !== info?.file_name ? file : file || null,
          referer: params.get("referer"),
          source: params.get("source") ?? "ui",
          category: category || null,
          description: desc.trim() || null,
          queue_id: mode === "later" ? queueId : null,
        },
        mode,
      );
      api.closeSelf();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  const dup = dups[0];
  return (
    <Dialog
      left={<span className="hint">{probing ? "Checking the address…" : info ? `${info.resumable ? "Resumable" : "Not resumable"}${info.content_type ? ` · ${info.content_type.split(";")[0]}` : ""}` : ""}</span>}
      foot={
        <>
          <button className="btn primary" disabled={busy || !url} onClick={() => go("now")}>
            Start download
          </button>
          <span className="row" style={{ gap: 0 }}>
            <button className="btn" disabled={busy || !url} onClick={() => go("later")} style={{ borderTopRightRadius: 0, borderBottomRightRadius: 0 }}>
              Download later
            </button>
            <select value={queueId} onChange={(e) => setQueueId(e.target.value)} title="Queue for “Download later”" style={{ height: 30, borderTopLeftRadius: 0, borderBottomLeftRadius: 0, marginLeft: -1 }}>
              {live.queues.map((q) => (
                <option key={q.id} value={q.id}>{q.name}</option>
              ))}
            </select>
          </span>
          <button className="btn" onClick={() => { if (rid) api.rejectRequest(rid); api.closeSelf(); }}>Cancel</button>
        </>
      }
    >
      {rid && (
        <div className="banner">
          <Icon name="link" size={18} />
          <div>Sent from your browser{params.get("referer") ? <> by <b>{F.host(params.get("referer"))}</b></> : null}. Check the details and choose how to download it.</div>
        </div>
      )}
      {dup && settings.duplicate_policy === "ask" && (
        <div className="banner warn">
          <Icon name="warning" size={18} />
          <div style={{ flex: 1 }}>
            <div><b>This address is already in your list</b> ({F.statusText(dup)}).</div>
            <div className="row" style={{ marginTop: 6, flexWrap: "wrap" }}>
              {dup.state === "COMPLETED" ? (
                <button className="btn small" onClick={() => { api.openWindow("complete", { id: dup.id }); api.closeSelf(); }}>Show the finished file</button>
              ) : (
                <button className="btn small" onClick={() => { api.resume([dup.id]); api.openWindow("progress", { id: dup.id }); api.closeSelf(); }}>Resume the existing download</button>
              )}
              <span className="hint">or continue below to add it again under a numbered name.</span>
            </div>
          </div>
        </div>
      )}
      <div className="grid2">
        <span>Address</span>
        <input type="text" value={url} onChange={(e) => setUrl(e.target.value.trim())} />
        <span>Category</span>
        <div className="row">
          <Icon name={categoryIcon(category || null)} size={18} />
          <select value={category} onChange={(e) => changeCategory(e.target.value)} className="grow">
            <option value="">General</option>
            {settings.categories.map((c) => (
              <option key={c.name} value={c.name}>{c.name}</option>
            ))}
          </select>
          <button className="btn small" title="Edit categories" onClick={() => api.openWindow("options", { tab: "saveto" })}>+</button>
        </div>
        <span>Save as</span>
        <div className="row">
          <input type="text" className="grow" value={path} placeholder={probing ? "…" : ""} onChange={(e) => { setPath(e.target.value); setPathTouched(true); }} />
          <button className="btn small" onClick={browse}>Browse…</button>
        </div>
        <span>Description</span>
        <input type="text" value={desc} onChange={(e) => setDesc(e.target.value)} />
        <span>Size</span>
        <span className="muted">
          {probing
            ? "…"
            : probeErr
              ? `Unknown: ${probeErr.replace(/\.$/, "")}. The download will still be tried (with retries).`
              : info?.size != null
                ? `${F.size(info.size)} (${info.size.toLocaleString()} bytes)`
                : "Unknown (the server did not say)"}
        </span>
      </div>
      {info?.is_web_page && (
        <div className="banner">
          <Icon name="video" size={18} />
          <div>This address opens a web page, not a file. If the page has a video, SwiftHarryDM will try to save the video instead.</div>
        </div>
      )}
      {err && <div className="err">{err}</div>}
    </Dialog>
  );
}
