import { api, windowParams } from "../api";
import * as F from "../format";
import { Check, Dialog, Loading } from "../components/Dialog";
import { Icon, fileIcon } from "../icons";
import { useLive, useTheme } from "../store";

/** Small picture shown under the cursor while a file is dragged out. */
function dragImage(): string {
  const c = document.createElement("canvas");
  c.width = c.height = 48;
  const g = c.getContext("2d")!;
  g.fillStyle = "#ffffff";
  g.strokeStyle = "#2d6a9f";
  g.lineWidth = 2;
  g.beginPath();
  g.moveTo(10, 4);
  g.lineTo(30, 4);
  g.lineTo(40, 14);
  g.lineTo(40, 44);
  g.lineTo(10, 44);
  g.closePath();
  g.fill();
  g.stroke();
  g.beginPath();
  g.moveTo(30, 4);
  g.lineTo(30, 14);
  g.lineTo(40, 14);
  g.stroke();
  return c.toDataURL("image/png");
}

/** "Download complete": open the file or its folder. */
export default function Complete() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const id = windowParams().get("id") ?? "";
  if (!live) return <Loading />;
  const j = live.jobMap.get(id);
  if (!j) return <div className="dlg-body muted">This download is no longer in the list.</div>;
  return (
    <Dialog
      left={
        <Check checked={!live.settings.show_complete_dialog} onChange={(v) => api.saveSettings({ ...live.settings, show_complete_dialog: !v })}>
          Don’t show this again
        </Check>
      }
      foot={
        <>
          <button className="btn primary" autoFocus onClick={() => { api.openFile(id); api.closeSelf(); }}>Open</button>
          <button className="btn" onClick={() => { api.openWith(id); api.closeSelf(); }}>Open with…</button>
          <button className="btn" onClick={() => { api.openFolder(id); api.closeSelf(); }}>Open folder</button>
          <button className="btn" onClick={() => api.closeSelf()}>Close</button>
        </>
      }
    >
      <div className="row" style={{ alignItems: "flex-start", gap: 14 }}>
        <div
          style={{ color: "var(--ok)", cursor: j.final_path ? "grab" : undefined }}
          title={j.final_path ? "Drag the file to a folder or another program" : undefined}
          onMouseDown={(e) => {
            if (e.button === 0 && j.final_path) api.dragFile(j.final_path, dragImage()).catch(() => {});
          }}
        >
          <Icon name={fileIcon(j.category)} size={40} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }} className="kv">
          <span>File</span>
          <b title={F.fileName(j)}>{F.fileName(j)}</b>
          <span>Size</span>
          <span>{F.size(j.total ?? j.downloaded)} ({(j.total ?? j.downloaded).toLocaleString()} bytes)</span>
          <span>Saved as</span>
          <span title={j.final_path ?? ""}>{j.final_path}</span>
          <span>Address</span>
          <span title={j.url}>{j.url}</span>
        </div>
      </div>
    </Dialog>
  );
}
