// Typed bridge to the Rust side. Types mirror crates/swiftdm-core (serde field names).
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type JobState =
  | "QUEUED" | "STARTING" | "DOWNLOADING" | "POSTPROCESSING" | "FINALIZING" | "COMPLETED"
  | "PAUSING" | "PAUSED" | "RETRY_WAIT" | "CANCELLING" | "CANCELLED" | "FAILED";

export interface ErrorInfo {
  code: string;
  message: string;
  detail: string;
  retryable: boolean;
  http_status: number | null;
}

export interface Job {
  id: string;
  url: string;
  title: string;
  file_name: string | null;
  state: JobState;
  reason: string | null;
  kind: string | null;
  dest_dir: string;
  final_path: string | null;
  total: number | null;
  downloaded: number;
  error: ErrorInfo | null;
  format_label: string;
  source: string;
  category: string | null;
  queue_id: string | null;
  description: string | null;
  referer: string | null;
  final_url: string | null;
  resumable: boolean | null;
  speed_limit: number | null;
  priority: number;
  created_at: number;
  completed_at: number | null;
  last_try: number | null;
  attempts: number;
  version: number;
}

export interface SegmentProgress { idx: number; start: number; end: number; downloaded: number; active: boolean }

export interface Progress {
  job_id: string;
  downloaded: number;
  total: number | null;
  speed: number | null;
  eta: number | null;
  phase: string;
  segments: SegmentProgress[];
}

export interface Category { name: string; extensions: string[]; folder: string }
export interface SiteLogin { host: string; username: string; password_enc: string }
export interface ServerLimit { host: string; max_connections: number }

export interface Settings {
  version: number;
  download_dir: string;
  use_categories: boolean;
  categories: Category[];
  ask_save_location: boolean;
  show_download_info_dialog: boolean;
  show_progress_window: boolean;
  show_complete_dialog: boolean;
  max_concurrent: number;
  max_segments: number;
  collision_policy: string;
  auto_resume: boolean;
  retries: number;
  speed_limit: number;
  proxy_mode: "system" | "none" | "manual";
  proxy_url: string;
  user_agent: string;
  site_logins: SiteLogin[];
  clipboard_monitor: boolean;
  capture_extensions: string[];
  capture_min_size_kb: number;
  trust_extension: boolean;
  minimize_to_tray: boolean;
  start_with_windows: boolean;
  start_minimized: boolean;
  start_while_info: boolean;
  force_close_apps: boolean;
  confirm_delete: boolean;
  offer_exception_on_cancel: boolean;
  av_scan_enabled: boolean;
  av_scan_command: string;
  sound_on_complete: boolean;
  theme: "system" | "light" | "dark";
  api_port: number;
  server_limits: ServerLimit[];
  capture_skip_sites: string[];
  capture_skip_urls: string[];
  duplicate_policy: "ask" | "number" | "overwrite" | "resume";
  quota_mb: number;
  quota_hours: number;
}

export type AfterAction = "none" | "exit" | "sleep" | "hibernate" | "shutdown";

export interface Schedule { enabled: boolean; start_time: string; days: boolean[]; stop_time: string | null }

export interface Queue {
  id: string;
  name: string;
  max_concurrent: number;
  running: boolean;
  schedule: Schedule | null;
  after: AfterAction;
}

export interface QuotaStatus { used: number; limit: number; blocked: boolean }

export interface UrlInfo {
  file_name: string;
  size: number | null;
  content_type: string | null;
  resumable: boolean;
  final_url: string;
  is_web_page: boolean;
  category: string | null;
  folder: string;
}

export interface AppState {
  jobs: Job[];
  progress: Progress[];
  queues: Queue[];
  settings: Settings;
  quota: QuotaStatus;
  version: string;
}

export interface MediaOptions { preset: string; max_height?: number | null; format_id?: string | null }

export interface FormatInfo {
  id: string;
  ext: string;
  height: number | null;
  fps: number | null;
  vcodec: string | null;
  acodec: string | null;
  size: number | null;
  note: string;
}

export interface MediaInfo { title: string; extractor: string; duration: number | null; thumbnail: string | null; formats: FormatInfo[] }

export interface JobSpec {
  url: string;
  dest_dir: string;
  kind?: string;
  media?: MediaOptions | null;
  format_label?: string;
  filename?: string | null;
  source?: string;
  referer?: string | null;
  priority?: number;
  category?: string | null;
  queue_id?: string | null;
  description?: string | null;
  speed_limit?: number | null;
}

export type StartMode = "now" | "later";

export interface ApiStatus { port: number | null; paired: string[]; error: string | null }

export interface Components { ytdlp: string | null; ffmpeg: boolean; ffprobe: boolean; ready: boolean; ytdlp_version: string | null }
export interface UpdateInfo { version: string; notes: string | null }
export interface ExtensionSetup { mode: "store" | "manual"; folder: string | null; browser_found: boolean }

export interface ZipEntry { name: string; size: number; packed: number; is_dir: boolean }

export interface Overrides {
  dest_dir?: string | null;
  filename?: string | null;
  category?: string | null;
  description?: string | null;
  mode?: StartMode;
  queue_id?: string | null;
  quiet?: boolean;
}

export type Series =
  | { kind: "numbers"; from: number; to: number; width: number }
  | { kind: "letters"; from: string; to: string };

export type CoreEvent =
  | { type: "job_changed"; snapshot: Job }
  | { type: "job_progress"; progress: Progress }
  | { type: "job_removed"; job_id: string }
  | { type: "job_log"; job_id: string; level: string; message: string }
  | { type: "queue_changed"; queue_id: string; running: boolean }
  | { type: "queue_finished"; queue_id: string; action: string }
  | { type: "quota_exceeded"; used: number; limit: number; resume_in_secs: number }
  | { type: "quota_released" }
  // Emitted by the app shell, not the core:
  | { type: "settings_changed"; settings: Settings }
  | { type: "queues_changed"; queues: Queue[] }
  | { type: "quota"; quota: QuotaStatus }
  | { type: "extension_changed" }
  | { type: "components"; step: string; done: number; total: number | null }
  | { type: "update_available"; update: UpdateInfo }
  | { type: "update_progress"; done: number; total: number | null };

export type WindowKind =
  | "add" | "info" | "progress" | "complete" | "options" | "scheduler" | "batch"
  | "properties" | "about" | "countdown" | "links" | "pairing" | "browser" | "drop" | "login";

export const api = {
  state: () => invoke<AppState>("get_state"),
  submit: (spec: JobSpec, mode: StartMode) => invoke<string>("submit", { spec, mode }),
  submitMany: (specs: JobSpec[], mode: StartMode) => invoke<string[]>("submit_many", { specs, mode }),
  pause: (ids: string[]) => invoke<void>("pause", { ids }),
  retry: (ids: string[]) => invoke<void>("retry", { ids }),
  resume: (ids: string[]) => invoke<void>("resume", { ids }),
  cancel: (ids: string[]) => invoke<void>("cancel", { ids }),
  remove: (ids: string[], deleteFiles: boolean) => invoke<void>("remove", { ids, deleteFiles }),
  redownload: (ids: string[]) => invoke<string[]>("redownload", { ids }),
  pauseAll: () => invoke<void>("pause_all"),
  resumeAll: () => invoke<void>("resume_all"),
  startQueue: (id: string) => invoke<void>("start_queue", { id }),
  stopQueue: (id: string) => invoke<void>("stop_queue", { id }),
  saveQueue: (queue: Queue) => invoke<void>("save_queue", { queue }),
  deleteQueue: (id: string) => invoke<void>("delete_queue", { id }),
  moveToQueue: (ids: string[], queueId: string | null) => invoke<void>("move_to_queue", { ids, queueId }),
  setDescription: (id: string, description: string | null) => invoke<void>("set_description", { id, description }),
  relocate: (id: string, dir: string, name: string | null) => invoke<void>("relocate", { id, dir, name }),
  /** Drag a file out of the window into Explorer or another program. */
  dragFile: (path: string, image: string) => invoke<void>("plugin:drag|start_drag", { item: [path], image, options: { mode: "copy" }, onEvent: new Channel() }),
  hold: (id: string) => invoke<void>("hold", { id }),
  countCancel: (url: string) => invoke<number>("count_cancel", { url }),
  zipPreview: (url: string, total: number) => invoke<ZipEntry[]>("zip_preview", { url, total }),
  release: (id: string, show: boolean) => invoke<void>("release", { id, show }),
  setSpeedLimit: (id: string, limit: number | null) => invoke<void>("set_speed_limit", { id, limit }),
  setPriority: (id: string, priority: number) => invoke<void>("set_priority", { id, priority }),
  setJobAfter: (id: string, action: AfterAction) => invoke<void>("set_job_after", { id, action }),
  getJobAfter: (id: string) => invoke<AfterAction>("get_job_after", { id }),
  refreshUrl: (id: string, url: string) => invoke<void>("refresh_url", { id, url }),
  inspectUrl: (url: string) => invoke<UrlInfo>("inspect_url", { url }),
  isMediaUrl: (url: string) => invoke<boolean>("is_media_url", { url }),
  inspectMedia: (url: string) => invoke<MediaInfo>("inspect_media", { url }),
  findDuplicates: (url: string) => invoke<Job[]>("find_duplicates", { url }),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  expandBatch: (pattern: string, series: Series) => invoke<string[]>("expand_batch", { pattern, series }),
  extractUrls: (text: string) => invoke<string[]>("extract_urls", { text }),
  clipboardUrls: () => invoke<string[]>("clipboard_urls"),
  openFile: (id: string) => invoke<void>("open_file", { id }),
  openWith: (id: string) => invoke<void>("open_with", { id }),
  openFolder: (id: string) => invoke<void>("open_folder", { id }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  pickFolder: (initial: string | null) => invoke<string | null>("pick_folder", { initial }),
  pickFile: (initial: string | null) => invoke<string | null>("pick_file", { initial }),
  exportList: (ids: string[] | null) => invoke<number>("export_list", { ids }),
  importList: () => invoke<string[]>("import_list"),
  openWindow: (kind: WindowKind, params: Record<string, string> = {}) => invoke<void>("open_window", { kind, params }),
  closeSelf: () => invoke<void>("close_self"),
  closeWindow: (label: string) => invoke<void>("close_window", { label }),
  showMain: () => invoke<void>("show_main"),
  powerAction: (action: AfterAction) => invoke<void>("power_action", { action }),
  exitApp: () => invoke<void>("exit_app"),
  protectPassword: (password: string) => invoke<string>("protect_password", { password }),
  apiStatus: () => invoke<ApiStatus>("api_status"),
  revokeExtension: (id: string | null) => invoke<void>("revoke_extension", { id }),
  denyPairing: (pairingId: string) => invoke<void>("deny_pairing", { pairingId }),
  resolveRequest: (rid: string, overrides: Overrides) => invoke<string>("resolve_request", { rid, overrides }),
  rejectRequest: (rid: string) => invoke<void>("reject_request", { rid }),
  componentsStatus: () => invoke<Components>("components_status"),
  installComponents: () => invoke<Components>("install_components"),
  updateYtdlp: () => invoke<string>("update_ytdlp"),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  pendingUpdate: () => invoke<UpdateInfo | null>("pending_update"),
  extensionSetup: (browser: "chrome" | "edge") => invoke<ExtensionSetup>("extension_setup", { browser }),
};

export function onCore(handler: (e: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("core", (e) => handler(e.payload));
}

declare global {
  interface Window {
    /** Set by the app shell before the page loads: "w=kind&id=...". */
    __SDM_PARAMS__?: string;
  }
}

/** Parameters the shell passed to this window (kind and its arguments). */
export function windowParams(): URLSearchParams {
  return new URLSearchParams(window.__SDM_PARAMS__ ?? window.location.search);
}
