import { useEffect, useRef, useState } from "react";
import { Copy, Plus, Sparkles, Trash2 } from "lucide-react";
import { api, type Snippet } from "../api.ts";
import { timeAgo } from "../lib.ts";
import { attempt, useLoad } from "../state.ts";
import { Kbd, toast, useNow } from "../ui/kit.tsx";

const PLACEHOLDERS: { token: string; label: string }[] = [
  { token: "{date}", label: "Today's date" },
  { token: "{time}", label: "Time" },
  { token: "{weekday}", label: "Weekday" },
  { token: "{clipboard}", label: "What's on the clipboard" },
];

type Draft = { id: number | null; name: string; keyword: string; body: string };
const toDraft = (s: Snippet): Draft => ({ id: s.id, name: s.name, keyword: s.keyword, body: s.body });
const EMPTY: Draft = { id: null, name: "", keyword: "", body: "" };

export function SnippetsView({ mac }: { mac: boolean }) {
  const [snippets] = useLoad(api.snippets, ["snippets-changed", "state-changed"]);
  const [picked, setDraft] = useState<Draft | null>(null);
  const [filter, setFilter] = useState("");
  const body = useRef<HTMLTextAreaElement>(null);
  const list = (snippets ?? []).filter((s) => `${s.name} ${s.keyword} ${s.body}`.toLowerCase().includes(filter.toLowerCase()));
  const open = (s: Snippet) => setDraft(toDraft(s));
  // Until something is picked, the editor shows the first snippet.
  const draft = picked ?? (snippets?.[0] ? toDraft(snippets[0]) : null);
  const now = useNow();

  const save = async () => {
    if (!draft) return;
    const id = await attempt(api.saveSnippet(draft), draft.id ? "Snippet saved" : "Snippet added");
    if (id) setDraft({ ...draft, id });
  };
  const insert = (token: string) => {
    if (!draft) return;
    const el = body.current;
    const at = el?.selectionStart ?? draft.body.length;
    const next = draft.body.slice(0, at) + token + draft.body.slice(el?.selectionEnd ?? at);
    setDraft({ ...draft, body: next });
    requestAnimationFrame(() => {
      el?.focus();
      el?.setSelectionRange(at + token.length, at + token.length);
    });
  };

  return (
    <div className="snippets">
      <section className="list-col">
        <header className="list-head">
          <div className="title-row">
            <h1>Snippets</h1>
            <button className="btn primary small" onClick={() => setDraft({ ...EMPTY })} data-testid="new-snippet">
              <Plus size={14} /> New
            </button>
          </div>
          <p className="muted small">Text you paste again and again: addresses, replies, signatures. They never expire, and quick paste finds them by name or keyword.</p>
          <div className="search">
            <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Find a snippet…" aria-label="Find a snippet" />
          </div>
        </header>
        <div className="clips">
          {list.map((s) => (
            <div key={s.id} className={`clip${draft?.id === s.id ? " on" : ""}`} onClick={() => open(s)} data-testid={`snippet-${s.name}`}>
              <span className="kind-chip k-snippet">
                <Sparkles size={15} />
              </span>
              <div className="clip-body">
                <div className="clip-text">
                  <b>{s.name}</b>
                  {s.keyword ? <span className="keyword">{s.keyword}</span> : null}
                </div>
                <div className="clip-meta">
                  <span>{s.body.split("\n")[0]!.slice(0, 60)}</span>
                  {s.uses ? <span>· used {s.uses}× · {timeAgo(s.usedAt, now)}</span> : null}
                </div>
              </div>
            </div>
          ))}
          {snippets && !snippets.length ? (
            <div className="empty">
              <span className="hl big">No snippets yet</span>
              <p className="muted">Make one here, or save any clip as a snippet from its preview.</p>
            </div>
          ) : null}
        </div>
      </section>
      <section className="preview-col">
        {draft ? (
          <div className="preview editor" data-testid="snippet-editor">
            <label className="field">
              <span>Name</span>
              <input className="input" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="Office address" data-testid="snippet-name-input" />
            </label>
            <label className="field">
              <span>Keyword (optional)</span>
              <input className="input mono" value={draft.keyword} onChange={(e) => setDraft({ ...draft, keyword: e.target.value })} placeholder=";addr" data-testid="snippet-keyword" />
            </label>
            <label className="field grow">
              <span>Text</span>
              <textarea ref={body} className="input mono" value={draft.body} onChange={(e) => setDraft({ ...draft, body: e.target.value })} placeholder={"Hi {clipboard},\n\nThanks for…"} data-testid="snippet-body" />
            </label>
            <div className="placeholders">
              {PLACEHOLDERS.map((p) => (
                <button key={p.token} className="chip" onClick={() => insert(p.token)} title={p.label}>
                  {p.token}
                </button>
              ))}
            </div>
            <div className="row">
              <button className="btn primary" onClick={() => void save()} disabled={!draft.name.trim() || !draft.body} data-testid="snippet-save-btn">
                Save <Kbd>{mac ? "⌘" : "Ctrl"} S</Kbd>
              </button>
              {draft.id ? (
                <>
                  <button
                    className="btn"
                    onClick={async () => {
                      const t = await attempt(api.useSnippet(draft.id!, false));
                      if (t !== undefined) toast("Copied, with placeholders filled in");
                    }}
                    data-testid="snippet-copy"
                  >
                    <Copy size={15} /> Copy
                  </button>
                  <span className="grow" />
                  <button
                    className="btn icon danger"
                    aria-label="Delete snippet"
                    onClick={async () => {
                      await attempt(api.deleteSnippet(draft.id!), "Snippet deleted");
                      setDraft(null);
                    }}
                  >
                    <Trash2 size={15} />
                  </button>
                </>
              ) : null}
            </div>
            <SaveShortcut onSave={() => void save()} />
          </div>
        ) : (
          <div className="preview-empty">Pick a snippet, or make a new one.</div>
        )}
      </section>
    </div>
  );
}

function SaveShortcut({ onSave }: { onSave: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        onSave();
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onSave]);
  return null;
}
