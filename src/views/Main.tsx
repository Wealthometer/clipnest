import { useEffect, useMemo, useRef, useState } from "react";
import { Clock3, FolderPlus, History, Pause, Pin, Play, Search, Settings as Cog, Sparkles, X, Folder as FolderIcon, Lock as LockIcon } from "lucide-react";
import { api, type Clip, type Folder, type Kind, type Query } from "../api.ts";
import { bytesText, plural, timeAgo } from "../lib.ts";
import { attempt, useInfo, useLoad, useTheme } from "../state.ts";
import { Highlighted, KINDS, KindChip, Kbd, Modal, Shortcut, toast, useNow } from "../ui/kit.tsx";
import { Lock } from "./Lock.tsx";
import { Preview, Thumb } from "./Preview.tsx";
import { SettingsView } from "./Settings.tsx";
import { SnippetsView } from "./Snippets.tsx";

export type Nav = { view: "history"; filter: "all" | "pinned" | "today" | { kind: Kind } | { folder: number } } | { view: "snippets" } | { view: "settings" };

const SINCE: { days: number | null; label: string }[] = [
  { days: null, label: "Any time" },
  { days: 1, label: "Today" },
  { days: 3, label: "3 days" },
  { days: 7, label: "This week" },
  { days: 30, label: "30 days" },
];

const FOLDER_COLORS = ["#FFE45C", "#73C2FF", "#FF94C8", "#6EF0A8", "#B89CFF", "#FFB25C"];

export function MainApp() {
  const info = useInfo();
  useTheme(info?.settings.theme);
  const [nav, setNav] = useState<Nav>({ view: "history", filter: "all" });
  const [stats] = useLoad(api.stats, ["clips-changed"]);
  const [folders] = useLoad(api.folders, ["clips-changed"]);
  const [newFolder, setNewFolder] = useState(false);
  const mac = info?.platform === "macos";

  if (!info) return <div className="boot" />;
  const isFilter = (f: unknown) => nav.view === "history" && JSON.stringify(nav.filter) === JSON.stringify(f);

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden>
            <i />
          </span>
          <span className="brand-name">
            Clip<b>Nest</b>
          </span>
        </div>
        <button
          className={`recording${info.settings.paused ? " paused" : ""}${info.vault === "locked" ? " locked" : ""}`}
          onClick={() => void attempt(api.setPaused(!info.settings.paused))}
          disabled={info.vault === "locked"}
          data-testid="recording"
          title={info.settings.paused ? "Start recording again" : "Pause recording"}
        >
          <span className="dot" />
          <span className="grow">{info.vault === "locked" ? "Locked · not recording" : info.settings.paused ? "Paused" : "Recording copies"}</span>
          {info.vault === "locked" ? <LockIcon size={14} /> : info.settings.paused ? <Play size={14} /> : <Pause size={14} />}
        </button>

        <nav aria-label="History">
          <NavItem icon={History} label="All copies" count={stats?.total} on={isFilter("all")} onClick={() => setNav({ view: "history", filter: "all" })} testId="nav-all" />
          <NavItem icon={Pin} label="Pinned" count={stats?.pinned} on={isFilter("pinned")} onClick={() => setNav({ view: "history", filter: "pinned" })} testId="nav-pinned" />
          <NavItem icon={Clock3} label="Today" count={stats?.today} on={isFilter("today")} onClick={() => setNav({ view: "history", filter: "today" })} testId="nav-today" />
        </nav>

        <div className="nav-label">Kinds</div>
        <nav aria-label="Kinds">
          {KINDS.map((k) => {
            const n = stats?.byKind.find(([kind]) => kind === k.kind)?.[1] ?? 0;
            return (
              <button key={k.kind} className={`nav-item${isFilter({ kind: k.kind }) ? " on" : ""}`} onClick={() => setNav({ view: "history", filter: { kind: k.kind } })} data-testid={`nav-kind-${k.kind}`}>
                <span className={`nav-dot k-${k.kind}`} />
                <span className="grow">{k.plural}</span>
                {n ? <span className="count">{n}</span> : null}
              </button>
            );
          })}
        </nav>

        <div className="nav-label">
          Folders
          <button className="icon-btn tiny" aria-label="New folder" onClick={() => setNewFolder(true)} data-testid="new-folder">
            <FolderPlus size={14} />
          </button>
        </div>
        <nav aria-label="Folders">
          {(folders ?? []).map((f) => (
            <button key={f.id} className={`nav-item${isFilter({ folder: f.id }) ? " on" : ""}`} onClick={() => setNav({ view: "history", filter: { folder: f.id } })} data-testid={`nav-folder-${f.name}`}>
              <FolderIcon size={15} style={{ color: f.color }} fill={f.color} fillOpacity={0.25} />
              <span className="grow">{f.name}</span>
              <span className="count">{f.count}</span>
            </button>
          ))}
          {folders && !folders.length ? <div className="nav-hint">Group clips you keep coming back to.</div> : null}
        </nav>

        <div className="nav-label">Yours</div>
        <nav>
          <NavItem icon={Sparkles} label="Snippets" on={nav.view === "snippets"} onClick={() => setNav({ view: "snippets" })} testId="nav-snippets" />
          <NavItem icon={Cog} label="Settings" on={nav.view === "settings"} onClick={() => setNav({ view: "settings" })} testId="nav-settings" />
        </nav>

        <div className="sidebar-foot">
          <div className="small">Quick paste from anywhere</div>
          <Shortcut accel={info.settings.shortcut} mac={mac} />
          {stats ? <div className="small faint">{plural(stats.total, "clip")} · {bytesText(stats.bytes)}</div> : null}
        </div>
      </aside>

      <main className="main">
        {info.vault === "locked" ? (
          <Lock />
        ) : nav.view === "snippets" ? (
          <SnippetsView mac={mac} />
        ) : nav.view === "settings" ? (
          <SettingsView info={info} />
        ) : (
          <HistoryView key={JSON.stringify(nav.filter)} filter={nav.filter} folders={folders ?? []} mac={mac} shortcut={info.settings.shortcut} />
        )}
      </main>

      {newFolder ? <FolderEditor onClose={() => setNewFolder(false)} /> : null}
    </div>
  );
}

function NavItem({ icon: I, label, count, on, onClick, testId }: { icon: typeof History; label: string; count?: number; on: boolean; onClick: () => void; testId: string }) {
  return (
    <button className={`nav-item${on ? " on" : ""}`} onClick={onClick} data-testid={testId} aria-current={on ? "page" : undefined}>
      <I size={16} />
      <span className="grow">{label}</span>
      {count ? <span className="count">{count}</span> : null}
    </button>
  );
}

export function FolderEditor({ folder, onClose }: { folder?: Folder; onClose: () => void }) {
  const [name, setName] = useState(folder?.name ?? "");
  const [color, setColor] = useState(folder?.color ?? FOLDER_COLORS[0]!);
  const save = async () => {
    const id = await attempt(api.saveFolder(folder?.id ?? null, name, color), folder ? "Folder renamed" : `Folder “${name.trim()}” made`);
    if (id) onClose();
  };
  return (
    <Modal title={folder ? "Rename folder" : "New folder"} onClose={onClose} testId="folder-editor">
      <div className="stack">
        <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void save()} placeholder="Work, Addresses, Code…" maxLength={60} data-testid="folder-name" aria-label="Folder name" />
        <div className="row">
          {FOLDER_COLORS.map((c) => (
            <button key={c} className={`swatch${c === color ? " on" : ""}`} style={{ background: c }} onClick={() => setColor(c)} aria-label={`Colour ${c}`} />
          ))}
        </div>
        <div className="row end">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={() => void save()} disabled={!name.trim()} data-testid="folder-save">
            {folder ? "Save" : "Make folder"}
          </button>
        </div>
      </div>
    </Modal>
  );
}

// ── History: search, list and preview ──────────────────────────────────────

function HistoryView({ filter, folders, mac, shortcut }: { filter: Extract<Nav, { view: "history" }>["filter"]; folders: Folder[]; mac: boolean; shortcut: string }) {
  const [text, setText] = useState("");
  const [since, setSince] = useState<number | null>(filter === "today" ? 1 : null);
  const [selected, setSelected] = useState<number | null>(null);
  const [limit, setLimit] = useState(200);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const now = useNow();

  const query: Query = useMemo(
    () => ({
      text,
      sinceDays: since,
      limit,
      kind: typeof filter === "object" && "kind" in filter ? filter.kind : null,
      folder: typeof filter === "object" && "folder" in filter ? filter.folder : null,
      pinned: filter === "pinned",
    }),
    [text, since, limit, filter],
  );
  const [clips] = useLoad(() => api.list(query), ["clips-changed", "state-changed"], [JSON.stringify(query)]);
  const items = useMemo(() => clips ?? [], [clips]);
  const current = items.find((c) => c.id === selected) ?? items[0] ?? null;
  const index = current ? items.indexOf(current) : -1;
  const title =
    filter === "all" ? "Everything you've copied" : filter === "pinned" ? "Pinned" : filter === "today" ? "Copied today" : "kind" in filter ? KINDS.find((k) => k.kind === filter.kind)!.plural : (folders.find((f) => f.id === filter.folder)?.name ?? "Folder");

  // Keyboard: arrows move, Enter copies, P pins, Delete removes, / searches, Ctrl+1–9 copies the nth.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector(".scrim")) return;
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if ((e.key === "/" && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f")) {
        e.preventDefault();
        search.current?.focus();
        search.current?.select();
        return;
      }
      if (e.key === "Escape" && typing) {
        if (text) setText("");
        else (e.target as HTMLElement).blur();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const next = items[Math.min(items.length - 1, Math.max(0, index + (e.key === "ArrowDown" ? 1 : -1)))];
        if (next) {
          setSelected(next.id);
          list.current?.querySelector(`[data-id="${next.id}"]`)?.scrollIntoView({ block: "nearest" });
        }
        return;
      }
      if ((e.ctrlKey || e.metaKey) && /^[1-9]$/.test(e.key)) {
        const c = items[Number(e.key) - 1];
        if (c) {
          e.preventDefault();
          void copy(c);
        }
        return;
      }
      if (!current) return;
      if (e.key === "Enter" && (!typing || e.target === search.current)) {
        e.preventDefault();
        void copy(current);
      } else if (!typing && e.key.toLowerCase() === "p" && !e.ctrlKey && !e.metaKey) {
        void attempt(api.pin([current.id], !current.pinned), current.pinned ? "Unpinned" : "Pinned");
      } else if (!typing && (e.key === "Delete" || e.key === "Backspace")) {
        const next = items[index + 1] ?? items[index - 1];
        setSelected(next?.id ?? null);
        void attempt(api.remove([current.id]), "Deleted");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="history">
      <section className="list-col">
        <header className="list-head">
          <div className="title-row">
            <h1 data-testid="history-title">{title}</h1>
            <span className="small faint">{clips ? plural(items.length, "clip") + (items.length >= limit ? "+" : "") : ""}</span>
          </div>
          <div className="search">
            <Search size={17} />
            <input
              ref={search}
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                setSelected(null);
              }}
              placeholder="Search everything you've copied…"
              aria-label="Search"
              data-testid="search"
              autoFocus
            />
            {text ? (
              <button className="icon-btn tiny" onClick={() => setText("")} aria-label="Clear search">
                <X size={14} />
              </button>
            ) : (
              <Kbd>/</Kbd>
            )}
          </div>
          <div className="since" role="group" aria-label="When">
            {SINCE.map((s) => (
              <button key={s.label} className={since === s.days ? "on" : ""} onClick={() => setSince(s.days)} data-testid={`since-${s.days ?? "any"}`}>
                {s.label}
              </button>
            ))}
          </div>
        </header>

        <div className="clips" ref={list} role="listbox" aria-label="Clips" data-testid="clips">
          {items.map((c, i) => (
            <ClipRow key={c.id} c={c} n={i} on={current?.id === c.id} now={now} onClick={() => setSelected(c.id)} onDouble={() => void copy(c)} folder={folders.find((f) => f.id === c.folderId)} />
          ))}
          {clips && !items.length ? <EmptyList searching={!!text} filter={filter} mac={mac} shortcut={shortcut} /> : null}
          {items.length >= limit ? (
            <button className="btn ghost more" onClick={() => setLimit((l) => l + 200)}>
              Show more
            </button>
          ) : null}
        </div>
      </section>
      <section className="preview-col">{current ? <Preview key={current.id} clip={current} folders={folders} mac={mac} onCopy={() => void copy(current)} /> : <div className="preview-empty">Copy something and it shows up here.</div>}</section>
    </div>
  );
}

async function copy(c: Clip) {
  if (c.locked) return;
  const ok = await attempt(api.copy(c.id, false));
  if (ok !== undefined) toast(c.kind === "image" ? "Picture copied" : "Copied");
}

function ClipRow({ c, n, on, now, onClick, onDouble, folder }: { c: Clip; n: number; on: boolean; now: number; onClick: () => void; onDouble: () => void; folder?: Folder }) {
  const fresh = now - c.createdAt < 4000;
  return (
    <div
      className={`clip${on ? " on" : ""}${fresh ? " fresh" : ""}`}
      role="option"
      aria-selected={on}
      tabIndex={-1}
      data-id={c.id}
      data-testid="clip"
      onClick={onClick}
      onDoubleClick={onDouble}
      style={{ animationDelay: `${Math.min(n, 12) * 18}ms` }}
    >
      <KindChip kind={c.kind} />
      <div className="clip-body">
        {c.kind === "image" ? (
          <div className="clip-image">
            <Thumb id={c.id} />
            <span>{c.preview}</span>
          </div>
        ) : c.kind === "color" ? (
          <div className="clip-text mono">
            <span className="color-dot" style={{ background: c.preview }} /> {c.preview}
          </div>
        ) : (
          <div className={`clip-text${c.kind === "code" || c.kind === "path" ? " mono" : ""}${c.sensitive ? " hidden-secret" : ""}`} data-testid="clip-text">
            {c.locked ? "Locked" : <Highlighted snippet={c.snippet} fallback={c.preview} />}
          </div>
        )}
        <div className="clip-meta">
          <span>{timeAgo(c.usedAt, now)}</span>
          {c.source ? <span>· {c.source.replace(/\.exe$/i, "")}</span> : null}
          {c.lines > 1 ? <span>· {c.lines} lines</span> : null}
          {c.uses > 0 ? <span>· used {c.uses}×</span> : null}
          {folder ? (
            <span className="folder-tag" style={{ color: folder.color }}>
              · {folder.name}
            </span>
          ) : null}
        </div>
      </div>
      <div className="clip-side">
        {c.pinned ? <Pin size={14} className="pinned" fill="currentColor" aria-label="Pinned" /> : null}
        {n < 9 ? <span className="nth">{n + 1}</span> : null}
      </div>
    </div>
  );
}

function EmptyList({ searching, filter, mac, shortcut }: { searching: boolean; filter: unknown; mac: boolean; shortcut: string }) {
  return (
    <div className="empty" data-testid="empty">
      <div className="empty-mark">
        <span className="hl big">{searching ? "Nothing matches" : filter === "pinned" ? "No pins yet" : "Nothing here yet"}</span>
      </div>
      <p className="muted">
        {searching
          ? "Try fewer words or a longer time range. Searches match the start of words, so “inv” finds “invoice”."
          : filter === "pinned"
            ? "Pin clips you keep coming back to (select one and press P). Pinned clips are never cleared."
            : "Copy something anywhere, it shows up here. ClipNest keeps text, links, code and pictures on this computer only."}
      </p>
      {!searching ? (
        <p className="small faint">
          Tip: press <Shortcut accel={shortcut} mac={mac} /> anywhere for quick paste.
        </p>
      ) : null}
    </div>
  );
}
