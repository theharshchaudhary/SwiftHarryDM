import { useEffect, useState } from "react";
import { api, type Series } from "../api";
import { Check, Dialog, Loading, Num } from "../components/Dialog";
import { useLive, useTheme } from "../store";

/** Batch download: one address with * replaced by a run of numbers or letters. */
export default function Batch() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const [pattern, setPattern] = useState("");
  const [kind, setKind] = useState<"numbers" | "letters">("numbers");
  const [from, setFrom] = useState(1);
  const [to, setTo] = useState(10);
  const [width, setWidth] = useState(0);
  const [lfrom, setLfrom] = useState("a");
  const [lto, setLto] = useState("z");
  const [preview, setPreview] = useState<string[]>([]);
  const [err, setErr] = useState("");
  const [dir, setDir] = useState("");
  const [queueId, setQueueId] = useState("main");
  const [startQueue, setStartQueue] = useState(false);

  useEffect(() => {
    if (live && !dir) setDir(live.settings.download_dir);
  }, [live, dir]);

  const series: Series = kind === "numbers" ? { kind, from, to, width } : { kind, from: lfrom, to: lto };
  useEffect(() => {
    if (!pattern.trim()) {
      setPreview([]);
      setErr("");
      return;
    }
    const t = setTimeout(() => {
      api
        .expandBatch(pattern, series)
        .then((v) => {
          setPreview(v);
          setErr("");
        })
        .catch((e) => {
          setPreview([]);
          setErr(String(e));
        });
    }, 200);
    return () => clearTimeout(t);
  }, [pattern, kind, from, to, width, lfrom, lto]);

  if (!live) return <Loading />;

  const add = async () => {
    await api.submitMany(
      preview.map((url) => ({ url, dest_dir: dir, source: "batch", queue_id: queueId })),
      "later",
    );
    if (startQueue) await api.startQueue(queueId);
    api.closeSelf();
  };

  return (
    <Dialog
      left={<span className="hint">{preview.length ? `${preview.length} files` : ""}</span>}
      foot={
        <>
          <button className="btn primary" disabled={!preview.length || !dir} onClick={add}>Add to queue</button>
          <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <div className="hint">
        Put one <b>*</b> where the number or letter changes, for example <span className="mono">https://example.com/photos/img*.jpg</span>.
      </div>
      <div className="grid2">
        <span>Address</span>
        <input type="text" autoFocus value={pattern} onChange={(e) => setPattern(e.target.value)} />
        <span>Replace * with</span>
        <div className="row">
          <label><input type="radio" checked={kind === "numbers"} onChange={() => setKind("numbers")} /> Numbers</label>
          <label><input type="radio" checked={kind === "letters"} onChange={() => setKind("letters")} /> Letters</label>
        </div>
        {kind === "numbers" ? (
          <>
            <span>From</span>
            <div className="row">
              <Num value={from} min={0} onChange={setFrom} />
              <span>to</span>
              <Num value={to} min={0} onChange={setTo} />
              <span>digits</span>
              <Num value={width} min={0} max={12} onChange={setWidth} width={60} />
              <span className="hint">(0 = no leading zeros)</span>
            </div>
          </>
        ) : (
          <>
            <span>From</span>
            <div className="row">
              <input type="text" maxLength={1} style={{ width: 40 }} value={lfrom} onChange={(e) => setLfrom(e.target.value)} />
              <span>to</span>
              <input type="text" maxLength={1} style={{ width: 40 }} value={lto} onChange={(e) => setLto(e.target.value)} />
            </div>
          </>
        )}
        <span>Save to</span>
        <div className="row">
          <input type="text" className="grow" value={dir} onChange={(e) => setDir(e.target.value)} />
          <button className="btn small" onClick={async () => { const d = await api.pickFolder(dir); if (d) setDir(d); }}>Browse…</button>
        </div>
        <span>Queue</span>
        <div className="row">
          <select value={queueId} onChange={(e) => setQueueId(e.target.value)}>
            {live.queues.map((q) => <option key={q.id} value={q.id}>{q.name}</option>)}
          </select>
          <Check checked={startQueue} onChange={setStartQueue}>Start the queue now</Check>
        </div>
      </div>
      {err && <div className="err">{err}</div>}
      {preview.length > 0 && (
        <div className="tbl" style={{ maxHeight: 160 }}>
          <table>
            <tbody>
              {(preview.length > 6 ? [...preview.slice(0, 3), "…", ...preview.slice(-3)] : preview).map((u, i) => (
                <tr key={i}><td className="mono">{u}</td></tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Dialog>
  );
}
