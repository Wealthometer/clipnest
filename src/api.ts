import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Everything the window can ask of ClipNest's Rust side, typed. Field names match the Rust structs. */

export type Kind = "text" | "link" | "email" | "color" | "number" | "code" | "path" | "image";

export type Clip = {
  id: number;
  kind: Kind;
  preview: string;
  snippet: string | null;
  createdAt: number;
  usedAt: number;
  uses: number;
  pinned: boolean;
  folderId: number | null;
  sensitive: boolean;
  source: string | null;
  width: number | null;
  height: number | null;
  bytes: number;
  lines: number;
  locked: boolean;
};

export type ClipDetail = Clip & { text: string | null; image: string | null };

export type Query = { text?: string; kind?: Kind | null; folder?: number | null; pinned?: boolean; sinceDays?: number | null; limit?: number; offset?: number };

export type Folder = { id: number; name: string; color: string; count: number };
export type Snippet = { id: number; name: string; keyword: string; body: string; uses: number; usedAt: number };

export type Settings = {
  shortcut: string;
  autoPaste: boolean;
  keepDays: number;
  maxItems: number;
  captureImages: boolean;
  imageLimitMb: number;
  secretPolicy: "skip" | "mask";
  excludedApps: string[];
  paused: boolean;
  startHidden: boolean;
  launchAtLogin: boolean;
  theme: "system" | "dark" | "light";
};

export type VaultState = "off" | "locked" | "unlocked";
export type AppInfo = { settings: Settings; vault: VaultState; platform: "windows" | "macos" | "linux"; version: string; dataDir: string };
export type Stats = { total: number; today: number; pinned: number; byKind: [Kind, number][]; bytes: number };

export const api = {
  list: (query: Query) => invoke<Clip[]>("list_clips", { query }),
  detail: (id: number, full = false) => invoke<ClipDetail>("clip_detail", { id, full }),
  copy: (id: number, paste = false) => invoke<void>("copy_clip", { id, paste }),
  pin: (ids: number[], pinned: boolean) => invoke<void>("pin_clips", { ids, pinned }),
  move: (ids: number[], folder: number | null) => invoke<void>("move_clips", { ids, folder }),
  remove: (ids: number[]) => invoke<number>("delete_clips", { ids }),
  clear: (everything: boolean) => invoke<number>("clear_history", { everything }),
  folders: () => invoke<Folder[]>("folders"),
  saveFolder: (id: number | null, name: string, color: string) => invoke<number>("save_folder", { id, name, color }),
  deleteFolder: (id: number) => invoke<void>("delete_folder", { id }),
  snippets: () => invoke<Snippet[]>("snippets"),
  saveSnippet: (s: { id: number | null; name: string; keyword: string; body: string }) => invoke<number>("save_snippet", s),
  deleteSnippet: (id: number) => invoke<void>("delete_snippet", { id }),
  useSnippet: (id: number, paste = false) => invoke<string>("use_snippet", { id, paste }),
  info: () => invoke<AppInfo>("app_info"),
  saveSettings: (settings: Settings) => invoke<AppInfo>("save_settings", { settings }),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  stats: () => invoke<Stats>("stats"),
  vaultEnable: (passphrase: string) => invoke<void>("vault_enable", { passphrase }),
  vaultUnlock: (passphrase: string) => invoke<void>("vault_unlock", { passphrase }),
  vaultLock: () => invoke<void>("vault_lock"),
  vaultChange: (old: string, next: string) => invoke<void>("vault_change", { old, new: next }),
  vaultDisable: (passphrase: string) => invoke<void>("vault_disable", { passphrase }),
  exportHistory: () => invoke<string>("export_history"),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  openMain: () => invoke<void>("open_main"),
  hideQuick: () => invoke<void>("hide_quick"),
};

export type AppEvent = "clips-changed" | "snippets-changed" | "state-changed" | "quick-shown";

export function on(event: AppEvent, cb: () => void): () => void {
  let un: UnlistenFn | null = null;
  let dead = false;
  void listen(event, () => cb()).then((u) => {
    if (dead) u();
    else un = u;
  });
  return () => {
    dead = true;
    un?.();
  };
}

/** Rust errors arrive as plain strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
