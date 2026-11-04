import { useEffect, useMemo, useRef, useState } from "react";
import { Search, Sparkles } from "lucide-react";
import { api, on, type Clip, type Snippet } from "../api.ts";
import { timeAgo } from "../lib.ts";
import { attempt, useInfo, useLoad, useTheme } from "../state.ts";
import { Highlighted, Kbd, KindChip, useNow } from "../ui/kit.tsx";
import { Thumb } from "./Preview.tsx";

type Tab = "recent" | "pinned" | "snippets";
type Item = { type: "clip"; clip: Clip } | { type: "snippet"; snippet: Snippet };

/**
 * The popup behind the global shortcut: type to search, arrows to choose, Enter to paste into the
 * app you came from. Tab switches between recent clips, pinned ones and snippets.
 */
export function QuickPaste() {
  const info = useInfo();
  useTheme(info?.settings.theme);
  const [text, setText] = useState("");
  const [tab, setTab] = useState<Tab>("recent");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const mac = info?.platform === "macos";
  const now = useNow();

  const [clips] = useLoad(() => api.list({ text, pinned: tab === "pinned", limit: 60 }), ["clips-changed", "state-changed"], [text, tab]);
  const [snippets] = useLoad(api.snippets, ["snippets-changed", "state-changed"]);

  const items: Item[] = useMemo(() => {
    if (tab === "snippets") {
      const q = text.toLowerCase();
      return (snippets ?? []).filter((s) => !q || `${s.keyword} ${s.name} ${s.body}`.toLowerCase().includes(q)).map((snippet) => ({ type: "snippet" as const, snippet }));
    }
    const list: Item[] = (clips ?? []).filter((c) => !c.locked).map((clip) => ({ type: "clip" as const, clip }));
    // A snippet whose keyword is typed exactly comes first ("addr" → the address snippet).
    const exact = text.trim() ? (snippets ?? []).filter((s) => s.keyword && s.keyword.toLowerCase() === text.trim().toLowerCase()) : [];
    return [...exact.map((snippet) => ({ type: "snippet" as const, snippet })), ...list];
  }, [tab, text, clips, snippets]);

  const cur = items[Math.min(index, items.length - 1)];

  // Fresh start every time the window opens.
  useEffect(
    () =>
      on("quick-shown", () => {
        setText("");
        setIndex(0);
        setTab("recent");
        input.current?.focus();
      }),
    [],
  );

  const choose = async (item: Item | undefined, paste: boolean) => {
    if (!item) return;
    const doPaste = paste && (info?.settings.autoPaste ?? true);
    if (item.type === "clip") await attempt(api.copy(item.clip.id, doPaste));
    else await attempt(api.useSnippet(item.snippet.id, doPaste));
    if (!doPaste) void api.hideQuick();
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (text) setText("");
      else void api.hideQuick();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = Math.min(items.length - 1, Math.max(0, index + (e.key === "ArrowDown" ? 1 : -1)));
      setIndex(n);
      listRef.current?.children[n]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      void choose(cur, !e.shiftKey);
    } else if (e.key === "Tab") {
      e.preventDefault();
      const order: Tab[] = ["recent", "pinned", "snippets"];
      setTab(order[(order.indexOf(tab) + (e.shiftKey ? 2 : 1)) % 3]!);
      setIndex(0);
    } else if ((e.ctrlKey || e.metaKey || e.altKey) && /^[1-9]$/.test(e.key)) {
      e.preventDefault();
      void choose(items[Number(e.key) - 1], true);
    }
  };

  return (
    <div className="quick" onKeyDown={onKey} data-testid="quick">
      <div className="quick-search">
        <Search size={18} />
        <input
          ref={input}
          autoFocus
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setIndex(0);
          }}
          placeholder={tab === "snippets" ? "Find a snippet…" : "Search what you've copied…"}
          aria-label="Search"
          data-testid="quick-search"
        />
        <div className="quick-tabs" role="tablist">
          {(["recent", "pinned", "snippets"] as Tab[]).map((t) => (
            <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? "on" : ""} onClick={() => setTab(t)} tabIndex={-1}>
              {t === "recent" ? "Recent" : t === "pinned" ? "Pinned" : "Snippets"}
            </button>
          ))}
        </div>
      </div>
      <div className="quick-body">
        <div className="quick-list" ref={listRef} role="listbox">
          {items.map((it, i) => (
            <div
              key={it.type === "clip" ? `c${it.clip.id}` : `s${it.snippet.id}`}
              className={`quick-item${i === index ? " on" : ""}`}
              role="option"
              aria-selected={i === index}
              onMouseMove={() => setIndex(i)}
              onClick={() => void choose(it, true)}
              data-testid="quick-item"
            >
              {it.type === "clip" ? <KindChip kind={it.clip.kind} size={26} /> : <span className="kind-chip k-snippet" style={{ width: 26, height: 26 }}><Sparkles size={13} /></span>}
              <span className={`quick-text${it.type === "clip" && it.clip.kind === "code" ? " mono" : ""}`}>
                {it.type === "clip" ? it.clip.kind === "image" ? it.clip.preview : <Highlighted snippet={it.clip.sensitive ? null : it.clip.snippet} fallback={it.clip.preview} /> : (
                  <>
                    <b>{it.snippet.name}</b> {it.snippet.keyword ? <span className="keyword">{it.snippet.keyword}</span> : null}
                  </>
                )}
              </span>
              {i < 9 ? <span className="nth">{i + 1}</span> : null}
            </div>
          ))}
          {!items.length ? <div className="quick-empty">{text ? "Nothing matches" : tab === "pinned" ? "Nothing pinned yet" : tab === "snippets" ? "No snippets yet" : "Copy something first"}</div> : null}
        </div>
        <div className="quick-preview">
          {cur?.type === "clip" ? (
            cur.clip.kind === "image" ? (
              <Thumb id={cur.clip.id} />
            ) : (
              <>
                <pre className={cur.clip.kind === "code" ? "mono" : ""}>{cur.clip.sensitive ? "•••••••• (hidden)" : cur.clip.preview}</pre>
                <div className="small faint">
                  {timeAgo(cur.clip.usedAt, now)}
                  {cur.clip.source ? ` · ${cur.clip.source.replace(/\.exe$/i, "")}` : ""}
                  {cur.clip.lines > 1 ? ` · ${cur.clip.lines} lines` : ""}
                </div>
              </>
            )
          ) : cur?.type === "snippet" ? (
            <pre className="mono">{cur.snippet.body}</pre>
          ) : null}
        </div>
      </div>
      <div className="quick-foot">
        <span>
          <Kbd>↵</Kbd> {info?.settings.autoPaste === false ? "copy" : "paste"}
        </span>
        <span>
          <Kbd>⇧</Kbd>
          <Kbd>↵</Kbd> copy only
        </span>
        <span>
          <Kbd>{mac ? "⌘" : "Ctrl"}</Kbd>
          <Kbd>1–9</Kbd> pick
        </span>
        <span>
          <Kbd>Tab</Kbd> recent · pinned · snippets
        </span>
        <span className="grow" />
        <button className="link" onClick={() => void api.openMain()}>
          Open ClipNest
        </button>
      </div>
    </div>
  );
}
