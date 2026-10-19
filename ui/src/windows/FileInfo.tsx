import { useEffect, useMemo, useState } from "react";
import { api, windowParams, type Job, type MediaInfo, type UrlInfo } from "../api";
import * as F from "../format";
import { Dialog, Loading } from "../components/Dialog";
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
  const comps = useComponents();

  // A browser request that is closed without a decision counts as cancelled.
  useEffect(() => {
    if (!rid) return;
    const off = () => api.rejectRequest(rid);
    window.addEventListener("beforeunload", off);
    return () => window.removeEventListener("beforeunload", off);
  }, [rid]);

  // Inspect the address: video sites go to the media engine; other links get a quick file
  // check, and if that finds a web page, the media engine is asked whether it has a video.
  useEffect(() => {
    let alive = true;
    setProbing(true);
    setProbeErr("");
    setMedia(null);
    setInfo(null);
    const t = setTimeout(async () => {
      api.findDuplicates(url).then((d) => alive && setDups(d));
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
      } finally {
        if (alive) setProbing(false);
      }
    }, 300);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [url, comps.c?.ready]);

  const settings = live?.settings;
  const isMedia = media != null || (probeErr !== "" && !!comps.c && !comps.c.ytdlp && /video|media|yt-dlp/i.test(probeErr));
  const needsComponents = isMedia && !!comps.c && !comps.c.ready;
  const name = params.get("filename") || info?.file_name || "";
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

  const dup = dups[0];
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
          <button className="btn" onClick={() => { if (rid) api.rejectRequest(rid); api.closeSelf(); }}>Cancel</button>
        </>
      }
    >
      {rid && (
        <div className="banner">
          <Icon name="link" size={18} />
          <div>Sent from your browser{params.get("referer") ? <> by <b>{F.host(params.get("referer"))}</b></> : null}. Check the details and choose how to download it.</div>
        </div>
      )}
      {dup && settings.duplicate_policy === "ask" && (
        <div className="banner warn">
          <Icon name="warning" size={18} />
          <div style={{ flex: 1 }}>
            <div><b>This address is already in your list</b> ({F.statusText(dup)}).</div>
            <div className="row" style={{ marginTop: 6, flexWrap: "wrap" }}>
              {dup.state === "COMPLETED" ? (
                <button className="btn small" onClick={() => { api.openWindow("complete", { id: dup.id }); api.closeSelf(); }}>Show the finished file</button>
              ) : (
                <button className="btn small" onClick={() => { api.resume([dup.id]); api.openWindow("progress", { id: dup.id }); api.closeSelf(); }}>Resume the existing download</button>
              )}
              <span className="hint">or continue below to add it again under a numbered name.</span>
            </div>
          </div>
        </div>
      )}
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
      </div>
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
