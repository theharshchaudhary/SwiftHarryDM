import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onCore, type Job, type Progress, type Queue, type UpdateInfo } from "../api";
import * as F from "../format";
import { Icon, categoryIcon, fileIcon } from "../icons";
import { ContextMenu, MenuBar, type MenuEntry, type MenuRoot } from "../components/Menu";
import { Check } from "../components/Dialog";
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
type Row = { j: Job; p?: Progress; qIndex: number };
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
    render: (r) => (r.qIndex > 0 ? <span className="qmark" title="Queue">{r.qIndex}</span> : null),
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
  const [openCats, setOpenCats] = useState(() => pref("openCats", true));
  const [openQueues, setOpenQueues] = useState(() => pref("openQueues", true));
  const [ctx, setCtx] = useState<{ x: number; y: number; items: MenuEntry[] } | null>(null);
  const [find, setFind] = useState<{ open: boolean; text: string; msg: string }>({ open: false, text: "", msg: "" });
  const [modal, setModal] = useState<null | { kind: "delete"; ids: string[] } | { kind: "columns" } | { kind: "deleteDone" }>(null);
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
      else alert(`SwiftHarryDM ${live.version} is the latest version.`);
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
  useEffect(() => savePref("openCats", openCats), [openCats]);
  useEffect(() => savePref("openQueues", openQueues), [openQueues]);

  const queueIndex = useMemo(() => new Map(queues.map((q, i) => [q.id, i + 1])), [queues]);

  const rows: Row[] = useMemo(() => {
    const list = filterJobs(live.jobs, filter).map((j) => ({ j, p: live.progressMap.get(j.id), qIndex: j.queue_id ? queueIndex.get(j.queue_id) ?? 0 : 0 }));
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
  }, [live.jobs, live.progressMap, filter, sort, queueIndex]);

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
    del: () => selIds.length && setModal({ kind: "delete", ids: selIds }),
    deleteDone: () => setModal({ kind: "deleteDone" }),
    options: (tab?: string) => api.openWindow("options", tab ? { tab } : {}),
    scheduler: (q?: string) => api.openWindow("scheduler", q ? { queue: q } : {}),
    open: (j: Job) => (j.state === "COMPLETED" ? api.openFile(j.id) : api.openWindow("progress", { id: j.id })),
    properties: (j: Job) => api.openWindow("properties", { id: j.id }),
    exportList: async (ids: string[] | null) => {
      const n = await api.exportList(ids);
      if (n > 0) alert(`Exported ${n} addresses.`);
    },
    importList: async () => {
      const urls = await api.importList();
      if (urls.length) api.openWindow("links", { source: "import", urls: JSON.stringify(urls) });
    },
    toggleLimiter: async (on: boolean) => {
      const last = pref<number>("lastLimit", 500 * 1024);
      if (on) await api.saveSettings({ ...settings, speed_limit: settings.speed_limit || last });
      else {
        if (settings.speed_limit) savePref("lastLimit", settings.speed_limit);
        await api.saveSettings({ ...settings, speed_limit: 0 });
      }
    },
  };

  const doFind = useCallback(
    (text: string, from: string | null) => {
      if (!text) return;
      const t = text.toLowerCase();
      const hit = (j: Job) => [F.fileName(j), j.description ?? "", j.url, j.referer ?? ""].some((f) => f.toLowerCase().includes(t));
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
    [rows],
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
      submenu: [...queueItems((q) => api.moveToQueue(selIds, q.id)), "sep", { label: "Remove from queue", onClick: () => api.moveToQueue(selIds, null) }],
    },
    { label: "Delete", hint: "Del", disabled: !selected.length, onClick: act.del },
    "sep",
    { label: "Export selected…", disabled: !selected.length, onClick: () => act.exportList(selIds) },
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
        "sep",
        { label: "Show drop target", checked: dropShown, onClick: () => toggleDrop(!dropShown) },
        "sep",
        {
          label: "Export",
          submenu: [
            { label: "All downloads to text file…", onClick: () => act.exportList(null) },
            { label: "Selected downloads to text file…", disabled: !selected.length, onClick: () => act.exportList(selIds) },
          ],
        },
        { label: "Import from text file…", onClick: act.importList },
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
        { label: "Show categories", checked: showSidebar, onClick: () => setShowSidebar(!showSidebar) },
        {
          label: "Sort by",
          submenu: [
            { label: "Order added", checked: !sort, onClick: () => setSort(null) },
            ...COLUMNS.filter((c) => c.id !== "q").map((c) => ({ label: c.title, checked: sort?.col === c.id, onClick: () => setSort({ col: c.id, asc: sort?.col === c.id ? !sort.asc : true }) })),
          ],
        },
        { label: "Columns…", onClick: () => setModal({ kind: "columns" }) },
        {
          label: "Toolbar",
          submenu: [
            { label: "Large buttons", checked: !smallToolbar, onClick: () => setSmallToolbar(false) },
            { label: "Small buttons", checked: smallToolbar, onClick: () => setSmallToolbar(true) },
          ],
        },
        {
          label: "Theme",
          submenu: (["system", "light", "dark"] as const).map((t) => ({
            label: t === "system" ? "Same as Windows" : t === "light" ? "Light" : "Dark",
            checked: settings.theme === t,
            onClick: () => api.saveSettings({ ...settings, theme: t }),
          })),
        },
      ],
    },
    {
      label: "Help",
      items: () => [
        { label: "Keyboard shortcuts", onClick: () => alert("Ctrl+N  Add download\nCtrl+F  Find\nF3  Find next\nDel  Delete\nEnter  Open / show progress\nCtrl+A  Select all") },
        { label: "Connect your browser…", onClick: () => api.openWindow("browser") },
        { label: "Check for updates…", onClick: checkUpdates },
        "sep",
        { label: "About SwiftHarryDM", onClick: () => api.openWindow("about") },
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
        <TB icon="add" label="Add URL" title="Add a new download (Ctrl+N)" onClick={act.add} />
        <TB icon="resume" label="Resume" cls="green" disabled={!selected.some(F.canResume)} onClick={act.resume} title="Start or resume the selected downloads" />
        <TB icon="stop" label="Stop" cls="amber" disabled={!selected.some(F.canStop)} onClick={act.stop} title="Stop the selected downloads" />
        <TB icon="stopAll" label="Stop all" cls="amber" disabled={active === 0 && !live.jobs.some((j) => j.state === "QUEUED")} onClick={act.stopAll} title="Stop every download" />
        <div className="divider" />
        <TB icon="delete" label="Delete" cls="red" disabled={!selected.length} onClick={act.del} title="Remove the selected downloads from the list" />
        <TB icon="deleteDone" label="Clear done" cls="red" disabled={!counts.finished} onClick={act.deleteDone} title="Remove all completed downloads from the list" />
        <div className="divider" />
        <TB icon="options" label="Options" onClick={() => act.options()} title="Browser integration, file types, connection, proxy, logins and more" />
        <TB icon="scheduler" label="Scheduler" onClick={() => act.scheduler()} title="Queues and their schedules" />
        <TB
          icon="startQueue"
          label="Start queue"
          cls="green"
          drop
          onClick={(e) => (queues.length === 1 ? api.startQueue(queues[0].id) : setCtx({ x: e.clientX, y: e.clientY, items: queueItems((q) => api.startQueue(q.id), false) }))}
          title="Start the main queue, or choose a queue"
        />
        <TB
          icon="stopQueue"
          label="Stop queue"
          cls="amber"
          drop
          disabled={!queues.some((q) => q.running)}
          onClick={(e) => setCtx({ x: e.clientX, y: e.clientY, items: queueItems((q) => api.stopQueue(q.id), true) })}
          title="Stop a running queue"
        />
        <div className="divider" />
        <TB icon="batch" label="Batch" onClick={act.batch} title="Add a numbered series of files" />
        <div className="spacer" />
      </div>

      {(update || updateErr) && (
        <div className="topbanner">
          <Icon name="all" size={16} />
          {updating ? (
            <span>
              Updating to {update?.version}… {updating.total ? `${Math.round((updating.done / updating.total) * 100)}%` : ""} SwiftHarryDM restarts by itself and your downloads continue.
            </span>
          ) : (
            <>
              <span>{updateErr ? <span className="err">{updateErr}</span> : <>SwiftHarryDM {update?.version} is available.</>}</span>
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
                    icon="queue"
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
              <p>No downloads here.</p>
              <p>
                Press <b>Add URL</b>, paste a link with Ctrl+N, or drag a link onto this window.
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
            placeholder="File name, description or address"
            value={find.text}
            onChange={(e) => setFind({ ...find, text: e.target.value, msg: "" })}
            onKeyDown={(e) => {
              if (e.key === "Enter") doFind(find.text, anchor);
              if (e.key === "Escape") setFind({ ...find, open: false });
            }}
          />
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
function Modal({ title, children, foot, onClose }: { title: string; children: React.ReactNode; foot: React.ReactNode; onClose: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.25)", display: "grid", placeItems: "center", zIndex: 80 }} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div style={{ background: "var(--bg)", borderRadius: 8, boxShadow: "var(--shadow)", width: 460, maxWidth: "92vw", overflow: "hidden", border: "1px solid var(--border-strong)" }} role="dialog" aria-label={title}>
        <div style={{ padding: "12px 16px", fontWeight: 600, borderBottom: "1px solid var(--border)" }}>{title}</div>
        <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>{children}</div>
        <div className="dlg-foot">{foot}</div>
      </div>
    </div>
  );
}

function DeleteModal({ live, ids, onClose }: { live: Live; ids: string[]; onClose: () => void }) {
  const [files, setFiles] = useState(false);
  const jobs = ids.map((id) => live.jobMap.get(id)).filter(Boolean) as Job[];
  const unfinished = jobs.filter((j) => j.state !== "COMPLETED").length;
  return (
    <Modal
      title="Delete downloads"
      onClose={onClose}
      foot={
        <>
          <button className="btn primary" autoFocus onClick={() => { api.remove(ids, files); onClose(); }}>
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
