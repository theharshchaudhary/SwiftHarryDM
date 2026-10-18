import { useEffect, useState } from "react";
import { api, onCore, type Components } from "../api";
import * as F from "../format";
import { Icon } from "../icons";

/** Live status of the video components and a one-click install. */
export function useComponents() {
  const [c, setC] = useState<Components | null>(null);
  const [step, setStep] = useState<{ step: string; done: number; total: number | null } | null>(null);
  const [err, setErr] = useState("");
  const reload = () => api.componentsStatus().then(setC);
  useEffect(() => {
    reload();
    const un = onCore((e) => {
      if (e.type !== "components") return;
      if (e.step === "done" || e.step === "failed") {
        setStep(null);
        reload();
      } else setStep(e);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  const install = async () => {
    setErr("");
    setStep({ step: "yt-dlp", done: 0, total: null });
    try {
      await api.installComponents();
    } catch (e) {
      setErr(String(e));
      setStep(null);
    }
    reload();
  };
  return { c, step, err, install, reload };
}

const STEP_TEXT: Record<string, string> = {
  "yt-dlp": "Downloading the video downloader",
  ffmpeg: "Downloading FFmpeg",
  unpacking: "Unpacking FFmpeg",
};

/** Banner shown where video downloads need the components that are not installed yet. */
export function ComponentsBanner({ compact }: { compact?: boolean }) {
  const { c, step, err, install } = useComponents();
  if (!c || (c.ready && !step)) return null;
  const pct = step?.total ? (step.done / step.total) * 100 : null;
  return (
    <div className="banner">
      <Icon name="video" size={18} />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
        {step ? (
          <>
            <div>
              {STEP_TEXT[step.step] ?? "Preparing"}… {step.total ? `${F.size(step.done)} of ${F.size(step.total)}` : step.done ? F.size(step.done) : ""}
            </div>
            <div className={`bigbar${pct == null ? " indet" : ""}`} style={{ height: 8 }}>
              <div style={{ width: `${pct ?? 30}%` }} />
            </div>
          </>
        ) : (
          <>
            <div>
              Video and audio downloads need two free components (yt-dlp and FFmpeg).{" "}
              {!compact && "They are downloaded once (about 100 MB), checked, and kept up to date automatically."}
            </div>
            <div>
              <button className="btn small primary" onClick={install}>Download components</button>
            </div>
          </>
        )}
        {err && <div className="err">{err}</div>}
      </div>
    </div>
  );
}
