import { useEffect, type ReactNode } from "react";
import { api } from "../api";

/** Frame for a dialog window: scrolling body and a button row. Escape closes the window. */
export function Dialog({ children, foot, left, onEscape }: { children: ReactNode; foot: ReactNode; left?: ReactNode; onEscape?: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") (onEscape ?? (() => api.closeSelf()))();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onEscape]);
  return (
    <div className="dlg">
      <div className="dlg-body">{children}</div>
      <div className="dlg-foot">
        {left && <div className="left">{left}</div>}
        {foot}
      </div>
    </div>
  );
}

export function Tabs<T extends string>({ tabs, value, onChange, vertical }: { tabs: [T, string][]; value: T; onChange: (t: T) => void; vertical?: boolean }) {
  return (
    <div className={`tabs${vertical ? " vertical" : ""}`} role="tablist">
      {tabs.map(([k, label]) => (
        <button key={k} role="tab" aria-selected={value === k} className={value === k ? "on" : ""} onClick={() => onChange(k)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function Check({ checked, onChange, children, disabled }: { checked: boolean; onChange: (v: boolean) => void; children: ReactNode; disabled?: boolean }) {
  return (
    <label>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      {children}
    </label>
  );
}

export function Num({ value, onChange, min, max, width = 80, disabled }: { value: number; onChange: (v: number) => void; min?: number; max?: number; width?: number; disabled?: boolean }) {
  return (
    <input
      type="number"
      style={{ width }}
      value={Number.isFinite(value) ? value : 0}
      min={min}
      max={max}
      disabled={disabled}
      onChange={(e) => {
        let v = Number(e.target.value);
        if (!Number.isFinite(v)) v = 0;
        if (min != null) v = Math.max(min, v);
        if (max != null) v = Math.min(max, v);
        onChange(v);
      }}
    />
  );
}

export function Loading() {
  return <div style={{ padding: 20 }} className="muted">Loading…</div>;
}

/** A small dialog over the current window (Escape or a click outside closes it). */
export function Modal({ title, children, foot, onClose }: { title: string; children: ReactNode; foot: ReactNode; onClose: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.25)", display: "grid", placeItems: "center", zIndex: 80 }} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div style={{ background: "var(--bg)", borderRadius: 8, boxShadow: "var(--shadow)", width: 460, maxWidth: "92vw", overflow: "hidden", border: "1px solid var(--border-strong)" }} role="dialog" aria-label={title}>
        <div style={{ padding: "12px 16px", fontWeight: 600, borderBottom: "1px solid var(--border)" }}>{title}</div>
        <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>{children}</div>
        <div className="dlg-foot">{foot}</div>
      </div>
    </div>
  );
}
