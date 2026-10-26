import { useEffect, useState } from "react";
import { getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";
import { Menu } from "@tauri-apps/api/menu";
import { api } from "../api";

/** Floating drop target: drag links here to download them. Drag the circle to move it. */
export default function Drop() {
  const [over, setOver] = useState(false);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    // Come back where the user left it.
    const w = getCurrentWindow();
    try {
      const saved = JSON.parse(localStorage.getItem("sdm.dropPos") ?? "null");
      if (saved) w.setPosition(new PhysicalPosition(saved.x, saved.y));
    } catch {
      /* default position */
    }
    const un = w.onMoved(({ payload }) => {
      try {
        localStorage.setItem("sdm.dropPos", JSON.stringify({ x: payload.x, y: payload.y }));
      } catch {
        /* not remembered */
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const drop = async (e: React.DragEvent) => {
    e.preventDefault();
    setOver(false);
    const text = e.dataTransfer.getData("text/uri-list") || e.dataTransfer.getData("text/plain") || "";
    const urls = await api.extractUrls(text);
    if (urls.length === 1) api.openWindow("info", { url: urls[0] });
    else if (urls.length > 1) api.openWindow("links", { source: "drop", urls: JSON.stringify(urls) });
  };

  return (
    <div
      style={{ width: "100vw", height: "100vh", display: "grid", placeItems: "center", background: "transparent" }}
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={drop}
      onContextMenu={async (e) => {
        e.preventDefault();
        // A native menu: the window is too small to hold one.
        const m = await Menu.new({
          items: [
            { id: "open", text: "Open Keepfetch", action: () => api.showMain() },
            {
              id: "hide",
              text: "Hide drop target",
              action: () => {
                try {
                  localStorage.setItem("sdm.dropTarget", "false");
                } catch {
                  /* ignore */
                }
                api.closeSelf();
              },
            },
          ],
        });
        await m.popup();
      }}
    >
      <div
        data-tauri-drag-region
        title="Drop links here to download them. Drag to move; double-click to open Keepfetch."
        onDoubleClick={() => api.showMain()}
        style={{
          width: 64,
          height: 64,
          borderRadius: "50%",
          background: over ? "#7c3aed" : "#6d28d9",
          boxShadow: "0 3px 10px rgba(0,0,0,.35)",
          display: "grid",
          placeItems: "center",
          cursor: "move",
          transform: over ? "scale(1.08)" : "none",
          transition: "transform .12s, background .12s",
        }}
      >
        <svg data-tauri-drag-region width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
          <path data-tauri-drag-region d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14" />
        </svg>
      </div>
    </div>
  );
}
