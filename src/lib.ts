/** Small pure helpers for the interface (tested in test/lib.test.ts). */

export type Part = { text: string; hit: boolean };

/** Splits a search snippet ("…the \u0001Kofi\u0002 invoice") into plain and highlighted parts. */
export function highlightParts(snippet: string): Part[] {
  const out: Part[] = [];
  let hit = false;
  let buf = "";
  for (const ch of snippet) {
    if (ch === "\u0001" || ch === "\u0002") {
      if (buf) out.push({ text: buf, hit });
      buf = "";
      hit = ch === "\u0001";
    } else buf += ch;
  }
  if (buf) out.push({ text: buf, hit });
  return out;
}

export function timeAgo(t: number, now: number): string {
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86_400) return `${Math.round(s / 3600)} h ago`;
  const d = new Date(t);
  const today = new Date(now);
  const y = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return "yesterday";
  if (s < 7 * 86_400) return d.toLocaleDateString(undefined, { weekday: "short" });
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(d.getFullYear() !== today.getFullYear() ? { year: "numeric" } : {}) });
}

export function bytesText(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(1)} GB`;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

// ── Colours: a copied colour shown in every common notation ────────────────

export type Rgba = { r: number; g: number; b: number; a: number };

export function parseColor(input: string): Rgba | null {
  const s = input.trim().toLowerCase();
  let m = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(s);
  if (m) {
    let h = m[1]!;
    if (h.length <= 4) h = [...h].map((c) => c + c).join("");
    const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
    return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? Math.round((n(6) / 255) * 100) / 100 : 1 };
  }
  m = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(s);
  if (m) {
    const [r, g, b] = [m[1], m[2], m[3]].map((v) => Math.min(255, Number(v)));
    return { r: r!, g: g!, b: b!, a: m[4] === undefined ? 1 : Math.min(1, Number(m[4])) };
  }
  m = /^hsla?\(\s*(\d{1,3})(?:deg)?\s*,\s*(\d{1,3})%\s*,\s*(\d{1,3})%\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(s);
  if (m) {
    const h = Number(m[1]) % 360;
    const sat = Math.min(100, Number(m[2])) / 100;
    const l = Math.min(100, Number(m[3])) / 100;
    const k = (n: number) => (n + h / 30) % 12;
    const a = sat * Math.min(l, 1 - l);
    const f = (n: number) => Math.round(255 * (l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)))));
    return { r: f(0), g: f(8), b: f(4), a: m[4] === undefined ? 1 : Math.min(1, Number(m[4])) };
  }
  return null;
}

export function colorFormats(c: Rgba): { label: string; value: string }[] {
  const hex = `#${[c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("")}${c.a < 1 ? Math.round(c.a * 255).toString(16).padStart(2, "0") : ""}`.toUpperCase();
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h *= 60;
  }
  const alpha = c.a < 1 ? `, ${c.a}` : "";
  return [
    { label: "HEX", value: hex },
    { label: "RGB", value: `rgb${c.a < 1 ? "a" : ""}(${c.r}, ${c.g}, ${c.b}${alpha})` },
    { label: "HSL", value: `hsl${c.a < 1 ? "a" : ""}(${Math.round(h)}, ${Math.round(s * 100)}%, ${Math.round(l * 100)}%${alpha})` },
  ];
}

/** Black or white text on a colour. */
export function inkOn(c: Rgba): string {
  const lum = (0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b) / 255;
  return lum > 0.6 ? "#18181B" : "#FFFFFF";
}

// ── Shortcuts ──────────────────────────────────────────────────────────────

/** Turns a key press into an accelerator string ("Ctrl+Alt+V"), or null if it's only modifiers. */
export function acceleratorFrom(e: { key: string; code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }, mac: boolean): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push(mac ? "Option" : "Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push(mac ? "Cmd" : "Super");
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (/^F\d{1,2}$/.test(e.code)) key = e.code;
  else if (e.code === "Space") key = "Space";
  else if (e.code === "Backquote") key = "`";
  else if (e.code === "Period") key = ".";
  else if (e.code === "Comma") key = ",";
  if (!key || !mods.length) return null;
  // Shift alone isn't enough: it would steal ordinary typing.
  if (mods.length === 1 && mods[0] === "Shift") return null;
  return [...mods, key].join("+");
}

/** "Ctrl+Alt+V" as separate keys for display, using symbols on a Mac. */
export function shortcutKeys(accel: string, mac: boolean): string[] {
  const sym: Record<string, string> = mac ? { Cmd: "⌘", Command: "⌘", Option: "⌥", Alt: "⌥", Shift: "⇧", Ctrl: "⌃", CommandOrControl: "⌘" } : { CommandOrControl: "Ctrl" };
  return accel.split("+").map((k) => sym[k] ?? k);
}
