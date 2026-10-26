import { useEffect, useMemo, useRef, useState } from "react";
import { api, windowParams, type Job, type MediaInfo, type UrlInfo, type ZipEntry } from "../api";
import * as F from "../format";
import { Check, Dialog, Loading, Modal } from "../components/Dialog";
import { ComponentsBanner, useComponents } from "../components/Components";
import { Icon, categoryIcon } from "../icons";
import { useLive, useTheme } from "../store";

function joinPath(dir: string, name: string) {
  return dir.replace(/[\\/]+$/, "") + "\\" + name;
}
function splitPath(p: string): [string, string] {
  const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
  return i < 0 ? ["", p] : [p.slice(0, i), p.slice(i + 1)];
}

/** A download started while the window is open. */
type Early = { id: string; dir: string; file: string };

const PRESETS: [string, string][] = [
  ["best", "Best quality"],
  ["2160", "2160p (4K)"],
  ["1440", "1440p"],
  ["1080", "1080p"],
  ["720", "720p"],
  ["480", "480p"],
  ["360", "360p"],
  ["mp4", "MP4 (plays everywhere)"],
  ["mkv", "MKV (any codec)"],
  ["mp3", "MP3 audio"],
  ["m4a", "M4A audio"],
];
const AUDIO = new Set(["mp3", "m4a"]);

/** "Download File Info": confirm name, category and folder (or video quality), then start now or later. */
/** A download cancelled twice: stop taking over this site or address. */
function TwiceCancelled({ url, onClose }: { url: string; onClose: () => void }) {
  const live = useLive();
  const host = F.host(url);
  const [site, setSite] = useState(false);
  const [addr, setAddr] = useState(true);
  const [quiet, setQuiet] = useState(false);
  if (!live) return null;
  const s = live.settings;
  return (
    <Modal
      title="Cancelled twice"
      onClose={onClose}
      foot={
        <>
          <button
            className="btn primary"
            autoFocus
            onClick={async () => {
              await api.saveSettings({
                ...s,
                capture_skip_sites: site && !s.capture_skip_sites.includes(host) ? [...s.capture_skip_sites, host] : s.capture_skip_sites,
                capture_skip_urls: addr && !s.capture_skip_urls.includes(url) ? [...s.capture_skip_urls, url] : s.capture_skip_urls,
                offer_exception_on_cancel: !quiet,
              });
              onClose();
            }}
          >
            OK
          </button>
          <button className="btn" onClick={onClose}>Cancel</button>
        </>
      }
    >
      <div>You have cancelled this download twice. If Keepfetch takes it over by mistake, the browser can keep it from now on:</div>
      <Check checked={site} onChange={setSite}>Don’t take over downloads from this site: <b>{host}</b></Check>
      <Check checked={addr} onChange={setAddr}>Don’t take over this address</Check>
      <div className="hint">Both lists are on Options › File types.</div>
      <Check checked={quiet} onChange={setQuiet}>Don’t show this again</Check>
    </Modal>
  );
}

/** Zip preview: the archive's files, read from the server without downloading it. */
function ZipList({ entries, onClose }: { entries: ZipEntry[] | string; onClose: () => void }) {
  return (
    <Modal title="Files in this ZIP" onClose={onClose} foot={<button className="btn primary" autoFocus onClick={onClose}>OK</button>}>
      {typeof entries === "string" ? (
        <div className="err">{entries}</div>
      ) : entries.length === 0 ? (
        <div className="muted">Reading the archive’s list of files…</div>
      ) : (
        <>
          <div style={{ maxHeight: 300, overflow: "auto", border: "1px solid var(--border)", borderRadius: 4 }}>
            <table className="list" style={{ width: "100%" }}>
              <thead>
                <tr><th style={{ textAlign: "left" }}>Name</th><th style={{ textAlign: "right" }}>Size</th></tr>
              </thead>
              <tbody>
                {entries.map((e, i) => (
                  <tr key={i}>
                    <td>{e.name}</td>
                    <td style={{ textAlign: "right" }}>{e.is_dir ? "" : F.size(e.size)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="hint">{entries.filter((e) => !e.is_dir).length} files, {F.size(entries.reduce((a, e) => a + e.size, 0))} when unpacked.</div>
        </>
      )}
    </Modal>
  );
}

/** The duplicate-link choices. */
function DuplicateDialog({ job, onChoose, onCancel }: { job: Job; onChoose: (p: "number" | "overwrite" | "resume", remember: boolean) => void; onCancel: () => void }) {
  const [pick, setPick] = useState<"number" | "overwrite" | "resume">("number");
  const [keep, setKeep] = useState(false);
  const opts: [typeof pick, string][] = [
    ["number", "Add it again with a numbered file name"],
    ["overwrite", "Add it again and replace the existing file"],
    ["resume", job.state === "COMPLETED" ? "Show the finished file instead" : "Resume the existing download instead"],
  ];
  return (
    <Modal
      title="This address is already in your list"
      onClose={onCancel}
      foot={
        <>
          <button className="btn primary" autoFocus onClick={() => onChoose(pick, keep)}>OK</button>
          <button className="btn" onClick={onCancel}>Cancel</button>
        </>
      }
    >
      <div>
        <b>{F.fileName(job)}</b> <span className="muted">({F.statusText(job)})</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {opts.map(([k, l]) => (
          <label key={k} className="row">
            <input type="radio" checked={pick === k} onChange={() => setPick(k)} /> {l}
          </label>
        ))}
      </div>
      <Check checked={keep} onChange={setKeep}>Remember my choice (Options › Downloads)</Check>
    </Modal>
  );
}

/** How far the download that started while this window is open has got. */
function EarlyProgress({ id }: { id: string }) {
  const live = useLive();
  const p = live?.progressMap.get(id);
  const j = live?.jobMap.get(id);
  if (j?.state === "COMPLETED") return <span className="ok">Already finished. Choose where to keep it.</span>;
  if (!p) return <span className="muted">Starting…</span>;
  const pct = p.total ? Math.floor((p.downloaded / p.total) * 100) : null;
  return (
    <span className="muted">
      {F.size(p.downloaded)}
      {pct != null ? ` (${pct}%)` : ""} at {F.speed(p.speed)}
    </span>
  );
}

export default function FileInfo() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const rid = params.get("rid");
  const [url, setUrl] = useState(params.get("url") ?? "");
  const [info, setInfo] = useState<UrlInfo | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [probeErr, setProbeErr] = useState("");
  const [probing, setProbing] = useState(true);
  const [category, setCategory] = useState<string>("");
  const [path, setPath] = useState("");
  const [pathTouched, setPathTouched] = useState(false);
  const [desc, setDesc] = useState(params.get("description") ?? "");
  const [queueId, setQueueId] = useState("main");
  const [quality, setQuality] = useState("best");
  const [dups, setDups] = useState<Job[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [remember, setRemember] = useState(false);
  // The duplicate-link choice, made once per window (or taken from Options).
  const [dupDone, setDupDone] = useState(false);
  const [twice, setTwice] = useState(false);
  const [zip, setZip] = useState<ZipEntry[] | string | null>(null);
  // The file starts downloading while this window is open; the choices made here
  // are applied when "Start download" or "Download later" is pressed.
  const [early, setEarly] = useState<Early | null>(null);
  const decided = useRef(false);
  // The early start, while it is being set up and after: every decision waits for it, so a
  // quick Cancel or Start never leaves a download behind or answers the browser twice.
  const earlyStart = useRef<Promise<Early | null> | null>(null);
  const settled = async () => (earlyStart.current ? await earlyStart.current : null);
  const [dupsChecked, setDupsChecked] = useState(false);
  // The same file as a download that stopped (its link expired, say): offer to continue it.
  const [stoppedDone, setStoppedDone] = useState(false);
  const comps = useComponents();

  // Closed without a decision: a browser request counts as rejected, and a download that
  // started early is removed with its partial data.
  useEffect(() => {
    const off = () => {
      if (decided.current) return;
      if (rid) api.rejectRequest(rid);
      settled().then((e) => e && api.remove([e.id], true));
    };
    window.addEventListener("beforeunload", off);
    return () => window.removeEventListener("beforeunload", off);
  }, [rid]);

  // Same address already in the list? Asked once per address, before anything starts: this
  // window's own early download must never count as a duplicate (the check below runs again
  // when the video components or a login change).
  useEffect(() => {
    let alive = true;
    setDupsChecked(false);
    api.findDuplicates(url).then((d) => {
      if (!alive) return;
      setDups(d);
      setDupsChecked(true);
    });
    return () => {
      alive = false;
    };
  }, [url]);

  // Inspect the address: video sites go to the media engine; other links get a quick file
  // check, and if that finds a web page, the media engine is asked whether it has a video.
  useEffect(() => {
    let alive = true;
    setProbing(true);
    setProbeErr("");
    setMedia(null);
    setInfo(null);
    const t = setTimeout(async () => {
      try {
        if (await api.isMediaUrl(url)) {
          const m = await api.inspectMedia(url);
          if (alive) setMedia(m);
        } else {
          const i = await api.inspectUrl(url);
          if (!alive) return;
          setInfo(i);
          if (i.is_web_page) {
            const m = await api.inspectMedia(url).catch(() => null);
            if (alive && m) setMedia(m);
          }
        }
      } catch (e) {
        if (alive) setProbeErr(String(e));
        // The server wants a login: ask now, and check again once it is saved.
        if (alive && /\(401\)/.test(String(e))) api.openWindow("login", { url });
      } finally {
        if (alive) setProbing(false);
      }
    }, 300);
    return () => {
      alive = false;
      clearTimeout(t);
    };
    // Checked again after a login was entered for the site.
  }, [url, comps.c?.ready, live?.settings.site_logins.length]);

  const settings = live?.settings;
  const isMedia = media != null || (probeErr !== "" && !!comps.c && !comps.c.ytdlp && /video|media|yt-dlp/i.test(probeErr));
  const needsComponents = isMedia && !!comps.c && !comps.c.ready;
  // When the check fails, the last part of the address is the name (the download still runs).
  const name = params.get("filename") || info?.file_name || (probeErr && !isMedia ? F.nameFromUrl(url) : "");
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

  // Default category and path (unless the user changed them).
  useEffect(() => {
    if (!settings || pathTouched) return;
    if (isMedia) {
      const cat = settings.categories.some((c) => c.name === (AUDIO.has(quality) ? "Music" : "Video")) ? (AUDIO.has(quality) ? "Music" : "Video") : "";
      setCategory(cat);
      setPath(cat ? folderFor(cat) : settings.download_dir);
      return;
    }
    const cat = info?.category ?? "";
    setCategory(cat);
    if (name) setPath(joinPath(cat ? folderFor(cat) : settings.download_dir, name));
  }, [info, name, settings, folderFor, pathTouched, isMedia, quality]);

  const stopped = useMemo(() => {
    if (!info || info.is_web_page || isMedia || !live) return null;
    const n = info.file_name.toLowerCase();
    return (
      live.jobs.find(
        (j) =>
          (j.state === "FAILED" || j.state === "PAUSED") &&
          F.fileName(j).toLowerCase() === n &&
          (j.total == null || info.size == null || j.total === info.size) &&
          j.url !== url &&
          j.url !== info.final_url,
      ) ?? null
    );
  }, [info, isMedia, live, url]);
  const stoppedAsk = !!stopped && !stoppedDone;
  const continueStopped = async () => {
    if (!stopped || !info) return;
    setBusy(true);
    try {
      await api.refreshUrl(stopped.id, info.final_url || url);
      await (stopped.state === "FAILED" ? api.retry([stopped.id]) : api.resume([stopped.id]));
      decided.current = true;
      if (rid) api.rejectRequest(rid); // answered: the download continues in the old entry
      api.closeSelf();
    } catch (e) {
      setErr(String(e));
      setStoppedDone(true);
    }
    setBusy(false);
  };

  // Start early once the address is known to be a file (not a video page) and nothing needs
  // asking first (a duplicate, or a stopped download it may belong to).
  const dupAsk = dups.length > 0 && !dupDone;
  useEffect(() => {
    if (!settings?.start_while_info || earlyStart.current || decided.current || probing || probeErr || !info || info.is_web_page || isMedia || !dupsChecked || dupAsk || stoppedAsk || !path) return;
    const [dir, file] = splitPath(path);
    earlyStart.current = (async () => {
      try {
        const id = rid
          ? await api.resolveRequest(rid, { dest_dir: dir, filename: file, category: category || null, mode: "now", quiet: true })
          : await api.submit(
              { url, dest_dir: dir, kind: "auto", filename: file, media: null, format_label: "best", referer: params.get("referer"), source: "info", category: category || null, description: null, queue_id: null },
              "now",
            );
        api.hold(id);
        const e = { id, dir, file };
        setEarly(e);
        return e;
      } catch {
        return null; // nothing started: "Start download" starts it as usual
      }
    })();
  }, [settings?.start_while_info, probing, probeErr, info, isMedia, dupsChecked, dupAsk, stoppedAsk, path, rid, url, category, params]);

  // A choice saved in Options is applied without asking.
  useEffect(() => {
    const p = settings?.duplicate_policy;
    if (!dups.length || dupDone || !p || p === "ask") return;
    applyDuplicate(p);
  }, [dups, dupDone, settings?.duplicate_policy]);

  const applyDuplicate = async (p: "number" | "overwrite" | "resume") => {
    const d = dups[0];
    if (p === "resume") {
      decided.current = true;
      if (rid) await api.rejectRequest(rid);
      if (d.state === "COMPLETED") await api.openWindow("complete", { id: d.id });
      else {
        await api.resume([d.id]);
        await api.openWindow("progress", { id: d.id });
      }
      api.closeSelf();
      return;
    }
    if (p === "overwrite") await api.remove(dups.map((x) => x.id), true);
    setDupDone(true);
  };

  if (!live || !settings) return <Loading />;

  const changeCategory = (c: string) => {
    setCategory(c);
    if (isMedia) setPath(c ? folderFor(c) : settings.download_dir);
    else {
      const [, n] = splitPath(path || name);
      setPath(joinPath(c ? folderFor(c) : settings.download_dir, n || name));
    }
  };

  const browse = async () => {
    const dir = isMedia ? path : splitPath(path)[0];
    const picked = await api.pickFolder(dir || settings.download_dir);
    if (picked) {
      setPath(isMedia ? picked : joinPath(picked, splitPath(path)[1] || name));
      setPathTouched(true);
    }
  };

  const exact = quality.startsWith("f:") ? quality.slice(2) : null;
  const preset = exact ? "best" : quality;

  const go = async (mode: "now" | "later") => {
    setErr("");
    const [dir, file] = isMedia ? [path, ""] : splitPath(path);
    if (!dir) {
      setErr("Choose a folder to save to.");
      return;
    }
    setBusy(true);
    try {
      if (remember) {
        // "Remember this path": the category's folder (or the main folder for General).
        const s = { ...settings };
        if (category) s.categories = s.categories.map((c) => (c.name === category ? { ...c, folder: dir } : c));
        else s.download_dir = dir;
        await api.saveSettings(s);
      }
      decided.current = true;
      const early = await settled();
      if (early) {
        const id = early.id;
        // "Later": stop first, so moving the file does not start it again.
        if (mode === "later") await api.pause([id]);
        if (dir !== early.dir || (!isMedia && file !== early.file)) await api.relocate(id, dir, isMedia ? null : file || null);
        if (desc.trim()) await api.setDescription(id, desc.trim());
        if (mode === "later") await api.moveToQueue([id], queueId);
        // After the move has been applied, so a finished file is shown where it now is.
        await api.release(id, mode === "now");
        api.closeSelf();
        return;
      }
      if (rid) {
        await api.resolveRequest(rid, {
          dest_dir: dir,
          filename: isMedia ? null : file || null,
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
          kind: isMedia ? "media" : "auto",
          filename: isMedia ? null : file || null,
          media: isMedia ? { preset, format_id: exact, max_height: null } : null,
          format_label: isMedia ? preset : "best",
          referer: params.get("referer"),
          source: params.get("source") ?? "ui",
          category: category || null,
          description: desc.trim() || (isMedia ? media?.title ?? null : null),
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

  const sizeText = (() => {
    if (probing) return "…";
    if (probeErr && isMedia && !media) return "Known after the video component is installed";
    if (probeErr) return `Unknown: ${probeErr.replace(/\.$/, "")}. The download will still be tried (with retries).`;
    if (isMedia) {
      if (!media) return "Known after the video component is installed";
      const f = exact ? media.formats.find((x) => x.id === exact) : null;
      if (f?.size) return `About ${F.size(f.size)}`;
      return media.duration ? `Length ${F.duration(media.duration)}; the size depends on the quality` : "Depends on the quality";
    }
    return info?.size != null ? `${F.size(info.size)} (${info.size.toLocaleString()} bytes)` : "Unknown (the server did not say)";
  })();

  return (
    <Dialog
      left={
        <span className="hint" title={info?.content_type ?? undefined}>
          {probing ? "Checking the address…" : isMedia ? `Video from ${media?.extractor || F.host(url)}` : info ? `${info.resumable ? "Resumable" : "Not resumable"}${info.content_type ? ` · ${info.content_type.split(";")[0]}` : ""}` : ""}
        </span>
      }
      foot={
        <>
          <button className="btn primary" disabled={busy || !url || probing || needsComponents} onClick={() => go("now")}>
            Start download
          </button>
          <span className="row" style={{ gap: 0 }}>
            <button className="btn" disabled={busy || !url || probing || needsComponents} onClick={() => go("later")} style={{ borderTopRightRadius: 0, borderBottomRightRadius: 0 }}>
              Download later
            </button>
            <select value={queueId} onChange={(e) => setQueueId(e.target.value)} title="Queue for “Download later”" style={{ height: 30, borderTopLeftRadius: 0, borderBottomLeftRadius: 0, marginLeft: -1 }}>
              {live.queues.map((q) => (
                <option key={q.id} value={q.id}>{q.name}</option>
              ))}
            </select>
          </span>
          <button
            className="btn"
            onClick={async () => {
              decided.current = true;
              // Wait for an early start still being set up, then undo it (before closing: a
              // closing window may drop calls still on their way).
              const e = await settled();
              if (rid && !e) await api.rejectRequest(rid);
              if (e) await api.remove([e.id], true).catch(() => {});
              // A browser download cancelled a second time: offer the exceptions.
              if (rid && settings.offer_exception_on_cancel && !twice && (await api.countCancel(url)) >= 2) {
                setTwice(true);
                return;
              }
              api.closeSelf();
            }}
          >
            Cancel
          </button>
        </>
      }
    >
      {rid && (
        <div className="banner">
          <Icon name="link" size={18} />
          <div>Sent from your browser{params.get("referer") ? <> by <b>{F.host(params.get("referer"))}</b></> : null}. Check the details and choose how to download it.</div>
        </div>
      )}
      {stoppedAsk && stopped && (
        <div className="banner warn" role="status">
          <Icon name="warning" size={18} />
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div>
              This looks like <b>{F.fileName(stopped)}</b>, which stopped{F.percent(stopped.downloaded, stopped.total) != null ? ` at ${Math.floor(F.percent(stopped.downloaded, stopped.total)!)}%` : ""}
              {stopped.error ? ` (${stopped.error.message})` : ""}. Continue it from this new address and keep what is already downloaded?
            </div>
            <div className="row">
              <button className="btn small primary" disabled={busy} onClick={continueStopped}>Continue the stopped download</button>
              <button className="btn small" disabled={busy} onClick={() => setStoppedDone(true)}>Download it again as a new file</button>
            </div>
          </div>
        </div>
      )}
      {twice && <TwiceCancelled url={url} onClose={() => api.closeSelf()} />}
      {dupAsk && settings.duplicate_policy === "ask" && <DuplicateDialog job={dups[0]} onChoose={async (p, keep) => {
        if (keep) await api.saveSettings({ ...settings, duplicate_policy: p });
        await applyDuplicate(p);
      }} onCancel={async () => {
        decided.current = true;
        if (rid) await api.rejectRequest(rid);
        api.closeSelf();
      }} />}
      <div className="grid2">
        <span>Address</span>
        <input type="text" value={url} onChange={(e) => setUrl(e.target.value.trim())} />
        {isMedia && media && (
          <>
            <span>Video</span>
            <b className="ellipsis" title={media!.title}>{media!.title}</b>
            <span>Quality</span>
            <select value={quality} disabled={!!rid} onChange={(e) => { setQuality(e.target.value); }}>
              <optgroup label="Presets">
                {PRESETS.map(([k, l]) => <option key={k} value={k}>{l}</option>)}
              </optgroup>
              {media!.formats.length > 0 && (
                <optgroup label="Exact formats">
                  {media!.formats.map((f) => (
                    <option key={f.id} value={`f:${f.id}`}>
                      {[f.height ? `${f.height}p` : "audio", f.ext, f.vcodec && !f.acodec ? "video only (best audio added)" : "", f.size ? F.size(f.size) : "", f.note].filter(Boolean).join(" · ")}
                    </option>
                  ))}
                </optgroup>
              )}
            </select>
          </>
        )}
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
        <span>{isMedia ? "Save in" : "Save as"}</span>
        <div className="row">
          <input type="text" className="grow" value={path} placeholder={probing ? "…" : ""} onChange={(e) => { setPath(e.target.value); setPathTouched(true); }} />
          <button className="btn small" onClick={browse}>Browse…</button>
        </div>
        <span>Description</span>
        <input type="text" value={desc} onChange={(e) => setDesc(e.target.value)} />
        <span>Size</span>
        <span className="muted">{sizeText}</span>
        {early && (
          <>
            <span>Downloaded</span>
            <EarlyProgress id={early.id} />
          </>
        )}
      </div>
      {!isMedia && info?.resumable && info.size != null && /\.zip$/i.test(splitPath(path)[1] || name) && (
        <div>
          <button
            className="btn small"
            onClick={() => {
              setZip([]);
              api.zipPreview(info.final_url, info.size!).then(setZip, (e) => setZip(String(e)));
            }}
          >
            Preview the files in this ZIP…
          </button>
        </div>
      )}
      {zip !== null && <ZipList entries={zip} onClose={() => setZip(null)} />}
      <Check checked={remember} onChange={setRemember}>
        Remember this folder for the “{category || "General"}” category
      </Check>
      {isMedia && <ComponentsBanner />}
      {isMedia && media && <div className="hint">The file is named after the video. Protected (DRM) videos, playlists and live streams are not downloaded.</div>}
      {!isMedia && info?.is_web_page && !probing && (
        <div className="banner">
          <Icon name="video" size={18} />
          <div>This address opens a web page, not a file, and no video was found on it.</div>
        </div>
      )}
      {err && <div className="err">{err}</div>}
    </Dialog>
  );
}
