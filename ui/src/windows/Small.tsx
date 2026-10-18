import { useEffect, useState } from "react";
import { api, windowParams, type AfterAction } from "../api";
import { Dialog, Loading } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

export function About() {
  const live = useLive();
  useTheme(live?.settings.theme);
  if (!live) return <Loading />;
  return (
    <Dialog foot={<button className="btn primary" autoFocus onClick={() => api.closeSelf()}>Close</button>}>
      <div className="row" style={{ gap: 14 }}>
        <div style={{ color: "var(--accent)" }}>
          <Icon name="all" size={48} />
        </div>
        <div>
          <div style={{ fontSize: 18, fontWeight: 600 }}>SwiftHarryDM</div>
          <div className="muted">Version {live.version}</div>
        </div>
      </div>
      <div>A fast, resumable download manager for Windows: segmented downloads, queues and schedules, and browser integration.</div>
      <div className="hint">Open-source components are listed in THIRD_PARTY_NOTICES.md next to the program.</div>
    </Dialog>
  );
}

const ACTION_TEXT: Record<AfterAction, string> = {
  none: "",
  exit: "SwiftHarryDM will close",
  sleep: "The computer will go to sleep",
  hibernate: "The computer will hibernate",
  shutdown: "The computer will shut down",
};

/** Warning before a power action, with a countdown that can be cancelled. */
export function Countdown() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const action = (params.get("action") ?? "none") as AfterAction;
  const what = params.get("what") ?? "The downloads have finished.";
  const [left, setLeft] = useState(30);
  useEffect(() => {
    if (left <= 0) {
      api.powerAction(action).finally(() => api.closeSelf());
      return;
    }
    const t = setTimeout(() => setLeft(left - 1), 1000);
    return () => clearTimeout(t);
  }, [left, action]);
  return (
    <Dialog
      foot={
        <>
          <button className="btn" onClick={() => setLeft(0)}>Do it now</button>
          <button className="btn primary" autoFocus onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <div>{what}</div>
      <div>{ACTION_TEXT[action]} in</div>
      <div className="countdown">{left} s</div>
    </Dialog>
  );
}

/** Placeholder shown when the window kind is unknown. */
export function Unknown() {
  const w = windowParams().get("w");
  return <div className="dlg-body muted">Unknown window “{w}”.</div>;
}
