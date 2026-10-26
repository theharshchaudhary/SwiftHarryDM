import { useState } from "react";
import { api, windowParams } from "../api";
import * as F from "../format";
import { Dialog, Loading } from "../components/Dialog";
import { Icon } from "../icons";
import { useLive, useTheme } from "../store";

/** The server asked for a login. It is saved in the site
 * logins (encrypted) and the download is tried again (or, opened from "Download file info"
 * with only an address, that window checks the address again). */
export default function Login() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const params = windowParams();
  const id = params.get("id") ?? "";
  const realm = params.get("realm") ?? "";
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [err, setErr] = useState("");
  if (!live) return <Loading />;
  const j = live.jobMap.get(id);
  // Site logins are matched by host name, without the port.
  const host = (() => {
    try {
      return new URL(j?.final_url || j?.url || params.get("url") || "").hostname;
    } catch {
      return "";
    }
  })();

  const ok = async () => {
    if (!user) {
      setErr("Type the user name.");
      return;
    }
    try {
      const s = live.settings;
      const password_enc = await api.protectPassword(pass);
      const others = s.site_logins.filter((l) => l.host.toLowerCase() !== host.toLowerCase());
      await api.saveSettings({ ...s, site_logins: [...others, { host, username: user, password_enc }] });
      if (j) await api.retry([id]);
      api.closeSelf();
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <Dialog
      foot={
        <>
          <button className="btn primary" onClick={ok}>OK</button>
          <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <div className="row" style={{ gap: 12, alignItems: "flex-start" }}>
        <Icon name="lock" size={28} />
        <div>
          <b>{host}</b> asks for a user name and password{j ? <> to download <b>{F.fileName(j)}</b></> : null}.
          {realm && <div className="muted">Site says: {realm}</div>}
        </div>
      </div>
      <div className="grid2">
        <span>User name</span>
        <input type="text" autoFocus value={user} onChange={(e) => setUser(e.target.value)} onKeyDown={(e) => e.key === "Enter" && ok()} />
        <span>Password</span>
        <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} onKeyDown={(e) => e.key === "Enter" && ok()} />
      </div>
      <div className="hint">The login is saved, encrypted, in Options › Site logins and used for this site from now on.</div>
      {err && <div className="err">{err}</div>}
    </Dialog>
  );
}
