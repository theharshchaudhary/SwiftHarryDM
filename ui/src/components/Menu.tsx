import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../icons";

export type MenuEntry =
  | "sep"
  | {
      label: string;
      hint?: string;
      onClick?: () => void;
      disabled?: boolean;
      checked?: boolean;
      submenu?: MenuEntry[];
    };

function useOutside(ref: React.RefObject<HTMLElement>, onOutside: () => void, active: boolean) {
  useEffect(() => {
    if (!active) return;
    const down = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onOutside();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onOutside();
    const blur = () => onOutside();
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key);
      window.removeEventListener("blur", blur);
    };
  }, [active, onOutside, ref]);
}

export function MenuList({ items, onDone, style }: { items: MenuEntry[]; onDone: () => void; style?: React.CSSProperties }) {
  const [sub, setSub] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    if (r.right > window.innerWidth) el.style.left = `${Math.max(0, parseFloat(el.style.left || "0") - (r.right - window.innerWidth) - 4)}px`;
    if (r.bottom > window.innerHeight) el.style.top = `${Math.max(0, parseFloat(el.style.top || "0") - (r.bottom - window.innerHeight) - 4)}px`;
  });
  return (
    <div className="menu" style={style} ref={ref} role="menu">
      {items.map((it, i) =>
        it === "sep" ? (
          <div className="sep" key={i} />
        ) : (
          <div key={i} style={{ position: "relative" }} onMouseEnter={() => setSub(it.submenu ? i : null)}>
            <button
              className={`item${sub === i ? " open" : ""}`}
              disabled={it.disabled}
              role="menuitem"
              onClick={() => {
                if (it.submenu) {
                  setSub(i);
                  return;
                }
                onDone();
                it.onClick?.();
              }}
            >
              <span className="check">{it.checked ? "✓" : ""}</span>
              <span className="label">{it.label}</span>
              {it.hint && <span className="hint">{it.hint}</span>}
              {it.submenu && <Icon name="chevron" size={12} />}
            </button>
            {it.submenu && sub === i && !it.disabled && <MenuList items={it.submenu} onDone={onDone} style={{}} />}
          </div>
        ),
      )}
    </div>
  );
}

export interface MenuRoot {
  label: string;
  items: () => MenuEntry[];
}

export function MenuBar({ roots }: { roots: MenuRoot[] }) {
  const [open, setOpen] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useOutside(ref, () => setOpen(null), open !== null);
  return (
    <div className="menubar" ref={ref}>
      {roots.map((r, i) => (
        <div key={r.label} className={`menu-root${open === i ? " open" : ""}`} onMouseEnter={() => open !== null && setOpen(i)}>
          <button onMouseDown={() => setOpen(open === i ? null : i)}>{r.label}</button>
          {open === i && <MenuList items={r.items()} onDone={() => setOpen(null)} style={{ top: "100%", left: 0 }} />}
        </div>
      ))}
    </div>
  );
}

export function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: MenuEntry[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useOutside(ref, onClose, true);
  return (
    <div ref={ref} style={{ position: "fixed", left: 0, top: 0, zIndex: 60 }}>
      <MenuList items={items} onDone={onClose} style={{ left: x, top: y }} />
    </div>
  );
}
