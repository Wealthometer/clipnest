import { useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Code2, FileText, FolderOpen, Hash, Image as ImageIcon, Link2, Mail, Palette, Type, X } from "lucide-react";
import type { Kind } from "../api.ts";
import { highlightParts, shortcutKeys } from "../lib.ts";

export const KINDS: { kind: Kind; label: string; plural: string; icon: typeof Type }[] = [
  { kind: "link", label: "Link", plural: "Links", icon: Link2 },
  { kind: "image", label: "Picture", plural: "Pictures", icon: ImageIcon },
  { kind: "code", label: "Code", plural: "Code", icon: Code2 },
  { kind: "color", label: "Colour", plural: "Colours", icon: Palette },
  { kind: "email", label: "Email", plural: "Emails", icon: Mail },
  { kind: "number", label: "Number", plural: "Numbers", icon: Hash },
  { kind: "path", label: "File path", plural: "File paths", icon: FolderOpen },
  { kind: "text", label: "Text", plural: "Text", icon: FileText },
];

export function kindInfo(kind: Kind) {
  return KINDS.find((k) => k.kind === kind) ?? KINDS[KINDS.length - 1]!;
}

/** The coloured marker tab for a kind of clip. */
export function KindChip({ kind, size = 30 }: { kind: Kind; size?: number }) {
  const I = kindInfo(kind).icon;
  return (
    <span className={`kind-chip k-${kind}`} style={{ width: size, height: size }} aria-hidden>
      <I size={size * 0.52} strokeWidth={2.2} />
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function Shortcut({ accel, mac }: { accel: string; mac: boolean }) {
  return (
    <span className="shortcut">
      {shortcutKeys(accel, mac).map((k, i) => (
        <Kbd key={i}>{k}</Kbd>
      ))}
    </span>
  );
}

/** Text with search matches drawn as highlighter strokes. */
export function Highlighted({ snippet, fallback }: { snippet: string | null; fallback: string }) {
  if (!snippet) return <>{fallback}</>;
  return (
    <>
      {highlightParts(snippet).map((p, i) =>
        p.hit ? (
          <mark key={i} className="hl">
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}

export function Toggle({ label, detail, value, onChange, testId }: { label: string; detail?: ReactNode; value: boolean; onChange: (v: boolean) => void; testId?: string }) {
  return (
    <label className="toggle" data-testid={testId}>
      <span className="grow">
        <span className="toggle-label">{label}</span>
        {detail ? <span className="toggle-detail">{detail}</span> : null}
      </span>
      <input type="checkbox" role="switch" checked={value} onChange={(e) => onChange(e.target.checked)} />
      <span className="switch" aria-hidden />
    </label>
  );
}

export function Modal({ title, onClose, children, testId }: { title: string; onClose: () => void; children: ReactNode; testId?: string }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-label={title} data-testid={testId}>
        <div className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

// ── Toasts ─────────────────────────────────────────────────────────────────

type Toast = { id: number; text: string; tone: "ok" | "error" };
let toasts: Toast[] = [];
let seq = 0;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

export function toast(text: string, tone: Toast["tone"] = "ok") {
  const id = ++seq;
  toasts = [...toasts.slice(-2), { id, text, tone }];
  emit();
  setTimeout(
    () => {
      toasts = toasts.filter((t) => t.id !== id);
      emit();
    },
    tone === "error" ? 5000 : 2200,
  );
}

export function Toasts() {
  const list = useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => toasts,
  );
  return (
    <div className="toasts" aria-live="polite">
      {list.map((t) => (
        <div key={t.id} className={`toast ${t.tone}`} data-testid="toast">
          {t.tone === "error" ? <AlertTriangle size={16} /> : <CheckCircle2 size={16} />}
          {t.text}
        </div>
      ))}
    </div>
  );
}

/** Ticks every `ms` so "3 min ago" stays true. */
export function useNow(ms = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}
