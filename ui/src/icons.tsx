// SwiftHarryDM's own icon set: 24x24 stroke drawings, coloured with currentColor.
import type { ReactElement } from "react";

const paths: Record<string, ReactElement> = {
  add: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7.5v9M7.5 12h9" />
    </>
  ),
  resume: <path d="M8 5.5v13l10.5-6.5z" />,
  stop: <rect x="6.5" y="6.5" width="11" height="11" rx="1.5" />,
  stopAll: (
    <>
      <rect x="4" y="7" width="9" height="9" rx="1.2" />
      <path d="M8 4h9.5A2.5 2.5 0 0 1 20 6.5V16" />
    </>
  ),
  delete: (
    <>
      <path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13" />
      <path d="M10.5 11v5.5M13.5 11v5.5" />
    </>
  ),
  deleteDone: (
    <>
      <path d="M4.5 7h11M8 7V4.5h4V7M6 7l1 12.5h6.5" />
      <path d="M14.5 16.5l2.2 2.2 4-4.4" />
    </>
  ),
  options: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8" />
    </>
  ),
  scheduler: (
    <>
      <circle cx="12" cy="13" r="8" />
      <path d="M12 8.5V13l3 2M9 2.5h6" />
    </>
  ),
  startQueue: (
    <>
      <path d="M4 6h8M4 11h8M4 16h5" />
      <path d="M14 10.5v8.5l6.5-4.25z" />
    </>
  ),
  stopQueue: (
    <>
      <path d="M4 6h8M4 11h8M4 16h5" />
      <rect x="13.5" y="11.5" width="7" height="7" rx="1" />
    </>
  ),
  batch: (
    <>
      <rect x="7" y="3.5" width="12" height="13" rx="1.5" />
      <path d="M4.5 7v11.5A1.5 1.5 0 0 0 6 20h10" />
      <path d="M10 8h6M10 11h6M10 14h3.5" />
    </>
  ),
  folder: <path d="M3.5 6.5A1.5 1.5 0 0 1 5 5h4.5l2 2.5H19a1.5 1.5 0 0 1 1.5 1.5v8.5A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z" />,
  all: (
    <>
      <path d="M12 3.5v11M7.5 10l4.5 4.5 4.5-4.5" />
      <path d="M4.5 16v2.5A1.5 1.5 0 0 0 6 20h12a1.5 1.5 0 0 0 1.5-1.5V16" />
    </>
  ),
  compressed: (
    <>
      <rect x="4.5" y="3.5" width="15" height="17" rx="1.5" />
      <path d="M12 3.5v2M12 7v2M12 10.5v2" />
      <rect x="10" y="14" width="4" height="3.5" rx="0.8" />
    </>
  ),
  documents: (
    <>
      <path d="M6 3.5h8l4.5 4.5v12.5H6z" />
      <path d="M14 3.5V8h4.5M9 12h6M9 15h6M9 18h3.5" />
    </>
  ),
  music: (
    <>
      <path d="M9.5 17.5V5.5l10-2v12" />
      <circle cx="7" cy="17.5" r="2.5" />
      <circle cx="17" cy="15.5" r="2.5" />
    </>
  ),
  programs: (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="1.5" />
      <path d="M3.5 8.5h17M7.5 12.5l2.5 2-2.5 2M12 16.5h4" />
    </>
  ),
  video: (
    <>
      <rect x="3.5" y="5.5" width="13" height="13" rx="1.5" />
      <path d="M16.5 10l4-2.5v9l-4-2.5" />
    </>
  ),
  file: (
    <>
      <path d="M6 3.5h8l4.5 4.5v12.5H6z" />
      <path d="M14 3.5V8h4.5" />
    </>
  ),
  unfinished: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 3.5a8.5 8.5 0 0 1 0 17z" fill="currentColor" stroke="none" />
    </>
  ),
  finished: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M8 12.2l2.8 2.8L16.2 9.5" />
    </>
  ),
  queue: (
    <>
      <path d="M8.5 6.5h11M8.5 12h11M8.5 17.5h11" />
      <circle cx="4.75" cy="6.5" r="1" fill="currentColor" />
      <circle cx="4.75" cy="12" r="1" fill="currentColor" />
      <circle cx="4.75" cy="17.5" r="1" fill="currentColor" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6" />
      <path d="M15 15l5 5" />
    </>
  ),
  close: <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />,
  up: <path d="M6.5 14.5L12 9l5.5 5.5" />,
  down: <path d="M6.5 9.5L12 15l5.5-5.5" />,
  speed: (
    <>
      <path d="M4 17a8 8 0 1 1 16 0" />
      <path d="M12 17l4-5.5" />
    </>
  ),
  chevron: <path d="M9.5 6.5L15 12l-5.5 5.5" />,
  link: (
    <>
      <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
      <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
    </>
  ),
  warning: (
    <>
      <path d="M12 4l9 16H3z" />
      <path d="M12 10v4.5M12 17.2v.3" />
    </>
  ),
};

export function Icon({ name, size = 18, className }: { name: string; size?: number; className?: string }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {paths[name] ?? paths.file}
    </svg>
  );
}

const CATEGORY_ICON: Record<string, string> = {
  Compressed: "compressed",
  Documents: "documents",
  Music: "music",
  Programs: "programs",
  Video: "video",
};

export function categoryIcon(name: string | null | undefined): string {
  return (name && CATEGORY_ICON[name]) || "folder";
}
