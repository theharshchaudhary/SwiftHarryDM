import { useEffect, useMemo, useState } from "react";
import { api, windowParams, type AfterAction, type Queue } from "../api";
import * as F from "../format";
import { Check, Loading, Num, Tabs } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function newQueue(n: number): Queue {
  return {
    id: `q${Date.now().toString(36)}`,
    name: `Queue ${n}`,
    max_concurrent: 2,
    running: false,
    schedule: null,
    after: "none",
  };
}

/** Queues: their files, how many run at once, start/stop times and what happens at the end. */
export default function Scheduler() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const [curId, setCurId] = useState<string>(params.get("queue") && params.get("queue") !== "new" ? params.get("queue")! : "main");
  const [draft, setDraft] = useState<Queue | null>(null);
  const [tab, setTab] = useState<"schedule" | "files">("schedule");
  const [dirty, setDirty] = useState(false);
  const [fileSel, setFileSel] = useState<string | null>(null);

  const cur = live?.queues.find((q) => q.id === curId) ?? null;
  useEffect(() => {
    if (cur && (!draft || draft.id !== cur.id)) {
      setDraft(cur);
      setDirty(false);
    }
  }, [cur, draft]);
  // Keep the running flag live while editing.
  useEffect(() => {
    if (cur && draft && draft.id === cur.id && draft.running !== cur.running) setDraft({ ...draft, running: cur.running });
  }, [cur, draft]);
  useEffect(() => {
    if (live && params.get("queue") === "new") {
      const q = newQueue(live.queues.length + 1);
      api.saveQueue(q).then(() => setCurId(q.id));
    }
    // Only once, when the state first arrives and the window was opened with queue=new.
  }, [live === null]);

  const files = useMemo(() => {
    if (!live) return [];
    return live.jobs
      .filter((j) => j.queue_id === curId && j.state !== "COMPLETED" && j.state !== "CANCELLED")
      .sort((a, b) => b.priority - a.priority || a.created_at - b.created_at);
  }, [live, curId]);

  if (!live || !draft) return <Loading />;

  const upd = (p: Partial<Queue>) => {
    setDraft({ ...draft, ...p });
    setDirty(true);
  };
  const sched = draft.schedule ?? { enabled: false, start_time: "02:00", days: [true, true, true, true, true, true, true], stop_time: null };
  const updS = (p: Partial<typeof sched>) => upd({ schedule: { ...sched, ...p } });
  const apply = async () => {
    await api.saveQueue(draft);
    setDirty(false);
  };
  const switchTo = async (id: string) => {
    if (dirty) await apply();
    setCurId(id);
    setDraft(null);
  };

  // Reorder by rewriting priorities: the first file gets the highest.
  const move = (id: string, d: number) => {
    const order = files.map((j) => j.id);
    const i = order.indexOf(id);
    const k = i + d;
    if (i < 0 || k < 0 || k >= order.length) return;
    [order[i], order[k]] = [order[k], order[i]];
    order.forEach((jid, n) => api.setPriority(jid, order.length - n));
  };

  return (
    <div className="dlg">
      <div className="split-pane">
        <div style={{ width: 190, borderRight: "1px solid var(--border)", display: "flex", flexDirection: "column", background: "var(--surface-2)" }}>
          <div style={{ padding: "10px 10px 4px", fontWeight: 600 }}>Queues</div>
          <div style={{ flex: 1, overflow: "auto", padding: "0 6px" }}>
            {live.queues.map((q) => (
              <div key={q.id} className={`tree-item${q.id === curId ? " sel" : ""}`} onMouseDown={() => switchTo(q.id)}>
                <Icon className="ico" name="queue" size={16} />
                <span className="ellipsis">{q.name}</span>
                {q.running && <span className="running-dot" title="Running" />}
              </div>
            ))}
          </div>
          <div className="row" style={{ padding: 8 }}>
            <button className="btn small" onClick={async () => { const q = newQueue(live.queues.length + 1); await api.saveQueue(q); switchTo(q.id); }}>New</button>
            <button className="btn small danger" disabled={curId === "main"} onClick={() => { api.deleteQueue(curId); setCurId("main"); setDraft(null); }}>Delete</button>
          </div>
        </div>
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div className="row" style={{ padding: "10px 16px" }}>
            <span>Name</span>
            <input type="text" className="grow" value={draft.name} disabled={draft.id === "main"} onChange={(e) => upd({ name: e.target.value })} />
            {draft.running ? (
              <button className="btn small" onClick={() => api.stopQueue(draft.id)}>Stop</button>
            ) : (
              <button className="btn small primary" onClick={async () => { if (dirty) await apply(); api.startQueue(draft.id); }}>Start now</button>
            )}
          </div>
          <Tabs tabs={[["schedule", "Schedule"], ["files", `Files in the queue (${files.length})`]]} value={tab} onChange={setTab} />
          <div className="pane" style={{ padding: "14px 16px", overflow: "auto", flex: 1, display: "flex", flexDirection: "column", gap: 12 }}>
            {tab === "schedule" ? (
              <>
                <Check checked={sched.enabled} onChange={(v) => updS({ enabled: v })}>Start this queue automatically</Check>
                <div className="grid2" style={{ opacity: sched.enabled ? 1 : 0.55 }}>
                  <span>Start at</span>
                  <input type="time" value={sched.start_time} disabled={!sched.enabled} onChange={(e) => updS({ start_time: e.target.value })} style={{ width: 120 }} />
                  <span>On</span>
                  <div className="days">
                    {DAYS.map((d, i) => (
                      <label key={d}>
                        <input type="checkbox" disabled={!sched.enabled} checked={sched.days[i]} onChange={(e) => updS({ days: sched.days.map((x, k) => (k === i ? e.target.checked : x)) })} />
                        {d}
                      </label>
                    ))}
                  </div>
                  <span>Stop at</span>
                  <div className="row">
                    <Check checked={sched.stop_time != null} disabled={!sched.enabled} onChange={(v) => updS({ stop_time: v ? "07:00" : null })}>
                      {""}
                    </Check>
                    <input type="time" value={sched.stop_time ?? "07:00"} disabled={!sched.enabled || sched.stop_time == null} onChange={(e) => updS({ stop_time: e.target.value })} style={{ width: 120 }} />
                  </div>
                </div>
                <div className="row">
                  <span>Download</span>
                  <Num value={draft.max_concurrent} min={1} max={16} onChange={(v) => upd({ max_concurrent: v })} width={64} />
                  <span>files at the same time</span>
                </div>
                <div className="grid2">
                  <span>When the queue finishes</span>
                  <select value={draft.after} onChange={(e) => upd({ after: e.target.value as AfterAction })}>
                    <option value="none">Do nothing</option>
                    <option value="exit">Exit SwiftHarryDM</option>
                    <option value="sleep">Put the computer to sleep</option>
                    <option value="hibernate">Hibernate the computer</option>
                    <option value="shutdown">Shut down the computer</option>
                  </select>
                </div>
                <div className="hint">
                  SwiftHarryDM must be running (it can sit in the notification area) for a queue to start on time. To start while the computer sleeps, allow wake timers in
                  the Windows power options.
                </div>
              </>
            ) : (
              <>
                <div className="row" style={{ alignItems: "stretch", flex: 1, minHeight: 0 }}>
                  <div className="tbl grow">
                    <table>
                      <thead><tr><th>File</th><th style={{ width: 90 }}>Size</th><th style={{ width: 110 }}>Status</th></tr></thead>
                      <tbody>
                        {files.map((j) => (
                          <tr key={j.id} className={fileSel === j.id ? "sel" : ""} onMouseDown={() => setFileSel(j.id)}>
                            <td>{F.fileName(j)}</td>
                            <td>{F.size(j.total)}</td>
                            <td>{F.statusText(j, live.progressMap.get(j.id))}</td>
                          </tr>
                        ))}
                        {!files.length && <tr><td colSpan={3} className="muted">No files. Use “Download later” or “Move to queue” to add some.</td></tr>}
                      </tbody>
                    </table>
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                    <button className="btn small" title="Move up" disabled={!fileSel} onClick={() => fileSel && move(fileSel, -1)}><Icon name="up" size={14} /></button>
                    <button className="btn small" title="Move down" disabled={!fileSel} onClick={() => fileSel && move(fileSel, 1)}><Icon name="down" size={14} /></button>
                    <button className="btn small" title="Remove from the queue" disabled={!fileSel} onClick={() => fileSel && api.moveToQueue([fileSel], null)}><Icon name="close" size={14} /></button>
                  </div>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
      <div className="dlg-foot">
        <div className="left">{dirty && <span className="hint">Unsaved changes</span>}</div>
        <button className="btn primary" disabled={!dirty} onClick={apply}>Apply</button>
        <button className="btn" onClick={async () => { if (dirty) await apply(); api.closeSelf(); }}>Close</button>
      </div>
    </div>
  );
}
