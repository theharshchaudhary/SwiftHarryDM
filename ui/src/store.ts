// Live application state for a window: one full snapshot, then core events applied on top.
import { useEffect, useRef, useState } from "react";
import { api, onCore, type AppState, type CoreEvent, type Job, type Progress } from "./api";

export interface Live extends AppState {
  jobMap: Map<string, Job>;
  progressMap: Map<string, Progress>;
  /** Recent log lines per job (only collected while this window is open). */
  logs: Map<string, string[]>;
}

function toLive(s: AppState): Live {
  return {
    ...s,
    jobMap: new Map(s.jobs.map((j) => [j.id, j])),
    progressMap: new Map(s.progress.map((p) => [p.job_id, p])),
    logs: new Map(),
  };
}

function apply(l: Live, e: CoreEvent): Live {
  switch (e.type) {
    case "job_changed": {
      const jobMap = new Map(l.jobMap);
      jobMap.set(e.snapshot.id, e.snapshot);
      let progressMap = l.progressMap;
      if (!["STARTING", "DOWNLOADING", "POSTPROCESSING", "FINALIZING", "PAUSING"].includes(e.snapshot.state) && progressMap.has(e.snapshot.id)) {
        progressMap = new Map(progressMap);
        progressMap.delete(e.snapshot.id);
      }
      return { ...l, jobMap, progressMap, jobs: [...jobMap.values()] };
    }
    case "job_progress": {
      const progressMap = new Map(l.progressMap);
      progressMap.set(e.progress.job_id, e.progress);
      return { ...l, progressMap };
    }
    case "job_removed": {
      const jobMap = new Map(l.jobMap);
      jobMap.delete(e.job_id);
      const progressMap = new Map(l.progressMap);
      progressMap.delete(e.job_id);
      return { ...l, jobMap, progressMap, jobs: [...jobMap.values()] };
    }
    case "job_log": {
      const logs = new Map(l.logs);
      const lines = [...(logs.get(e.job_id) ?? []), `${new Date().toLocaleTimeString()}  ${e.message}`].slice(-200);
      logs.set(e.job_id, lines);
      return { ...l, logs };
    }
    case "settings_changed":
      return { ...l, settings: e.settings };
    case "queues_changed":
      return { ...l, queues: e.queues };
    case "quota":
      return { ...l, quota: e.quota };
    default:
      return l;
  }
}

export function useLive(): Live | null {
  const [live, setLive] = useState<Live | null>(null);
  const pending = useRef<CoreEvent[]>([]);
  useEffect(() => {
    let alive = true;
    let ready = false;
    // Subscribe first so nothing is missed between the snapshot and the first event.
    const un = onCore((e) => {
      if (!ready) {
        pending.current.push(e);
        return;
      }
      // Progress events arrive several times a second per download; batch them. (A timer,
      // not requestAnimationFrame, which never fires while the window is hidden in the tray.)
      pending.current.push(e);
      if (pending.current.length === 1) {
        setTimeout(() => {
          const batch = pending.current;
          pending.current = [];
          setLive((l) => (l ? batch.reduce(apply, l) : l));
        }, 80);
      }
    });
    api.state().then((s) => {
      if (!alive) return;
      const batch = pending.current;
      pending.current = [];
      ready = true;
      setLive(batch.reduce(apply, toLive(s)));
    });
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);
  return live;
}

/** Apply the theme setting (system/light/dark) to the document. */
export function useTheme(theme: string | undefined) {
  useEffect(() => {
    const root = document.documentElement;
    if (!theme || theme === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
  }, [theme]);
}
