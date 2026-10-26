import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onCore, type Job, type Progress, type Queue, type UpdateInfo } from "../api";
import * as F from "../format";
import { Icon, categoryIcon, fileIcon } from "../icons";
import { ContextMenu, MenuBar, type MenuEntry, type MenuRoot } from "../components/Menu";
import { Check, Modal } from "../components/Dialog";
import { useLive, useTheme, type Live } from "../store";

// ------------------------------------------------------------------ persisted view prefs
function pref<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(`sdm.${key}`);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}
function savePref(key: string, v: unknown) {
  try {
    localStorage.setItem(`sdm.${key}`, JSON.stringify(v));
  } catch {
    /* storage unavailable: preferences just are not remembered */
  }
}

// ------------------------------------------------------------------ columns
type Row = { j: Job; p?: Progress; qIndex: number; queue?: Queue };
interface Column {
  id: string;
  title: string;
  width: number;
  render: (r: Row) => React.ReactNode;
  sort: (r: Row) => string | number;
  cls?: string;
}

const COLUMNS: Column[] = [
  {
    id: "q",
    title: "Q",
    width: 34,
    render: (r) => {
      if (!r.queue) return null;
      const sch = r.queue.schedule?.enabled ? r.queue.schedule : null;
      const tip = `${r.queue.name}${sch ? `, starts at ${sch.start_time}` : ""}${r.queue.running ? ", running" : ""}`;
      return (
        <span className="qmark" title={tip} aria-label={tip}>
          <Icon name={sch ? "scheduler" : "queue"} size={13} />
          {r.qIndex}
        </span>
      );
    },
    sort: (r) => r.qIndex,
  },
  {
    id: "name",
    title: "File name",
    width: 300,
    render: (r) => (
      <div className="namecell">
        <Icon className="ico" name={fileIcon(r.j.category)} size={16} />
        <span title={F.fileName(r.j)}>{F.fileName(r.j)}</span>
      </div>
    ),
    sort: (r) => F.fileName(r.j).toLowerCase(),
  },
  { id: "size", title: "Size", width: 86, render: (r) => F.size(r.p?.total ?? r.j.total), sort: (r) => r.p?.total ?? r.j.total ?? -1 },
  {
    id: "status",
    title: "Status",
    width: 130,
    cls: "c-status",
    render: (r) => {
      const pct = F.isActive(r.j.state) ? F.percent(r.p?.downloaded ?? r.j.downloaded, r.p?.total ?? r.j.total) : null;
      return (
        <div className="statuscell" title={r.j.error?.message ?? undefined}>
          <span>{F.statusText(r.j, r.p)}</span>
          {pct != null && (
            <div className="minibar">
              <div style={{ width: `${pct}%` }} />
            </div>
          )}
        </div>
      );
    },
    sort: (r) => r.j.state,
  },
  { id: "left", title: "Time left", width: 92, render: (r) => (F.isActive(r.j.state) ? F.duration(r.p?.eta) : ""), sort: (r) => r.p?.eta ?? Infinity },
  { id: "rate", title: "Transfer rate", width: 100, render: (r) => (F.isActive(r.j.state) ? F.speed(r.p?.speed) : ""), sort: (r) => r.p?.speed ?? -1 },
  { id: "lasttry", title: "Last try", width: 140, render: (r) => F.date(r.j.last_try), sort: (r) => r.j.last_try ?? 0 },
  { id: "desc", title: "Description", width: 180, render: (r) => r.j.description ?? "", sort: (r) => (r.j.description ?? "").toLowerCase() },
  { id: "added", title: "Date added", width: 140, render: (r) => F.date(r.j.created_at), sort: (r) => r.j.created_at },
  { id: "saveto", title: "Save to", width: 200, render: (r) => r.j.dest_dir, sort: (r) => r.j.dest_dir.toLowerCase() },
  { id: "address", title: "Address", width: 240, render: (r) => r.j.url, sort: (r) => r.j.url },
  { id: "referer", title: "Web page", width: 200, render: (r) => r.j.referer ?? "", sort: (r) => r.j.referer ?? "" },
];
const DEFAULT_COLS = ["q", "name", "size", "status", "left", "rate", "lasttry", "desc"];

// ------------------------------------------------------------------ list filters
type Filter =
  | { kind: "all" }
  | { kind: "category"; name: string | null }
  | { kind: "unfinished" }
  | { kind: "finished" }
  | { kind: "queued" }
  | { kind: "queue"; id: string };

function filterJobs(jobs: Job[], f: Filter): Job[] {
  switch (f.kind) {
    case "all":
      return jobs;
    case "category":
      return jobs.filter((j) => (j.category ?? null) === f.name);
    case "unfinished":
      return jobs.filter((j) => j.state !== "COMPLETED");
    case "finished":
      return jobs.filter((j) => j.state === "COMPLETED");
    case "queued":
      return jobs.filter((j) => j.queue_id && j.state !== "COMPLETED" && j.state !== "CANCELLED");
    case "queue":
      return jobs.filter((j) => j.queue_id === f.id && j.state !== "COMPLETED" && j.state !== "CANCELLED");
  }
}
const sameFilter = (a: Filter, b: Filter) => JSON.stringify(a) === JSON.stringify(b);

// ------------------------------------------------------------------ main window
type FindField = "any" | "name" | "description" | "address" | "referer";
const FIND_FIELDS: [FindField, string, string][] = [
  ["any", "Everywhere", "File name, description or address"],
  ["name", "File name", "File name"],
  ["description", "Description", "Description"],
  ["address", "Address", "Download address"],
  ["referer", "Web page", "Page the link was on"],
];

export default function Main() {
  const live = useLive();
  useTheme(live?.settings.theme);
  if (!live) return null;
  return <MainView live={live} />;
}

function MainView({ live }: { live: Live }) {
  const { settings, queues } = live;
  const [filter, setFilter] = useState<Filter>(() => pref<Filter>("filter", { kind: "all" }));
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [cols, setCols] = useState<string[]>(() => pref("cols", DEFAULT_COLS));
  const [widths, setWidths] = useState<Record<string, number>>(() => pref("widths", {}));
  const [sort, setSort] = useState<{ col: string; asc: boolean } | null>(() => pref("sort", null));
  const [showSidebar, setShowSidebar] = useState(() => pref("sidebar", true));
  const [sidebarW, setSidebarW] = useState(() => pref("sidebarW", 210));
  const [smallToolbar, setSmallToolbar] = useState(() => pref("smallToolbar", false));
  const [tbItems, setTbItems] = useState<string[]>(() => pref("toolbar", DEFAULT_TOOLBAR));
  const [openCats, setOpenCats] = useState(() => pref("openCats", true));
  const [openQueues, setOpenQueues] = useState(() => pref("openQueues", true));
  const [ctx, setCtx] = useState<{ x: number; y: number; items: MenuEntry[] } | null>(null);
  const [elevated, setElevated] = useState(false);
  useEffect(() => {
    api.isElevated().then(setElevated).catch(() => {});
  }, []);
  const [find, setFind] = useState<{ open: boolean; text: string; msg: string; field: FindField; matchCase: boolean; whole: boolean }>({ open: false, text: "", msg: "", field: "any", matchCase: false, whole: false });
  const [modal, setModal] = useState<
    | null
    | { kind: "delete"; ids: string[] }
    | { kind: "columns" }
    | { kind: "deleteDone" }
    | { kind: "export"; format: "txt" | "ef2" }
    | { kind: "toolbar" }
    | { kind: "font" }
  >(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updating, setUpdating] = useState<{ done: number; total: number | null } | null>(null);
  const [updateErr, setUpdateErr] = useState("");
  const [dropShown, setDropShown] = useState(() => pref<boolean>("dropTarget", false));
  const toggleDrop = (on: boolean) => {
    setDropShown(on);
    savePref("dropTarget", on);
    if (on) api.openWindow("drop");
    else api.closeWindow("drop");
  };
  // Show the drop target again at start if it was open last time.
  useEffect(() => {
    if (pref<boolean>("dropTarget", false)) api.openWindow("drop");
  }, []);
  // Tip of the day when the window opens (not when Keepfetch starts hidden in the tray).
  const tipsShown = useRef(false);
  useEffect(() => {
    if (tipsShown.current || !live?.settings.tips_on_startup || document.visibilityState !== "visible") return;
    tipsShown.current = true;
    const t = setTimeout(() => api.openWindow("tips"), 1500);
    return () => clearTimeout(t);
  }, [live?.settings.tips_on_startup]);
  useEffect(() => {
    api.pendingUpdate().then(setUpdate);
    const un = onCore((e) => {
      if (e.type === "update_available") setUpdate(e.update);
      if (e.type === "update_progress") setUpdating({ done: e.done, total: e.total });
    });
    // First run: offer to connect the browser.
    api.apiStatus().then((s) => {
      let done = "";
      try {
        done = localStorage.getItem("sdm.browserSetupDone") ?? "";
      } catch {
        /* ignore */
      }
      if (!done && s.port && s.paired.length === 0) {
        try {
          localStorage.setItem("sdm.browserSetupDone", "shown");
        } catch {
          /* ignore */
        }
        api.openWindow("browser");
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  const installUpdate = async () => {
    setUpdateErr("");
    setUpdating({ done: 0, total: null });
    try {
      await api.installUpdate();
    } catch (e) {
      setUpdating(null);
      setUpdateErr(String(e));
    }
  };
  const checkUpdates = async () => {
    try {
      const u = await api.checkUpdate();
      if (u) setUpdate(u);
      else alert(`Keepfetch ${live.version} is the latest version.`);
    } catch (e) {
      alert(String(e));
    }
  };
  const findRef = useRef<HTMLInputElement>(null);

  useEffect(() => savePref("filter", filter), [filter]);
  useEffect(() => savePref("cols", cols), [cols]);
  useEffect(() => savePref("widths", widths), [widths]);
  useEffect(() => savePref("sort", sort), [sort]);
  useEffect(() => savePref("sidebar", showSidebar), [showSidebar]);
  useEffect(() => savePref("sidebarW", sidebarW), [sidebarW]);
  useEffect(() => savePref("smallToolbar", smallToolbar), [smallToolbar]);
  useEffect(() => savePref("toolbar", tbItems), [tbItems]);
  useEffect(() => savePref("openCats", openCats), [openCats]);
  useEffect(() => savePref("openQueues", openQueues), [openQueues]);

  const queueIndex = useMemo(() => new Map(queues.map((q, i) => [q.id, i + 1])), [queues]);
  const queueById = useMemo(() => new Map(queues.map((q) => [q.id, q])), [queues]);

  const rows: Row[] = useMemo(() => {
    const list = filterJobs(live.jobs, filter).map((j) => ({ j, p: live.progressMap.get(j.id), qIndex: j.queue_id ? queueIndex.get(j.queue_id) ?? 0 : 0, queue: j.queue_id ? queueById.get(j.queue_id) : undefined }));
    const col = sort && COLUMNS.find((c) => c.id === sort.col);
    if (col) {
      list.sort((a, b) => {
        const x = col.sort(a);
        const y = col.sort(b);
        const c = x < y ? -1 : x > y ? 1 : a.j.created_at - b.j.created_at;
        return sort!.asc ? c : -c;
      });
    } else {
      list.sort((a, b) => a.j.created_at - b.j.created_at || a.j.id.localeCompare(b.j.id));
    }
    return list;
  }, [live.jobs, live.progressMap, filter, sort, queueIndex, queueById]);

  // Drop selections that are no longer visible.
  useEffect(() => {
    const visible = new Set(rows.map((r) => r.j.id));
    if ([...sel].some((id) => !visible.has(id))) setSel(new Set([...sel].filter((id) => visible.has(id))));
  }, [rows, sel]);

  const selected = rows.filter((r) => sel.has(r.j.id)).map((r) => r.j);
  const selIds = selected.map((j) => j.id);
  const one = selected.length === 1 ? selected[0] : null;

  // ------------------------------------------------------------------ actions
  const act = {
    add: () => api.openWindow("add"),
    batch: () => api.openWindow("batch"),
    batchClipboard: async () => {
      const urls = await api.clipboardUrls();
      if (urls.length) api.openWindow("links", { source: "clipboard" });
      else alert("No web addresses were found in the clipboard.");
    },
    resume: () => api.resume(selected.filter(F.canResume).map((j) => j.id)),
    stop: () => api.pause(selected.filter(F.canStop).map((j) => j.id)),
    stopAll: () => api.pauseAll(),
    del: () => {
      if (!selIds.length) return;
      if (live.settings.confirm_delete) setModal({ kind: "delete", ids: selIds });
      else api.remove(selIds, false);
    },
    deleteDone: () => setModal({ kind: "deleteDone" }),
    options: (tab?: string) => api.openWindow("options", tab ? { tab } : {}),
    scheduler: (q?: string) => api.openWindow("scheduler", q ? { queue: q } : {}),
    open: (j: Job) => (j.state === "COMPLETED" ? api.openFile(j.id) : api.openWindow("progress", { id: j.id })),
    properties: (j: Job) => api.openWindow("properties", { id: j.id }),
    exportList: async (ids: string[] | null, format: "txt" | "ef2" = "txt") => {
      const n = await api.exportList(ids, format);
      if (n > 0) alert(`Exported ${n} addresses.`);
    },
    importList: async () => {
      const list = await api.importList().catch((e) => {
        alert(String(e));
        return [];
      });
      if (!list.length) return;
      const referers = Object.fromEntries(list.filter((e) => e.referer).map((e) => [e.url, e.referer]));
      api.openWindow("links", { source: "import", urls: JSON.stringify(list.map((e) => e.url)), referers: JSON.stringify(referers) });
    },
    toggleLimiter: async (on: boolean) => {
      if (on) await api.saveSettings({ ...settings, speed_limit: settings.speed_limit || settings.last_speed_limit || 500 * 1024 });
      else await api.saveSettings({ ...settings, speed_limit: 0, last_speed_limit: settings.speed_limit || settings.last_speed_limit });
    },
  };

  const doFind = useCallback(
    (text: string, from: string | null) => {
      if (!text) return;
      const norm = (v: string) => (find.matchCase ? v : v.toLowerCase());
      const t = norm(text);
      const fields = (j: Job) => {
        switch (find.field) {
          case "name": return [F.fileName(j)];
          case "description": return [j.description ?? ""];
          case "address": return [j.url];
          case "referer": return [j.referer ?? ""];
          default: return [F.fileName(j), j.description ?? "", j.url, j.referer ?? ""];
        }
      };
      const hit = (j: Job) => fields(j).some((f) => (find.whole ? norm(f) === t : norm(f).includes(t)));
      const start = from ? rows.findIndex((r) => r.j.id === from) + 1 : 0;
      for (let k = 0; k < rows.length; k++) {
        const r = rows[(start + k) % rows.length];
        if (hit(r.j)) {
          setSel(new Set([r.j.id]));
          setAnchor(r.j.id);
          setFind((f) => ({ ...f, msg: "" }));
          document.getElementById(`row-${r.j.id}`)?.scrollIntoView({ block: "nearest" });
          return;
        }
      }
      setFind((f) => ({ ...f, msg: "Not found in this list." }));
    },
    [rows, find.field, find.matchCase, find.whole],
  );

  // ------------------------------------------------------------------ keyboard
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement).tagName === "INPUT" || (e.target as HTMLElement).tagName === "TEXTAREA";
      if (e.ctrlKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        act.add();
      } else if (e.ctrlKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setFind((f) => ({ ...f, open: true }));
        setTimeout(() => findRef.current?.select(), 0);
      } else if (e.key === "F3") {
        e.preventDefault();
        doFind(find.text, anchor);
      } else if (typing) {
        return;
      } else if (e.ctrlKey && e.key.toLowerCase() === "a") {
        e.preventDefault();
        setSel(new Set(rows.map((r) => r.j.id)));
      } else if (e.key === "Delete") {
        act.del();
      } else if (e.key === "Enter" && one) {
        act.open(one);
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const i = rows.findIndex((r) => r.j.id === anchor);
        const n = Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
        const id = rows[n]?.j.id;
        if (id) {
          setSel(new Set([id]));
          setAnchor(id);
          document.getElementById(`row-${id}`)?.scrollIntoView({ block: "nearest" });
        }
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });

  // Text or links dropped on the window become new downloads.
  useEffect(() => {
    const over = (e: DragEvent) => e.preventDefault();
    const drop = async (e: DragEvent) => {
      e.preventDefault();
      const text = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
      const urls = await api.extractUrls(text);
      if (urls.length === 1) api.openWindow("info", { url: urls[0] });
      else if (urls.length > 1) api.openWindow("links", { source: "drop", urls: JSON.stringify(urls) });
    };
    window.addEventListener("dragover", over);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragover", over);
      window.removeEventListener("drop", drop);
    };
  }, []);

  // ------------------------------------------------------------------ menus
  const queueItems = (fn: (q: Queue) => void, onlyRunning?: boolean): MenuEntry[] =>
    queues.filter((q) => (onlyRunning == null ? true : onlyRunning ? q.running : !q.running)).map((q) => ({ label: q.name, onClick: () => fn(q) }));

  const rowMenu = (targets: Job[] = selected): MenuEntry[] => {
    const one = targets.length === 1 ? targets[0] : null;
    const selIds = targets.map((j) => j.id);
    const selected = targets;
    return [
    { label: "Open", disabled: !one || one.state !== "COMPLETED", onClick: () => one && api.openFile(one.id), hint: "Enter" },
    { label: "Open with…", disabled: !one || one.state !== "COMPLETED", onClick: () => one && api.openWith(one.id) },
    { label: "Open folder", disabled: !one, onClick: () => one && api.openFolder(one.id) },
    "sep",
    { label: "Resume download", disabled: !selected.some(F.canResume), onClick: act.resume },
    { label: "Stop download", disabled: !selected.some(F.canStop), onClick: act.stop },
    { label: "Show progress", disabled: !one || one.state === "COMPLETED", onClick: () => one && api.openWindow("progress", { id: one.id }) },
    { label: "Download again", disabled: !selected.length, onClick: () => api.redownload(selIds) },
    { label: "Refresh download address…", disabled: !one || one.state === "COMPLETED", onClick: () => one && api.openWindow("properties", { id: one.id, focus: "address" }) },
    "sep",
    {
      label: "Move to queue",
      disabled: !selected.length,
      // Finished files can only go into synchronization queues.
      submenu: [
        ...queues.map((q) => ({
          label: q.name,
          disabled: q.kind !== "synchronization" && selected.every((j) => j.state === "COMPLETED"),
          onClick: () => api.moveToQueue(selIds, q.id),
        })),
        "sep",
        { label: "Remove from queue", onClick: () => api.moveToQueue(selIds, null) },
      ],
    },
    { label: "Delete", hint: "Del", disabled: !selected.length, onClick: act.del },
    "sep",
    { label: "Export selected…", disabled: !selected.length, onClick: () => act.exportList(selIds, "ef2") },
    { label: "Properties", disabled: !one, onClick: () => one && act.properties(one) },
    ];
  };

  const limiterOn = settings.speed_limit > 0;
  const roots: MenuRoot[] = [
    {
      label: "Tasks",
      items: () => [
        { label: "Add new download…", hint: "Ctrl+N", onClick: act.add },
        { label: "Add batch download…", onClick: act.batch },
        { label: "Add batch download from clipboard", onClick: act.batchClipboard },
        { label: "Site grabber…", onClick: () => api.openWindow("grabber") },
        "sep",
        { label: "Show drop target", checked: dropShown, onClick: () => toggleDrop(!dropShown) },
        "sep",
        {
          label: "Export",
          submenu: [
            { label: "To an .ef2 export file…", onClick: () => setModal({ kind: "export", format: "ef2" }) },
            { label: "To text file…", onClick: () => setModal({ kind: "export", format: "txt" }) },
          ],
        },
        {
          label: "Import",
          submenu: [
            { label: "From an .ef2 export file…", onClick: act.importList },
            { label: "From text file…", onClick: act.importList },
          ],
        },
        "sep",
        { label: "Exit", onClick: () => api.exitApp() },
      ],
    },
    { label: "File", items: () => rowMenu() },
    {
      label: "Downloads",
      items: () => [
        { label: "Resume", disabled: !selected.some(F.canResume), onClick: act.resume },
        { label: "Stop", disabled: !selected.some(F.canStop), onClick: act.stop },
        { label: "Stop all", onClick: act.stopAll },
        { label: "Resume all", onClick: () => api.resumeAll() },
        "sep",
        { label: "Delete all completed…", onClick: act.deleteDone },
        "sep",
        { label: "Find…", hint: "Ctrl+F", onClick: () => setFind((f) => ({ ...f, open: true })) },
        { label: "Find next", hint: "F3", onClick: () => doFind(find.text, anchor) },
        "sep",
        { label: "Scheduler…", onClick: () => act.scheduler() },
        { label: "Start queue", submenu: queueItems((q) => api.startQueue(q.id), false) },
        { label: "Stop queue", submenu: queueItems((q) => api.stopQueue(q.id), true) },
        "sep",
        {
          label: "Speed limiter",
          submenu: [
            { label: "Turn on", checked: limiterOn, onClick: () => act.toggleLimiter(true) },
            { label: "Turn off", checked: !limiterOn, onClick: () => act.toggleLimiter(false) },
            "sep",
            { label: "Settings…", onClick: () => act.options("connection") },
          ],
        },
        "sep",
        { label: "Options…", onClick: () => act.options() },
      ],
    },
    {
      label: "View",
      items: () => [
        { label: "Hide categories", checked: !showSidebar, onClick: () => setShowSidebar(!showSidebar) },
        {
          label: "Arrange files",
          submenu: [
            { label: "Order added", checked: !sort, onClick: () => setSort(null) },
            ...COLUMNS.filter((c) => c.id !== "q").map((c) => ({ label: c.title, checked: sort?.col === c.id, onClick: () => setSort({ col: c.id, asc: sort?.col === c.id ? !sort.asc : true }) })),
          ],
        },
        {
          label: "Toolbar",
          submenu: [
            { label: "Customize…", onClick: () => setModal({ kind: "toolbar" }) },
            "sep",
            { label: "Large buttons", checked: !smallToolbar, onClick: () => setSmallToolbar(false) },
            { label: "Small buttons", checked: smallToolbar, onClick: () => setSmallToolbar(true) },
          ],
        },
        {
          label: "Notification area icon",
          submenu: [
            { label: "Show", checked: settings.tray_icon, onClick: () => api.saveSettings({ ...settings, tray_icon: true }) },
            { label: "Don’t show", checked: !settings.tray_icon, onClick: () => api.saveSettings({ ...settings, tray_icon: false }) },
          ],
        },
        { label: "Customize list columns…", onClick: () => setModal({ kind: "columns" }) },
        {
          label: "Theme",
          submenu: (["system", "light", "dark"] as const).map((t) => ({
            label: t === "system" ? "Same as Windows" : t === "light" ? "Light" : "Dark",
            checked: settings.theme === t,
            onClick: () => api.saveSettings({ ...settings, theme: t }),
          })),
        },
        {
          label: "Font",
          submenu: [
            { label: "Choose font…", onClick: () => setModal({ kind: "font" }) },
            { label: "Reset to the default font", disabled: !settings.ui_font && !settings.ui_font_size, onClick: () => api.saveSettings({ ...settings, ui_font: "", ui_font_size: 0 }) },
          ],
        },
        "sep",
        { label: "Language", submenu: [{ label: "English", checked: true, onClick: () => {} }] },
      ],
    },
    {
      label: "Help",
      items: () => [
        { label: "Help contents", onClick: () => api.openPath("https://github.com/theharshchaudhary/SwiftHarryDM/blob/main/docs/HELP.md") },
        { label: "Scheduler and queues", onClick: () => api.openPath("https://github.com/theharshchaudhary/SwiftHarryDM/blob/main/docs/HELP.md#scheduler-and-queues") },
        { label: "Keyboard shortcuts", onClick: () => alert("Ctrl+N  Add download\nCtrl+F  Find\nF3  Find next\nDel  Delete\nEnter  Open / show progress\nCtrl+A  Select all") },
        { label: "Tip of the day…", onClick: () => api.openWindow("tips") },
        "sep",
        { label: "Keepfetch home page", onClick: () => api.openPath("https://github.com/theharshchaudhary/SwiftHarryDM") },
        { label: "Report a problem", onClick: () => api.openPath("https://github.com/theharshchaudhary/SwiftHarryDM/issues") },
        { label: "Connect your browser…", onClick: () => api.openWindow("browser") },
        "sep",
        { label: "Check for updates…", onClick: checkUpdates },
        "sep",
        { label: "About Keepfetch", onClick: () => api.openWindow("about") },
      ],
    },
  ];

  // ------------------------------------------------------------------ row interaction
  const onRowDown = (e: React.MouseEvent, id: string) => {
    if (e.button === 2 && sel.has(id)) return;
    if (e.shiftKey && anchor) {
      const a = rows.findIndex((r) => r.j.id === anchor);
      const b = rows.findIndex((r) => r.j.id === id);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      const range = rows.slice(lo, hi + 1).map((r) => r.j.id);
      setSel(new Set(e.ctrlKey ? [...sel, ...range] : range));
      return;
    }
    if (e.ctrlKey) {
      const s = new Set(sel);
      s.has(id) ? s.delete(id) : s.add(id);
      setSel(s);
    } else setSel(new Set([id]));
    setAnchor(id);
  };

  const startResize = (e: React.MouseEvent, colId: string, base: number) => {
    e.preventDefault();
    e.stopPropagation();
    const x0 = e.clientX;
    const move = (m: MouseEvent) => setWidths((w) => ({ ...w, [colId]: Math.max(30, base + m.clientX - x0) }));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const startSidebarResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const x0 = e.clientX;
    const base = sidebarW;
    const move = (m: MouseEvent) => setSidebarW(Math.max(140, Math.min(420, base + m.clientX - x0)));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const visibleCols = cols.map((id) => COLUMNS.find((c) => c.id === id)).filter(Boolean) as Column[];

  // ------------------------------------------------------------------ sidebar
  const counts = useMemo(() => {
    const c = { all: live.jobs.length, unfinished: 0, finished: 0, cat: new Map<string | null, number>(), queue: new Map<string, number>() };
    for (const j of live.jobs) {
      if (j.state === "COMPLETED") c.finished++;
      else c.unfinished++;
      c.cat.set(j.category ?? null, (c.cat.get(j.category ?? null) ?? 0) + 1);
      if (j.queue_id && j.state !== "COMPLETED" && j.state !== "CANCELLED") c.queue.set(j.queue_id, (c.queue.get(j.queue_id) ?? 0) + 1);
    }
    return c;
  }, [live.jobs]);

  const TreeItem = ({ f, icon, label, count, child, twist, dot, menu }: { f: Filter; icon: string; label: string; count?: number; child?: boolean; twist?: [boolean, () => void]; dot?: boolean; menu?: MenuEntry[] }) => (
    <div
      className={`tree-item${child ? " child" : ""}${sameFilter(filter, f) ? " sel" : ""}`}
      onMouseDown={() => setFilter(f)}
      onContextMenu={(e) => {
        e.preventDefault();
        if (menu) setCtx({ x: e.clientX, y: e.clientY, items: menu });
      }}
      onDragOver={(e) => f.kind === "queue" && e.preventDefault()}
      onDrop={(e) => {
        if (f.kind !== "queue") return;
        e.preventDefault();
        e.stopPropagation();
        const ids = JSON.parse(e.dataTransfer.getData("application/x-sdm-ids") || "[]") as string[];
        if (ids.length) api.moveToQueue(ids, f.id);
      }}
    >
      {twist ? (
        <span className={`twist${twist[0] ? " open" : ""}`} onMouseDown={(e) => { e.stopPropagation(); twist[1](); }}>
          <Icon name="chevron" size={12} />
        </span>
      ) : (
        !child && <span className="twist" />
      )}
      <Icon className="ico" name={icon} size={17} />
      <span className="ellipsis">{label}</span>
      {dot && <span className="running-dot" title="Running" />}
      {count != null && count > 0 && <span className="count">{count}</span>}
    </div>
  );

  const totalSpeed = [...live.progressMap.values()].reduce((s, p) => s + (p.speed ?? 0), 0);
  const active = live.jobs.filter((j) => F.isActive(j.state)).length;

  return (
    <div className="app" onContextMenu={(e) => e.preventDefault()}>
      <MenuBar roots={roots} />
      <div className={`toolbar${smallToolbar ? " small" : ""}`}>
        {tbItems.map((id, i) => {
          if (id === "|") return <div key={i} className="divider" />;
          switch (id) {
            case "add":
              return <TB key={id} icon="add" label="Add URL" title="Add a new download (Ctrl+N)" onClick={act.add} />;
            case "resume":
              return <TB key={id} icon="resume" label="Resume" cls="green" disabled={!selected.some(F.canResume)} onClick={act.resume} title="Start or resume the selected downloads" />;
            case "stop":
              return <TB key={id} icon="stop" label="Stop" cls="amber" disabled={!selected.some(F.canStop)} onClick={act.stop} title="Stop the selected downloads" />;
            case "stopAll":
              return <TB key={id} icon="stopAll" label="Stop all" cls="amber" disabled={active === 0 && !live.jobs.some((j) => j.state === "QUEUED")} onClick={act.stopAll} title="Stop every download" />;
            case "delete":
              return <TB key={id} icon="delete" label="Delete" cls="red" disabled={!selected.length} onClick={act.del} title="Remove the selected downloads from the list" />;
            case "deleteDone":
              return <TB key={id} icon="deleteDone" label="Clear done" cls="red" disabled={!counts.finished} onClick={act.deleteDone} title="Remove all completed downloads from the list" />;
            case "options":
              return <TB key={id} icon="options" label="Options" onClick={() => act.options()} title="Browser integration, file types, connection, proxy, logins and more" />;
            case "scheduler":
              return <TB key={id} icon="scheduler" label="Scheduler" onClick={() => act.scheduler()} title="Queues and their schedules" />;
            case "startQueue":
              return (
                <TB
                  key={id}
                  icon="startQueue"
                  label="Start queue"
                  cls="green"
                  drop
                  onClick={(e) => (queues.length === 1 ? api.startQueue(queues[0].id) : setCtx({ x: e.clientX, y: e.clientY, items: queueItems((q) => api.startQueue(q.id), false) }))}
                  title="Start the main queue, or choose a queue"
                />
              );
            case "stopQueue":
              return (
                <TB
                  key={id}
                  icon="stopQueue"
                  label="Stop queue"
                  cls="amber"
                  drop
                  disabled={!queues.some((q) => q.running)}
                  onClick={(e) => setCtx({ x: e.clientX, y: e.clientY, items: queueItems((q) => api.stopQueue(q.id), true) })}
                  title="Stop a running queue"
                />
              );
            case "batch":
              return <TB key={id} icon="batch" label="Batch" onClick={act.batch} title="Add a numbered series of files" />;
            case "find":
              return <TB key={id} icon="search" label="Find" onClick={() => setFind((f) => ({ ...f, open: true }))} title="Find a download (Ctrl+F)" />;
            default:
              return null;
          }
        })}
        <div className="spacer" />
      </div>

      {elevated && (
        <div className="topbanner warn" role="status">
          <Icon name="warning" size={16} />
          <span>Keepfetch is running as administrator, so Windows blocks dragging links and files into it from other programs. Start it normally to drag and drop.</span>
          <span style={{ flex: 1 }} />
          <button className="btn small" title="Hide" onClick={() => setElevated(false)}><Icon name="close" size={12} /></button>
        </div>
      )}
      {(update || updateErr) && (
        <div className="topbanner">
          <Icon name="all" size={16} />
          {updating ? (
            <span>
              Updating to {update?.version}… {updating.total ? `${Math.round((updating.done / updating.total) * 100)}%` : ""} Keepfetch restarts by itself and your downloads continue.
            </span>
          ) : (
            <>
              <span>{updateErr ? <span className="err">{updateErr}</span> : <>Keepfetch {update?.version} is available.</>}</span>
              <button className="btn small primary" onClick={installUpdate}>Update now</button>
              {update?.notes && <span className="muted ellipsis" style={{ maxWidth: 420 }} title={update.notes}>{update.notes.split("\n")[0]}</span>}
              <span style={{ flex: 1 }} />
              <button className="btn small" title="Later" onClick={() => { setUpdate(null); setUpdateErr(""); }}><Icon name="close" size={12} /></button>
            </>
          )}
        </div>
      )}
      <div className={`body${showSidebar ? "" : " nosidebar"}`} style={{ ["--sidebar-w" as string]: `${sidebarW}px` }}>
        <div className="sidebar">
          {showSidebar && (
            <>
              <div className="title">Downloads</div>
              <TreeItem f={{ kind: "all" }} icon="all" label="All downloads" count={counts.all} twist={[openCats, () => setOpenCats(!openCats)]} />
              {openCats && (
                <>
                  {settings.categories.map((c) => (
                    <TreeItem key={c.name} child f={{ kind: "category", name: c.name }} icon={categoryIcon(c.name)} label={c.name} count={counts.cat.get(c.name)} />
                  ))}
                  <TreeItem child f={{ kind: "category", name: null }} icon="folder" label="General" count={counts.cat.get(null)} />
                </>
              )}
              <TreeItem f={{ kind: "unfinished" }} icon="unfinished" label="Unfinished" count={counts.unfinished} />
              <TreeItem f={{ kind: "finished" }} icon="finished" label="Finished" count={counts.finished} />
              <div className="title" style={{ marginTop: 8 }}>Queues</div>
              <TreeItem f={{ kind: "queued" }} icon="queue" label="All queues" twist={[openQueues, () => setOpenQueues(!openQueues)]} menu={[{ label: "New queue…", onClick: () => act.scheduler("new") }]} />
              {openQueues &&
                queues.map((q) => (
                  <TreeItem
                    key={q.id}
                    child
                    f={{ kind: "queue", id: q.id }}
                    icon={q.kind === "synchronization" ? "sync" : "queue"}
                    label={q.name}
                    dot={q.running}
                    count={counts.queue.get(q.id)}
                    menu={[
                      { label: "Start", disabled: q.running, onClick: () => api.startQueue(q.id) },
                      { label: "Stop", disabled: !q.running, onClick: () => api.stopQueue(q.id) },
                      "sep",
                      { label: "Edit in scheduler…", onClick: () => act.scheduler(q.id) },
                    ]}
                  />
                ))}
            </>
          )}
        </div>
        <div className="splitter" onMouseDown={startSidebarResize} />
        <div
          className="list"
          ref={listRef}
          tabIndex={0}
          onMouseDown={(e) => {
            if (e.target === listRef.current) setSel(new Set());
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            setCtx({ x: e.clientX, y: e.clientY, items: rowMenu() });
          }}
        >
          <table>
            <colgroup>
              {visibleCols.map((c) => (
                <col key={c.id} style={{ width: widths[c.id] ?? c.width }} />
              ))}
            </colgroup>
            <thead>
              <tr>
                {visibleCols.map((c) => (
                  <th
                    key={c.id}
                    style={{ position: "sticky" }}
                    onClick={() => setSort(sort?.col === c.id ? (sort.asc ? { col: c.id, asc: false } : null) : { col: c.id, asc: true })}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      setCtx({
                        x: e.clientX,
                        y: e.clientY,
                        items: [
                          ...COLUMNS.filter((x) => x.id !== "name").map((x) => ({
                            label: x.title,
                            checked: cols.includes(x.id),
                            onClick: () => setCols(cols.includes(x.id) ? cols.filter((i) => i !== x.id) : [...cols, x.id]),
                          })),
                          "sep",
                          { label: "Columns…", onClick: () => setModal({ kind: "columns" }) },
                        ],
                      });
                    }}
                  >
                    {c.title}
                    {sort?.col === c.id && <span className="sort">{sort.asc ? "▲" : "▼"}</span>}
                    <span className="resize" onClick={(e) => e.stopPropagation()} onMouseDown={(e) => startResize(e, c.id, widths[c.id] ?? c.width)} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr
                  key={r.j.id}
                  id={`row-${r.j.id}`}
                  className={`state-${r.j.state}${sel.has(r.j.id) ? " sel" : ""}`}
                  onMouseDown={(e) => onRowDown(e, r.j.id)}
                  onDoubleClick={() => act.open(r.j)}
                  draggable
                  onDragStart={(e) => {
                    const ids = sel.has(r.j.id) ? selIds : [r.j.id];
                    e.dataTransfer.setData("application/x-sdm-ids", JSON.stringify(ids));
                    e.dataTransfer.setData("text/plain", ids.map((id) => live.jobMap.get(id)?.url).join("\n"));
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    const targets = sel.has(r.j.id) ? selected : [r.j];
                    if (!sel.has(r.j.id)) {
                      setSel(new Set([r.j.id]));
                      setAnchor(r.j.id);
                    }
                    setCtx({ x: e.clientX, y: e.clientY, items: rowMenu(targets) });
                  }}
                >
                  {visibleCols.map((c) => (
                    <td key={c.id} className={c.cls}>
                      {c.render(r)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length === 0 && (
            <div className="empty">
              <p>Nothing here yet.</p>
              <p>
                Click <b>Add URL</b> (Ctrl+N) or drop a link on this window. Downloads you start in your browser land here too.
              </p>
            </div>
          )}
        </div>
      </div>

      {find.open ? (
        <div className="findbar">
          <Icon name="search" size={16} />
          <input
            ref={findRef}
            type="text"
            autoFocus
            aria-label="Find"
            placeholder={FIND_FIELDS.find(([k]) => k === find.field)![2]}
            value={find.text}
            onChange={(e) => setFind({ ...find, text: e.target.value, msg: "" })}
            onKeyDown={(e) => {
              if (e.key === "Enter") doFind(find.text, anchor);
              if (e.key === "Escape") setFind({ ...find, open: false });
            }}
          />
          <select aria-label="Look in" value={find.field} onChange={(e) => setFind({ ...find, field: e.target.value as FindField, msg: "" })}>
            {FIND_FIELDS.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
          </select>
          <Check checked={find.matchCase} onChange={(v) => setFind({ ...find, matchCase: v, msg: "" })}>Match case</Check>
          <Check checked={find.whole} onChange={(v) => setFind({ ...find, whole: v, msg: "" })}>Whole text only</Check>
          <button className="btn small" onClick={() => doFind(find.text, anchor)}>
            Find next
          </button>
          <span className="msg">{find.msg}</span>
          <span style={{ flex: 1 }} />
          <button className="btn small" title="Close" onClick={() => setFind({ ...find, open: false })}>
            <Icon name="close" size={14} />
          </button>
        </div>
      ) : (
        <div />
      )}

      <div className="statusbar">
        <span>{live.jobs.length} downloads</span>
        <span>{active} active</span>
        <span>{totalSpeed > 0 ? `Total ${F.speed(totalSpeed)}` : ""}</span>
        <span className="grow" />
        {live.quota.limit > 0 && (
          <span className={live.quota.blocked ? "warn" : ""} title={`Quota: ${settings.quota_mb} MB every ${settings.quota_hours} h`}>
            Quota {F.size(live.quota.used)} / {F.size(live.quota.limit)}
            {live.quota.blocked ? " (reached, downloads stopped)" : ""}
          </span>
        )}
        <span className={limiterOn ? "warn" : ""} style={{ cursor: "pointer" }} onClick={() => act.toggleLimiter(!limiterOn)} title="Click to turn the speed limiter on or off">
          <Icon name="speed" size={13} /> {limiterOn ? `Limited to ${F.speed(settings.speed_limit)}` : "No speed limit"}
        </span>
      </div>

      {ctx && <ContextMenu x={ctx.x} y={ctx.y} items={ctx.items} onClose={() => setCtx(null)} />}
      {modal?.kind === "delete" && <DeleteModal live={live} ids={modal.ids} onClose={() => setModal(null)} />}
      {modal?.kind === "deleteDone" && <DeleteDoneModal live={live} onClose={() => setModal(null)} />}
      {modal?.kind === "toolbar" && <ToolbarModal items={tbItems} onSave={(v) => { setTbItems(v); setModal(null); }} onClose={() => setModal(null)} />}
      {modal?.kind === "font" && <FontModal live={live} onClose={() => setModal(null)} />}
      {modal?.kind === "export" && (
        <ExportModal
          format={modal.format}
          live={live}
          selected={selIds}
          onExport={(ids) => {
            setModal(null);
            act.exportList(ids, modal.format);
          }}
          onClose={() => setModal(null)}
        />
      )}
      {modal?.kind === "columns" && <ColumnsModal cols={cols} setCols={setCols} onReset={() => { setCols(DEFAULT_COLS); setWidths({}); }} onClose={() => setModal(null)} />}
    </div>
  );
}

function TB({ icon, label, onClick, disabled, title, cls, drop }: { icon: string; label: string; onClick: (e: React.MouseEvent) => void; disabled?: boolean; title?: string; cls?: string; drop?: boolean }) {
  return (
    <button className={`tb ${cls ?? ""}`} disabled={disabled} onClick={onClick} title={title}>
      <Icon className="ico" name={icon} size={24} />
      <span className="lbl">{label}</span>
      {drop && <span className="drop">▼</span>}
    </button>
  );
}

// ------------------------------------------------------------------ modals inside the main window
/** Toolbar buttons, in order; "|" is a separator. */
const TOOLBAR_BUTTONS: [string, string][] = [
  ["add", "Add URL"],
  ["resume", "Resume"],
  ["stop", "Stop"],
  ["stopAll", "Stop all"],
  ["delete", "Delete"],
  ["deleteDone", "Clear done"],
  ["options", "Options"],
  ["scheduler", "Scheduler"],
  ["startQueue", "Start queue"],
  ["stopQueue", "Stop queue"],
  ["batch", "Batch"],
  ["find", "Find"],
];
const DEFAULT_TOOLBAR = ["add", "resume", "stop", "stopAll", "|", "delete", "deleteDone", "|", "options", "scheduler", "startQueue", "stopQueue", "|", "batch"];
const tbLabel = (id: string) => (id === "|" ? "— Separator —" : TOOLBAR_BUTTONS.find(([k]) => k === id)?.[1] ?? id);

/** "Customize toolbar": which buttons, in what order. */
function ToolbarModal({ items, onSave, onClose }: { items: string[]; onSave: (v: string[]) => void; onClose: () => void }) {
  const [list, setList] = useState(items);
  const [cur, setCur] = useState<number | null>(null);
  const [avail, setAvail] = useState<string | null>(null);
  const available = [...TOOLBAR_BUTTONS.map(([k]) => k).filter((k) => !list.includes(k)), "|"];
  const move = (d: number) => {
    if (cur == null || cur + d < 0 || cur + d >= list.length) return;
    const l = [...list];
    [l[cur], l[cur + d]] = [l[cur + d], l[cur]];
    setList(l);
    setCur(cur + d);
  };
  const box = { height: 220, overflow: "auto", border: "1px solid var(--border)", borderRadius: 4, flex: 1 } as const;
  const row = (on: boolean) => ({ padding: "3px 8px", cursor: "default", background: on ? "var(--sel)" : undefined, color: on ? "var(--sel-text)" : undefined });
  return (
    <Modal
      title="Customize toolbar"
      onClose={onClose}
      foot={
        <>
          <button className="btn" onClick={() => setList(DEFAULT_TOOLBAR)}>Reset</button>
          <span style={{ flex: 1 }} />
          <button className="btn primary" onClick={() => onSave(list)}>OK</button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <div className="row" style={{ alignItems: "stretch", gap: 8 }}>
        <div style={box}>
          <div className="hint" style={{ padding: "4px 8px" }}>Available</div>
          {available.map((k) => <div key={k} style={row(avail === k)} onClick={() => setAvail(k)}>{tbLabel(k)}</div>)}
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 6, justifyContent: "center" }}>
          <button className="btn small" disabled={!avail} onClick={() => { if (avail) { const l = [...list]; l.splice(cur == null ? l.length : cur + 1, 0, avail); setList(l); setAvail(null); } }}>Add →</button>
          <button className="btn small" disabled={cur == null} onClick={() => { if (cur != null) { setList(list.filter((_, i) => i !== cur)); setCur(null); } }}>← Remove</button>
          <button className="btn small" disabled={cur == null} onClick={() => move(-1)}>Move up</button>
          <button className="btn small" disabled={cur == null} onClick={() => move(1)}>Move down</button>
        </div>
        <div style={box}>
          <div className="hint" style={{ padding: "4px 8px" }}>On the toolbar</div>
          {list.map((k, i) => <div key={i} style={row(cur === i)} onClick={() => setCur(i)}>{tbLabel(k)}</div>)}
        </div>
      </div>
    </Modal>
  );
}

const FONTS = ["Segoe UI Variable Text", "Segoe UI", "Arial", "Calibri", "Tahoma", "Verdana", "Microsoft Sans Serif", "Consolas"];

/** View > Font: the font and size of the windows. */
function FontModal({ live, onClose }: { live: Live; onClose: () => void }) {
  const s = live.settings;
  const [font, setFont] = useState(s.ui_font || "Segoe UI Variable Text");
  const [size, setSize] = useState(s.ui_font_size || 13);
  return (
    <Modal
      title="Font"
      onClose={onClose}
      foot={
        <>
          <button className="btn primary" onClick={() => { api.saveSettings({ ...s, ui_font: font === "Segoe UI Variable Text" ? "" : font, ui_font_size: size === 13 ? 0 : size }); onClose(); }}>OK</button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <div className="grid2">
        <span>Font</span>
        <select value={font} onChange={(e) => setFont(e.target.value)}>
          {FONTS.map((f) => <option key={f} value={f}>{f}</option>)}
        </select>
        <span>Size</span>
        <select value={size} onChange={(e) => setSize(Number(e.target.value))}>
          {[11, 12, 13, 14, 15, 16, 18].map((n) => <option key={n} value={n}>{n}</option>)}
        </select>
      </div>
      <div style={{ fontFamily: `"${font}"`, fontSize: size, border: "1px solid var(--border)", borderRadius: 4, padding: 8 }}>ubuntu-24.04-desktop-amd64.iso · 5.82 GB · 3.2 MB/s</div>
    </Modal>
  );
}

/** "Export download list": the download queue, the selected files, or all files. */
function ExportModal({ format, live, selected, onExport, onClose }: { format: "txt" | "ef2"; live: Live; selected: string[]; onExport: (ids: string[] | null) => void; onClose: () => void }) {
  const [what, setWhat] = useState<"queue" | "selected" | "all">(selected.length ? "selected" : "all");
  const queueIds = live.jobs.filter((j) => j.queue_id === "main").map((j) => j.id);
  return (
    <Modal
      title={format === "ef2" ? "Export to an .ef2 export file" : "Export to a text file"}
      onClose={onClose}
      foot={
        <>
          <button className="btn primary" autoFocus onClick={() => onExport(what === "all" ? null : what === "queue" ? queueIds : selected)}>OK</button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <label className="row"><input type="radio" checked={what === "queue"} disabled={!queueIds.length} onChange={() => setWhat("queue")} /> The download queue ({queueIds.length})</label>
      <label className="row"><input type="radio" checked={what === "selected"} disabled={!selected.length} onChange={() => setWhat("selected")} /> The selected files ({selected.length})</label>
      <label className="row"><input type="radio" checked={what === "all"} onChange={() => setWhat("all")} /> All files ({live.jobs.length})</label>
      {format === "ef2" && <div className="hint">Download managers that read .ef2 files can import it; it keeps each download’s web page.</div>}
    </Modal>
  );
}

function DeleteModal({ live, ids, onClose }: { live: Live; ids: string[]; onClose: () => void }) {
  const [files, setFiles] = useState(false);
  const [quiet, setQuiet] = useState(false);
  const jobs = ids.map((id) => live.jobMap.get(id)).filter(Boolean) as Job[];
  const unfinished = jobs.filter((j) => j.state !== "COMPLETED").length;
  return (
    <Modal
      title="Delete downloads"
      onClose={onClose}
      foot={
        <>
          <button
            className="btn primary"
            autoFocus
            onClick={() => {
              api.remove(ids, files);
              if (quiet) api.saveSettings({ ...live.settings, confirm_delete: false });
              onClose();
            }}
          >
            Delete
          </button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <div>
        Remove {jobs.length === 1 ? <b>{F.fileName(jobs[0])}</b> : `${jobs.length} downloads`} from the list?
      </div>
      {unfinished > 0 && <div className="hint">{unfinished === jobs.length ? "Partly downloaded data will be deleted." : `${unfinished} unfinished: their partial data will be deleted.`}</div>}
      {jobs.some((j) => j.state === "COMPLETED") && (
        <Check checked={files} onChange={setFiles}>
          Also delete the downloaded files from disk
        </Check>
      )}
      <Check checked={quiet} onChange={setQuiet}>
        Don’t ask again (Options › Downloads)
      </Check>
    </Modal>
  );
}

function DeleteDoneModal({ live, onClose }: { live: Live; onClose: () => void }) {
  const done = live.jobs.filter((j) => j.state === "COMPLETED");
  return (
    <Modal
      title="Clear completed downloads"
      onClose={onClose}
      foot={
        <>
          <button className="btn primary" autoFocus disabled={!done.length} onClick={() => { api.remove(done.map((j) => j.id), false); onClose(); }}>
            Clear
          </button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <div>Remove {done.length} completed downloads from the list? The files stay on disk.</div>
    </Modal>
  );
}

function ColumnsModal({ cols, setCols, onReset, onClose }: { cols: string[]; setCols: (c: string[]) => void; onReset: () => void; onClose: () => void }) {
  const [order, setOrder] = useState<string[]>(() => [...cols, ...COLUMNS.map((c) => c.id).filter((id) => !cols.includes(id))]);
  const [shown, setShown] = useState(new Set(cols));
  const [cur, setCur] = useState<string | null>(null);
  const move = (d: number) => {
    if (!cur) return;
    const i = order.indexOf(cur);
    const j = i + d;
    if (j < 0 || j >= order.length) return;
    const o = [...order];
    [o[i], o[j]] = [o[j], o[i]];
    setOrder(o);
  };
  return (
    <Modal
      title="Columns"
      onClose={onClose}
      foot={
        <>
          <span style={{ marginRight: "auto" }}>
            <button className="btn" onClick={() => { onReset(); onClose(); }}>Reset</button>
          </span>
          <button className="btn primary" onClick={() => { setCols(order.filter((id) => shown.has(id))); onClose(); }}>OK</button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <div className="hint">Tick the columns to show; select one and move it to change the order.</div>
      <div className="row" style={{ alignItems: "stretch" }}>
        <div className="tbl grow" style={{ height: 260 }}>
          <table>
            <tbody>
              {order.map((id) => {
                const c = COLUMNS.find((x) => x.id === id)!;
                return (
                  <tr key={id} className={cur === id ? "sel" : ""} onMouseDown={() => setCur(id)}>
                    <td>
                      <label>
                        <input
                          type="checkbox"
                          checked={shown.has(id)}
                          disabled={id === "name"}
                          onChange={(e) => {
                            const s = new Set(shown);
                            e.target.checked ? s.add(id) : s.delete(id);
                            setShown(s);
                          }}
                        />
                        {c.title === "Q" ? "Q (queue)" : c.title}
                      </label>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <button className="btn small" disabled={!cur} onClick={() => move(-1)}>Move up</button>
          <button className="btn small" disabled={!cur} onClick={() => move(1)}>Move down</button>
        </div>
      </div>
    </Modal>
  );
}
