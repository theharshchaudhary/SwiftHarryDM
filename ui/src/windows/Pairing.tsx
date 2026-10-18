import { api, windowParams } from "../api";
import { Dialog } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

/** Shows the one-time code a browser extension needs to connect. */
export default function Pairing() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const p = windowParams();
  const pid = p.get("pid") ?? "";
  const code = p.get("code") ?? "";
  const deny = async () => {
    await api.denyPairing(pid);
    api.closeSelf();
  };
  return (
    <Dialog
      onEscape={deny}
      foot={
        <>
          <button className="btn danger" onClick={deny}>Don’t allow</button>
          <button className="btn primary" autoFocus onClick={() => api.closeSelf()}>Close</button>
        </>
      }
    >
      <div className="row" style={{ gap: 12 }}>
        <div style={{ color: "var(--accent)" }}><Icon name="link" size={34} /></div>
        <div>A browser extension wants to connect to SwiftHarryDM. To allow it, type this code in the extension:</div>
      </div>
      <div className="countdown mono" style={{ letterSpacing: "0.25em", fontSize: 38 }}>{code.slice(0, 3)} {code.slice(3)}</div>
      <div className="hint">The code works once and expires in 2 minutes. If you did not start this, choose “Don’t allow”.</div>
      <div className="hint mono ellipsis" title={p.get("ext") ?? ""}>Extension ID: {p.get("ext")}</div>
    </Dialog>
  );
}
