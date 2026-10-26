import { useState } from "react";
import { api } from "../api";
import { Check, Dialog, Loading } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

const TIPS = [
  "Hold Alt while clicking a link in the browser to let the browser download it instead.",
  "Downloads start while the “Download file info” window is open, so a small file is often done before you press Start.",
  "Drag a finished file from the “Download complete” window straight into a folder or another program.",
  "Right-click a download and choose “Refresh download address” when a link has expired: the part already downloaded is kept.",
  "Queues can start and stop at set times. Open the Scheduler, pick a queue, and set its days and times.",
  "Speed limiter: Downloads › Speed limiter turns it on and off; each download can also have its own limit in its progress window.",
  "Copy a link to a file and SwiftHarryDM offers to download it, when “Offer to download links I copy” is on in Options.",
  "Tasks › Add batch download fetches numbered files like photo001.jpg to photo120.jpg in one go.",
  "Right-click a web page and choose “Download all links” to pick files from the page, filtered by type.",
  "Tasks › Export and Import move your list to and from other download managers that use .ef2 files.",
  "The Q column shows which queue a download belongs to. Drag downloads onto a queue on the left to move them.",
  "View › Font changes the font and size of every window.",
  "Use “Preview the files in this ZIP” in the “Download file info” window to see what an archive contains before downloading it.",
  "SwiftHarryDM starts with Windows and waits in the notification area, ready to take over downloads from your browser.",
];

/** Tip of the day. */
export default function Tips() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const [i, setI] = useState(() => Math.floor(Math.random() * TIPS.length));
  if (!live) return <Loading />;
  return (
    <Dialog
      left={
        <Check checked={live.settings.tips_on_startup} onChange={(v) => api.saveSettings({ ...live.settings, tips_on_startup: v })}>
          Show tips at startup
        </Check>
      }
      foot={
        <>
          <button className="btn" onClick={() => setI((i + 1) % TIPS.length)}>Next tip</button>
          <button className="btn primary" autoFocus onClick={() => api.closeSelf()}>Close</button>
        </>
      }
    >
      <div className="row" style={{ gap: 14, alignItems: "flex-start" }}>
        <div style={{ color: "var(--accent)" }}>
          <Icon name="link" size={32} />
        </div>
        <div>
          <div style={{ fontWeight: 600, marginBottom: 6 }}>Did you know…</div>
          <div>{TIPS[i]}</div>
        </div>
      </div>
    </Dialog>
  );
}
