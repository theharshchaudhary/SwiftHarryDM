import { useEffect, useState } from "react";
import { api } from "../api";
import { Dialog } from "../components/Dialog";
import { useLive, useTheme } from "../store";

/** "Enter new address to download": a single field, prefilled from the clipboard. */
export default function AddUrl() {
  const live = useLive();
  useTheme(live?.settings.theme);
  const [url, setUrl] = useState("");
  const [err, setErr] = useState("");
  useEffect(() => {
    api.clipboardUrls().then((u) => u.length && setUrl((cur) => cur || u[0]));
  }, []);
  const ok = async () => {
    const urls = await api.extractUrls(url);
    if (!urls.length) {
      setErr("Enter a full http:// or https:// address.");
      return;
    }
    if (urls.length > 1) await api.openWindow("links", { source: "typed", urls: JSON.stringify(urls) });
    else await api.openWindow("info", { url: urls[0], source: "ui" });
    api.closeSelf();
  };
  return (
    <Dialog
      foot={
        <>
          <button className="btn primary" onClick={ok} disabled={!url.trim()}>OK</button>
          <button className="btn" onClick={() => api.closeSelf()}>Cancel</button>
        </>
      }
    >
      <label htmlFor="u">Address</label>
      <input
        id="u"
        type="text"
        autoFocus
        value={url}
        placeholder="https://example.com/file.zip"
        onChange={(e) => {
          setUrl(e.target.value);
          setErr("");
        }}
        onKeyDown={(e) => e.key === "Enter" && ok()}
        onFocus={(e) => e.target.select()}
      />
      {err ? <div className="err">{err}</div> : <div className="hint">Several addresses can be pasted at once; you can then choose which ones to download.</div>}
    </Dialog>
  );
}
