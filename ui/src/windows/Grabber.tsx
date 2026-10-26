import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onGrabber, windowParams, type GrabberItem, type GrabberProject, type GrabberStats, type GrabberTemplate } from "../api";
import * as F from "../format";
import { Check, Loading, Num, Tabs } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

const TEMPLATES: [GrabberTemplate, string, string][] = [
  ["files", "All files", "Every file the pages link to: documents, archives, programs, media. Not the pages themselves."],
  ["images", "Images", "Pictures only (JPG, PNG, GIF, WebP, SVG…)."],
  ["audio", "Audio", "Music and sound files (MP3, FLAC, M4A, OGG…)."],
  ["video", "Video", "Video files linked from the pages (MP4, MKV, WebM…). Videos inside players are not files: use the browser button for those."],
  ["site", "Whole site", "The pages and everything they use, to keep a copy of the site."],
  ["custom", "Only the file types I list", "Choose the extensions on step 4."],
];

function blank(): GrabberProject {
  return {
    id: "",
    name: "",
    start_url: "",
    template: "files",
    save_mode: "categories",
    folder: "",
    keep_subfolders: false,
    depth: 1,
    other_sites_depth: 0,
    no_parent: true,
    whole_domain: false,
    include_paths: [],
    exclude_paths: [],
    file_types: [],
    exclude_types: [],
    files_this_site_only: false,
    hide_duplicates: true,
    min_size: 0,
    max_size: 0,
    start_at_once: false,
    max_pages: 500,
    parallel: 2,
    delay_ms: 250,
    respect_robots: true,
    created_at: 0,
    updated_at: 0,
  };
}

const lines = (v: string[]) => v.join("\n");
const unlines = (t: string) => t.split(/[\n,;]+/).map((x) => x.trim()).filter(Boolean);
const exts = (t: string) => t.split(/[\s,;]+/).map((x) => x.trim().replace(/^\*?\./, "").toLowerCase()).filter(Boolean);

/** IDM's Grabber: explore a site from a start page and download the files it links to. */
export default function Grabber() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const [projects, setProjects] = useState<GrabberProject[] | null>(null);
  const [curId, setCurId] = useState<string | null>(params.get("project"));
  const [editing, setEditing] = useState<GrabberProject | null>(null);

  const reload = useCallback(async () => {
    const list = await api.grabberProjects();
    setProjects(list);
    return list;
  }, []);
  useEffect(() => {
    reload().then((list) => {
      if (!params.get("project") && list.length === 0) setEditing(blank());
      else if (!params.get("project") && list.length) setCurId(list[0].id);
    });
  }, []);

  if (!live || !projects) return <Loading />;

  return (
    <div className="dlg">
      <div className="split-pane">
        <div style={{ width: 200, borderRight: "1px solid var(--border)", display: "flex", flexDirection: "column", background: "var(--surface-2)" }}>
          <div style={{ padding: "10px 10px 4px", fontWeight: 600 }}>Projects</div>
          <div style={{ flex: 1, overflow: "auto", padding: "0 6px" }} role={projects.length ? "listbox" : undefined} aria-label="Projects">
            {projects.map((p) => (
              <div
                key={p.id}
                role="option"
                aria-selected={p.id === curId && !editing}
                className={`tree-item${p.id === curId && !editing ? " sel" : ""}`}
                onMouseDown={() => { setEditing(null); setCurId(p.id); }}
                title={p.start_url}
              >
                <Icon className="ico" name="link" size={16} />
                <span className="ellipsis">{p.name}</span>
              </div>
            ))}
            {!projects.length && <div className="muted" style={{ padding: 6 }}>No projects yet.</div>}
          </div>
          <div className="row" style={{ padding: 8 }}>
            <button className="btn small" onClick={() => setEditing(blank())}>New project</button>
          </div>
        </div>
        {editing ? (
          <Wizard
            key={editing.id || "new"}
            initial={editing}
            onCancel={projects.length ? () => setEditing(null) : undefined}
            onDone={async (p, explore) => {
              const saved = await api.grabberSave(p);
              await reload();
              setEditing(null);
              setCurId(saved.id);
              if (explore) await api.grabberExplore(saved.id);
            }}
          />
        ) : curId ? (
          <ProjectView
            key={curId}
            id={curId}
            onEdit={(p) => setEditing(p)}
            onDeleted={async () => {
              const list = await reload();
              setCurId(list[0]?.id ?? null);
              if (!list.length) setEditing(blank());
            }}
          />
        ) : (
          <div className="pane muted">Choose a project, or start a new one.</div>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- the four steps
function Wizard({ initial, onDone, onCancel }: { initial: GrabberProject; onDone: (p: GrabberProject, explore: boolean) => Promise<void>; onCancel?: () => void }) {
  const [p, setP] = useState(initial);
  const [step, setStep] = useState<"1" | "2" | "3" | "4">("1");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const upd = (patch: Partial<GrabberProject>) => setP({ ...p, ...patch });
  const order = ["1", "2", "3", "4"] as const;
  const at = order.indexOf(step);
  const urlOk = /^https?:\/\/[^/\s]+/i.test(p.start_url.trim());

  const finish = async (explore: boolean) => {
    if (!urlOk) {
      setStep("1");
      setErr("Type the full address of the start page, starting with http:// or https://");
      return;
    }
    if (p.template === "custom" && !p.file_types.length) {
      setStep("4");
      setErr("List at least one file type, or choose another kind of files on step 1.");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      await onDone(p, explore);
    } catch (e) {
      setErr(String(e));
    }
    setBusy(false);
  };

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
      <Tabs
        tabs={[
          ["1", "1. Start page"],
          ["2", "2. Where to save"],
          ["3", "3. Pages to explore"],
          ["4", "4. Files to list"],
        ]}
        value={step}
        onChange={(v) => setStep(v as typeof step)}
      />
      <div className="pane" style={{ padding: "14px 18px", overflow: "auto", flex: 1, display: "flex", flexDirection: "column", gap: 12 }}>
        {step === "1" && (
          <>
            <div className="grid2">
              <span>Start page</span>
              <input type="text" autoFocus aria-label="Start page" placeholder="https://example.com/downloads/" value={p.start_url} onChange={(e) => upd({ start_url: e.target.value })} />
              <span>Project name</span>
              <input type="text" aria-label="Project name" placeholder="The site's name, if left empty" value={p.name} onChange={(e) => upd({ name: e.target.value })} />
            </div>
            <fieldset>
              <legend>What to look for</legend>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {TEMPLATES.map(([k, label, hint]) => (
                  <label key={k} style={{ alignItems: "flex-start" }}>
                    <input type="radio" name="template" checked={p.template === k} onChange={() => upd({ template: k })} />
                    <span>
                      {label}
                      <span className="hint" style={{ display: "block" }}>{hint}</span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
            <div className="hint">If the site needs a login, add it in Options › Site logins first; the grabber uses it.</div>
          </>
        )}
        {step === "2" && (
          <>
            <fieldset>
              <legend>Save the files</legend>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <label><input type="radio" name="save" checked={p.save_mode === "categories"} onChange={() => upd({ save_mode: "categories" })} /> In their category folders (Documents, Music, Video…)</label>
                <label><input type="radio" name="save" checked={p.save_mode === "folder"} onChange={() => upd({ save_mode: "folder" })} /> All in one folder:</label>
                <div className="row">
                  <input type="text" className="grow" aria-label="Folder" disabled={p.save_mode !== "folder"} placeholder="The download folder" value={p.folder} onChange={(e) => upd({ folder: e.target.value })} />
                  <button className="btn small" disabled={p.save_mode !== "folder"} onClick={async () => { const d = await api.pickFolder(p.folder || null); if (d) upd({ folder: d }); }}>Browse…</button>
                </div>
              </div>
            </fieldset>
            <Check checked={p.keep_subfolders} onChange={(v) => upd({ keep_subfolders: v })}>Recreate the site's folders (site name, then its folders)</Check>
            <div className="hint">A file that already exists is saved under a numbered name, never overwritten.</div>
          </>
        )}
        {step === "3" && (
          <>
            <div className="row">
              <span>Follow links</span>
              <Num value={p.depth} min={0} max={20} onChange={(v) => upd({ depth: v })} width={64} />
              <span>levels deep on this site (0 = the start page only)</span>
            </div>
            <div className="row">
              <span>Follow links</span>
              <Num value={p.other_sites_depth} min={0} max={5} onChange={(v) => upd({ other_sites_depth: v })} width={64} />
              <span>levels deep on other sites (0 = don't explore them)</span>
            </div>
            <Check checked={p.no_parent} onChange={(v) => upd({ no_parent: v })}>Stay in the start page's folder and below</Check>
            <Check checked={p.whole_domain} onChange={(v) => upd({ whole_domain: v })}>Count the domain's other addresses (e.g. files.example.com) as this site</Check>
            <Check checked={p.respect_robots} onChange={(v) => upd({ respect_robots: v })}>Respect the site's robots.txt</Check>
            <div className="grid2" style={{ alignItems: "start" }}>
              <span>Only pages matching</span>
              <textarea rows={2} className="mono" aria-label="Only pages matching" placeholder="Any page" value={lines(p.include_paths)} onChange={(e) => upd({ include_paths: unlines(e.target.value) })} />
              <span>Never pages matching</span>
              <textarea rows={2} className="mono" aria-label="Never pages matching" placeholder="*/print/*" value={lines(p.exclude_paths)} onChange={(e) => upd({ exclude_paths: unlines(e.target.value) })} />
            </div>
            <div className="row">
              <span>Stop after</span>
              <Num value={p.max_pages} min={1} max={100000} onChange={(v) => upd({ max_pages: v })} width={80} />
              <span>pages, reading</span>
              <Num value={p.parallel} min={1} max={8} onChange={(v) => upd({ parallel: v })} width={56} />
              <span>at a time</span>
            </div>
            <div className="hint">One address pattern per line, with * as a wildcard. Reading a few pages at a time, with a short pause, keeps the site's owner happy.</div>
          </>
        )}
        {step === "4" && (
          <>
            <div className="grid2">
              <span>{p.template === "custom" ? "File types" : "Also these types"}</span>
              <input type="text" className="mono" aria-label="File types" placeholder="pdf zip epub" value={p.file_types.join(" ")} onChange={(e) => upd({ file_types: exts(e.target.value) })} />
              <span>Never these types</span>
              <input type="text" className="mono" aria-label="Excluded file types" placeholder="exe msi" value={p.exclude_types.join(" ")} onChange={(e) => upd({ exclude_types: exts(e.target.value) })} />
            </div>
            <div className="row">
              <span>Size from</span>
              <Num value={Math.round(p.min_size / 1024)} min={0} onChange={(v) => upd({ min_size: v * 1024 })} width={90} />
              <span>KB to</span>
              <Num value={Math.round(p.max_size / 1024)} min={0} onChange={(v) => upd({ max_size: v * 1024 })} width={90} />
              <span>KB (0 = no limit)</span>
            </div>
            <Check checked={p.files_this_site_only} onChange={(v) => upd({ files_this_site_only: v })}>Only files on this site</Check>
            <Check checked={p.hide_duplicates} onChange={(v) => upd({ hide_duplicates: v })}>Hide files with the same name and size as one already listed</Check>
            <Check checked={p.start_at_once} onChange={(v) => upd({ start_at_once: v })}>Download each file as soon as it is found</Check>
          </>
        )}
        {err && <div className="err">{err}</div>}
      </div>
      <div className="dlg-foot">
        <div className="left">{onCancel && <button className="btn" onClick={onCancel}>Cancel</button>}</div>
        <button className="btn" disabled={at === 0} onClick={() => setStep(order[at - 1])}>Back</button>
        {at < 3 ? (
          <button className="btn primary" onClick={() => setStep(order[at + 1])}>Next</button>
        ) : (
          <>
            <button className="btn" disabled={busy} onClick={() => finish(false)}>Save</button>
            <button className="btn primary" disabled={busy} onClick={() => finish(true)}>Save and explore</button>
          </>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- a project
function ProjectView({ id, onEdit, onDeleted }: { id: string; onEdit: (p: GrabberProject) => void; onDeleted: () => void }) {
  const [project, setProject] = useState<GrabberProject | null>(null);
  const [items, setItems] = useState<Map<number, GrabberItem>>(new Map());
  const [running, setRunning] = useState(false);
  const [stats, setStats] = useState<GrabberStats | null>(null);
  const [done, setDone] = useState("");
  const [showPages, setShowPages] = useState(false);
  const [filter, setFilter] = useState("");
  const [msg, setMsg] = useState("");

  useEffect(() => {
    let alive = true;
    api.grabberState(id).then((s) => {
      if (!alive) return;
      setProject(s.project);
      setItems(new Map(s.items.map((i) => [i.id, i])));
      setRunning(s.running);
    });
    const un = onGrabber((e) => {
      if (e.project_id !== id) return;
      if (e.type === "item") setItems((m) => new Map(m).set(e.item.id, e.item));
      else if (e.type === "stats") setStats(e.stats);
      else {
        setRunning(false);
        setDone(e.reason);
      }
    });
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, [id]);

  const all = useMemo(() => [...items.values()], [items]);
  const files = all.filter((i) => i.kind === "file");
  const shown = all.filter((i) => (showPages || i.kind === "file") && (!filter || i.url.toLowerCase().includes(filter.toLowerCase()) || (i.link_text ?? "").toLowerCase().includes(filter.toLowerCase())));
  const checked = files.filter((i) => i.checked && !i.job_id);
  const total = checked.reduce((n, i) => n + (i.size ?? 0), 0);

  if (!project) return <Loading />;

  const explore = async () => {
    setDone("");
    setMsg("");
    setRunning(true);
    try {
      await api.grabberExplore(id);
    } catch (e) {
      setRunning(false);
      setMsg(String(e));
    }
  };
  const download = async () => {
    try {
      const jobs = await api.grabberDownload(id, checked.map((i) => i.id));
      setMsg(jobs.length ? `${jobs.length} file${jobs.length === 1 ? "" : "s"} added to the download list.` : "Nothing new to download.");
    } catch (e) {
      setMsg(String(e));
    }
  };
  const setAll = (v: boolean) => api.grabberCheck(id, shown.filter((i) => i.kind === "file" && !i.job_id).map((i) => i.id), v);

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
      <div className="row" style={{ padding: "10px 14px", gap: 8, borderBottom: "1px solid var(--border)" }}>
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ fontWeight: 600 }} className="ellipsis">{project.name}</div>
          <div className="muted ellipsis mono" title={project.start_url}>{project.start_url}</div>
        </div>
        {running ? (
          <button className="btn small" onClick={() => api.grabberStop(id)}>Stop</button>
        ) : (
          <button className="btn small primary" onClick={explore}>{files.length ? "Explore again" : "Explore"}</button>
        )}
        <button className="btn small" disabled={running} onClick={() => onEdit(project)}>Settings…</button>
        <button
          className="btn small danger"
          disabled={running}
          onClick={async () => {
            if (!confirm(`Delete the project “${project.name}”? Files already downloaded stay where they are.`)) return;
            await api.grabberDelete(id);
            onDeleted();
          }}
        >
          Delete
        </button>
      </div>
      <div className="row" style={{ padding: "6px 14px", gap: 10 }}>
        <span className={running ? "" : "muted"} aria-live="polite">
          {running
            ? `Exploring… ${stats ? `${stats.pages_explored} pages read, ${stats.pages_waiting} waiting, ${stats.files_found} files found` : ""}`
            : done
              ? `Exploring ${done === "finished" ? "finished" : done}. ${files.length} files listed.`
              : files.length
                ? `${files.length} files listed.`
                : "Not explored yet."}
          {stats && stats.errors > 0 ? ` ${stats.errors} pages could not be read.` : ""}
        </span>
        <span style={{ flex: 1 }} />
        <input type="text" aria-label="Filter" placeholder="Filter" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ width: 160 }} />
        <Check checked={showPages} onChange={setShowPages}>Show pages</Check>
      </div>
      <div className="tbl" style={{ margin: "0 14px 10px", minHeight: 0, flex: 1, overflow: "auto" }}>
        <table>
          <thead>
            <tr>
              <th style={{ width: 28 }} />
              <th>File</th>
              <th style={{ width: 90 }}>Size</th>
              <th style={{ width: 110 }}>Status</th>
              <th style={{ width: 220 }}>Link text</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((i) => (
              <tr key={i.id} className={i.kind === "page" ? "muted" : ""}>
                <td>
                  {i.kind === "file" && (
                    <input type="checkbox" aria-label={`Download ${F.nameFromUrl(i.url)}`} checked={i.checked} disabled={!!i.job_id} onChange={(e) => api.grabberCheck(id, [i.id], e.target.checked)} />
                  )}
                </td>
                <td className="ellipsis" title={i.url}>
                  {i.kind === "page" ? <span className="mono">{i.url}</span> : F.nameFromUrl(i.url)}
                </td>
                <td>{i.size != null ? F.size(i.size) : ""}</td>
                <td title={i.note ?? ""}>{statusText(i)}</td>
                <td className="ellipsis" title={i.link_text ?? ""}>{i.link_text ?? ""}</td>
              </tr>
            ))}
            {!shown.length && (
              <tr>
                <td colSpan={5} className="muted">{running ? "Looking…" : files.length ? "Nothing matches the filter." : "Explore the site to list its files."}</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <div className="dlg-foot">
        <div className="left">
          <button className="btn small" onClick={() => setAll(true)} disabled={!files.length}>Check all</button>
          <button className="btn small" onClick={() => setAll(false)} disabled={!files.length}>Uncheck all</button>
          <span className="hint" aria-live="polite">{msg}</span>
        </div>
        <span className="muted">{checked.length ? `${checked.length} checked${total ? `, ${F.size(total)}` : ""}` : ""}</span>
        <button className="btn primary" disabled={!checked.length} onClick={download}>Download checked</button>
      </div>
    </div>
  );
}

function statusText(i: GrabberItem): string {
  if (i.kind === "page") {
    return { explored: "Read", skipped: "Skipped", failed: "Could not read", found: "Waiting", queued: "Added" }[i.status];
  }
  return { found: "Found", queued: "Added to the list", explored: "Found", skipped: "Skipped", failed: "Failed" }[i.status];
}
