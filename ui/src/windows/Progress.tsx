import { useEffect, useState } from "react";
import { api, windowParams, type AfterAction, type Job, type Progress as P } from "../api";
import * as F from "../format";
import { Check, Num, Tabs } from "../components/Dialog";
import { useLive, useTheme } from "../store";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

/** Per-download progress window: status, segment map, speed limiter and what to do when done. */
export default function Progress() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const id = windowParams().get("id") ?? "";
  const job = live?.jobMap.get(id);
  const p = live?.progressMap.get(id);
  const [tab, setTab] = useState<"status" | "limit" | "done">("status");
  const [details, setDetails] = useState(true);

  useEffect(() => {
    if (job) document.title = `${pctText(job, p)} ${F.fileName(job)}`;
  }, [job, p]);
  useEffect(() => {
    getCurrentWindow().setSize(new LogicalSize(560, details ? 520 : 300));
  }, [details]);

  if (!live) return null;
  if (!job) return <div className="dlg-body muted">This download is no longer in the list.</div>;

  const running = F.isRunning(job.state);
  const total = p?.total ?? job.total;
  const done = p?.downloaded ?? job.downloaded;
  const pct = F.percent(done, total);

  return (
    <div className="dlg">
      <Tabs
        tabs={[
          ["status", "Download status"],
          ["limit", "Speed limiter"],
          ["done", "When finished"],
        ]}
        value={tab}
        onChange={setTab}
      />
      <div className="dlg-body">
        {tab === "status" && <StatusTab job={job} p={p} />}
        {tab === "limit" && <LimitTab job={job} p={p} />}
        {tab === "done" && <DoneTab job={job} />}
        <div className={`bigbar${running && pct == null ? " indet" : ""}`}>
          <div style={{ width: `${pct ?? (job.state === "COMPLETED" ? 100 : 0)}%` }} />
        </div>
        {details && tab === "status" && <Segments p={p} total={total} />}
        {details && tab === "status" && (
          <div className="logbox mono">
            {(live.logs.get(id) ?? []).map((l, i) => (
              <div key={i}>{l}</div>
            ))}
            {!(live.logs.get(id) ?? []).length && <span className="muted">Messages about this download appear here.</span>}
          </div>
        )}
      </div>
      <div className="dlg-foot">
        <div className="left">
          <button className="btn" onClick={() => setDetails(!details)}>{details ? "Hide details" : "Show details"}</button>
        </div>
        {F.canStop(job) || job.state === "PAUSING" ? (
          <button className="btn" disabled={job.state === "PAUSING"} onClick={() => api.pause([id])}>Pause</button>
        ) : (
          <button className="btn primary" disabled={!F.canResume(job)} onClick={() => api.resume([id])}>{job.state === "FAILED" ? "Retry" : "Resume"}</button>
        )}
        <button className="btn" disabled={job.state === "COMPLETED" || job.state === "CANCELLED"} onClick={() => { api.cancel([id]); api.closeSelf(); }}>
          Cancel
        </button>
        <button className="btn" onClick={() => api.closeSelf()} title="The download continues">Close</button>
      </div>
    </div>
  );
}

function pctText(j: Job, p?: P) {
  const pct = F.percent(p?.downloaded ?? j.downloaded, p?.total ?? j.total);
  return pct != null ? `${Math.floor(pct)}%` : "";
}

function StatusTab({ job, p }: { job: Job; p?: P }) {
  const total = p?.total ?? job.total;
  const done = p?.downloaded ?? job.downloaded;
  const pct = F.percent(done, total);
  return (
    <div className="kv">
      <span>Address</span>
      <span title={job.url}>{job.url}</span>
      <span>Status</span>
      <span className={job.state === "FAILED" ? "err" : ""}>{job.error && job.state !== "COMPLETED" ? job.error.message : F.statusText(job, p)}</span>
      <span>File size</span>
      <span>{total != null ? F.size(total) : "Unknown"}</span>
      <span>Downloaded</span>
      <span>
        {F.size(done)}
        {pct != null ? ` (${pct.toFixed(2)}%)` : ""}
      </span>
      <span>Transfer rate</span>
      <span>{F.isActive(job.state) ? F.speed(p?.speed) || "…" : ""}</span>
      <span>Time left</span>
      <span>{F.isActive(job.state) ? F.duration(p?.eta) || "…" : ""}</span>
      <span>Resumable</span>
      <span>{job.resumable == null ? "Unknown" : job.resumable ? "Yes" : "No: stopping it means starting over"}</span>
    </div>
  );
}

/** One bar per connection range: where each segment starts and how much of it is done. */
function Segments({ p, total }: { p?: P; total: number | null }) {
  if (!p || !total || p.segments.length === 0) return null;
  const segs = [...p.segments].sort((a, b) => a.start - b.start);
  return (
    <>
      <div className="segmap" title="Each block is one connection's part of the file">
        {segs.map((s) => (
          <div key={s.idx}>
            <div className={`seg${s.active ? " active" : ""}`} style={{ left: `${(s.start / total) * 100}%`, width: `${(s.downloaded / total) * 100}%` }} />
            {s.start > 0 && <div className="tick" style={{ left: `${(s.start / total) * 100}%` }} />}
          </div>
        ))}
      </div>
      <div className="tbl" style={{ maxHeight: 150 }}>
        <table>
          <thead>
            <tr>
              <th style={{ width: 40 }}>#</th>
              <th>Range</th>
              <th>Downloaded</th>
              <th>Connection</th>
            </tr>
          </thead>
          <tbody>
            {segs.map((s, i) => {
              const len = s.end >= 0 ? s.end - s.start + 1 : null;
              const fin = len != null && s.downloaded >= len;
              return (
                <tr key={s.idx}>
                  <td>{i + 1}</td>
                  <td className="mono">
                    {F.size(s.start)} – {s.end >= 0 ? F.size(s.end + 1) : "end"}
                  </td>
                  <td>
                    {F.size(s.downloaded)}
                    {len ? ` (${((s.downloaded / len) * 100).toFixed(0)}%)` : ""}
                  </td>
                  <td className={fin ? "okc" : ""}>{fin ? "Done" : s.active ? "Receiving data…" : "Waiting"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}

function LimitTab({ job, p }: { job: Job; p?: P }) {
  const live = useLive();
  const [kb, setKb] = useState(() => Math.round((job.speed_limit ?? 500 * 1024) / 1024));
  const on = (job.speed_limit ?? 0) > 0;
  const apply = (enabled: boolean, v: number) => {
    setKb(v);
    api.setSpeedLimit(job.id, enabled ? Math.max(1, v) * 1024 : null);
  };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <Check checked={on} onChange={(v) => apply(v, kb)}>Limit this download</Check>
      <div className="row">
        <span>Maximum speed</span>
        <Num value={kb} min={1} max={10_000_000} onChange={(v) => apply(on, v)} disabled={!on} width={110} />
        <span>KB/s</span>
      </div>
      <div className="muted">Current rate: {F.speed(p?.speed) || "—"}</div>
      {(live?.settings.speed_limit ?? 0) > 0 && <div className="hint">The global speed limiter is on as well ({F.speed(live!.settings.speed_limit)} for all downloads together).</div>}
      <div className="hint">The limit is remembered when the download is stopped and resumed. Very low limits can make some servers drop the connection.</div>
    </div>
  );
}

function DoneTab({ job }: { job: Job }) {
  const live = useLive();
  const [after, setAfter] = useState<AfterAction>("none");
  useEffect(() => {
    api.getJobAfter(job.id).then(setAfter);
  }, [job.id]);
  if (!live) return null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <Check checked={live.settings.show_complete_dialog} onChange={(v) => api.saveSettings({ ...live.settings, show_complete_dialog: v })}>
        Show the “Download complete” window
      </Check>
      <fieldset>
        <legend>When this download finishes</legend>
        {(
          [
            ["none", "Do nothing"],
            ["exit", "Exit SwiftHarryDM"],
            ["sleep", "Put the computer to sleep"],
            ["hibernate", "Hibernate the computer"],
            ["shutdown", "Shut down the computer"],
          ] as [AfterAction, string][]
        ).map(([k, l]) => (
          <div key={k}>
            <label>
              <input
                type="radio"
                name="after"
                checked={after === k}
                onChange={() => {
                  setAfter(k);
                  api.setJobAfter(job.id, k);
                }}
              />
              {l}
            </label>
          </div>
        ))}
      </fieldset>
      <div className="hint">Power actions show a 30-second warning first, which you can cancel.</div>
    </div>
  );
}
