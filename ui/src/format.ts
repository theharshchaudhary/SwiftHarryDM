import type { Job, JobState, Progress } from "./api";

export function size(n: number | null | undefined): string {
  if (n == null) return "";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n;
  let i = -1;
  do {
    v /= 1024;
    i++;
  } while (v >= 1024 && i < units.length - 1);
  return `${v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${units[i]}`;
}

export function speed(bps: number | null | undefined): string {
  if (bps == null || !isFinite(bps)) return "";
  return `${size(Math.round(bps))}/s`;
}

export function duration(secs: number | null | undefined): string {
  if (secs == null || !isFinite(secs)) return "";
  const s = Math.max(0, Math.round(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 0) return `${h} h ${m} min`;
  if (m > 0) return `${m} min ${r} s`;
  return `${r} s`;
}

export function date(epochSecs: number | null | undefined): string {
  if (!epochSecs) return "";
  const d = new Date(epochSecs * 1000);
  return d.toLocaleString(undefined, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

export function percent(done: number, total: number | null | undefined): number | null {
  if (!total) return null;
  return Math.min(100, (done / total) * 100);
}

const STATE_TEXT: Record<JobState, string> = {
  QUEUED: "Waiting",
  STARTING: "Connecting…",
  DOWNLOADING: "Downloading",
  POSTPROCESSING: "Processing…",
  FINALIZING: "Finishing…",
  COMPLETED: "Complete",
  PAUSING: "Stopping…",
  PAUSED: "Stopped",
  RETRY_WAIT: "Retrying soon",
  CANCELLING: "Cancelling…",
  CANCELLED: "Cancelled",
  FAILED: "Error",
};

/** Text for the Status column: a percentage while transferring, otherwise a word. */
export function statusText(j: Job, p?: Progress): string {
  if (j.state === "DOWNLOADING" || j.state === "STARTING") {
    const pct = percent(p?.downloaded ?? j.downloaded, p?.total ?? j.total);
    if (j.state === "DOWNLOADING" && pct != null) return `${pct.toFixed(2)}%`;
    if (j.state === "DOWNLOADING") return size(p?.downloaded ?? j.downloaded);
  }
  if (j.state === "PAUSED") {
    if (j.reason === "queued") return "Queued";
    if (j.reason === "disk_full") return "Disk full";
    if (j.reason === "interrupted") return "Interrupted";
    const pct = percent(j.downloaded, j.total);
    return pct != null && pct > 0 ? `Stopped (${pct.toFixed(1)}%)` : "Stopped";
  }
  if (j.state === "RETRY_WAIT" && j.reason) return `Retry: ${j.reason.replace("retry in ", "in ")}`;
  return STATE_TEXT[j.state];
}

export function isActive(s: JobState): boolean {
  return s === "STARTING" || s === "DOWNLOADING" || s === "POSTPROCESSING" || s === "FINALIZING";
}

export function isRunning(s: JobState): boolean {
  return isActive(s) || s === "PAUSING" || s === "CANCELLING";
}

export function canResume(j: Job): boolean {
  return j.state === "PAUSED" || j.state === "FAILED" || j.state === "QUEUED" || j.state === "RETRY_WAIT";
}

export function canStop(j: Job): boolean {
  return isActive(j.state) || j.state === "QUEUED" || j.state === "RETRY_WAIT";
}

export function fileName(j: Job): string {
  return j.file_name ?? j.title;
}

export function extension(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

export function host(url: string | null | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return "";
  }
}
