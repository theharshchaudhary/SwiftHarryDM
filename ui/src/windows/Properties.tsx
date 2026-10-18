import { useEffect, useRef, useState } from "react";
import { api, windowParams } from "../api";
import * as F from "../format";
import { Dialog, Loading } from "../components/Dialog";
import { useLive, useTheme } from "../store";

/** File properties: details, editable description, and a new address for an unfinished download. */
export default function Properties() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const id = params.get("id") ?? "";
  const j = live?.jobMap.get(id);
  const [desc, setDesc] = useState<string | null>(null);
  const [addr, setAddr] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const addrRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (j && desc === null) setDesc(j.description ?? "");
    if (j && addr === null) setAddr(j.url);
  }, [j, desc, addr]);
  useEffect(() => {
    if (params.get("focus") === "address") setTimeout(() => addrRef.current?.select(), 50);
  }, [params]);
  if (!live) return <Loading />;
  if (!j) return <div className="dlg-body muted">This download is no longer in the list.</div>;

  const finished = j.state === "COMPLETED" || j.state === "CANCELLED";
  const addrChanged = addr != null && addr.trim() !== j.url;

  const save = async () => {
    setMsg(null);
    setBusy(true);
    try {
      if ((desc ?? "") !== (j.description ?? "")) await api.setDescription(id, desc?.trim() ? desc.trim() : null);
      if (addrChanged && addr) {
        if (F.isRunning(j.state)) await api.pause([id]);
        await api.refreshUrl(id, addr.trim());
        setMsg({ ok: true, text: "The new address serves the same file. Resume the download to continue." });
        setBusy(false);
        return;
      }
      api.closeSelf();
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
      setBusy(false);
    }
  };

  return (
    <Dialog
      left={
        j.state === "COMPLETED" ? (
          <>
            <button className="btn" onClick={() => api.openFile(id)}>Open</button>
            <button className="btn" onClick={() => api.openFolder(id)}>Open folder</button>
          </>
        ) : undefined
      }
      foot={
        <>
          <button className="btn primary" disabled={busy} onClick={save}>OK</button>
          <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <div className="kv">
        <span>File name</span>
        <b>{F.fileName(j)}</b>
        <span>Type</span>
        <span>{j.kind === "media" ? "Video/audio" : F.extension(F.fileName(j)).toUpperCase() || "File"}{j.category ? ` · ${j.category}` : ""}</span>
        <span>Status</span>
        <span className={j.state === "FAILED" ? "err" : ""}>{F.statusText(j)}</span>
        <span>Size</span>
        <span>{j.total != null ? `${F.size(j.total)} (${j.total.toLocaleString()} bytes)` : "Unknown"}</span>
        <span>Save to</span>
        <span title={j.final_path ?? j.dest_dir}>{j.final_path ?? j.dest_dir}</span>
        <span>Added</span>
        <span>{F.date(j.created_at)}</span>
        <span>Last try</span>
        <span>{F.date(j.last_try)}</span>
        {j.error && (
          <>
            <span>Last error</span>
            <span className="err" title={j.error.detail}>{j.error.message}</span>
          </>
        )}
        {j.final_url && j.final_url !== j.url && (
          <>
            <span>Redirected to</span>
            <span title={j.final_url}>{j.final_url}</span>
          </>
        )}
        <span>Web page</span>
        <span title={j.referer ?? ""}>{j.referer ?? "Unknown"}</span>
      </div>
      <div className="grid2">
        <span>Address</span>
        <input ref={addrRef} type="text" value={addr ?? ""} readOnly={finished} onChange={(e) => setAddr(e.target.value)} />
        {!finished && (
          <span className="full hint">
            If the link expired, paste a fresh one for the same file here. The downloaded part is kept only if the new link serves the same file.
          </span>
        )}
        <span>Description</span>
        <input type="text" value={desc ?? ""} onChange={(e) => setDesc(e.target.value)} />
      </div>
      {msg && <div className={msg.ok ? "okc" : "err"}>{msg.text}</div>}
    </Dialog>
  );
}
