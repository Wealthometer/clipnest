import { test } from "node:test";
import assert from "node:assert/strict";
import { acceleratorFrom, bytesText, colorFormats, highlightParts, inkOn, parseColor, plural, shortcutKeys, timeAgo } from "../src/lib.ts";

test("search highlights", () => {
  assert.deepEqual(highlightParts("…the \u0001Kofi\u0002 invoice \u0001kofi\u0002"), [
    { text: "…the ", hit: false },
    { text: "Kofi", hit: true },
    { text: " invoice ", hit: false },
    { text: "kofi", hit: true },
  ]);
  assert.deepEqual(highlightParts("plain"), [{ text: "plain", hit: false }]);
  assert.deepEqual(highlightParts(""), []);
});

test("times and sizes", () => {
  const now = new Date(2026, 10, 4, 12, 0).getTime();
  assert.equal(timeAgo(now - 10_000, now), "just now");
  assert.equal(timeAgo(now - 5 * 60_000, now), "5 min ago");
  assert.equal(timeAgo(now - 3 * 3_600_000, now), "3 h ago");
  assert.equal(timeAgo(new Date(2026, 10, 3, 9, 0).getTime(), now), "yesterday");
  assert.match(timeAgo(new Date(2026, 9, 1).getTime(), now), /1/);
  assert.equal(bytesText(512), "512 B");
  assert.equal(bytesText(1536), "1.5 KB");
  assert.equal(bytesText(5 * 1024 ** 2), "5.0 MB");
  assert.equal(plural(1, "clip"), "1 clip");
  assert.equal(plural(1200, "clip"), "1,200 clips");
});

test("colours in every notation", () => {
  assert.deepEqual(parseColor("#FFE45C"), { r: 255, g: 228, b: 92, a: 1 });
  assert.deepEqual(parseColor("#fe5"), { r: 255, g: 238, b: 85, a: 1 });
  assert.deepEqual(parseColor("rgba(10, 20, 30, 0.5)"), { r: 10, g: 20, b: 30, a: 0.5 });
  assert.deepEqual(parseColor("hsl(0, 100%, 50%)"), { r: 255, g: 0, b: 0, a: 1 });
  assert.equal(parseColor("yellow"), null);
  const f = colorFormats({ r: 255, g: 228, b: 92, a: 1 });
  assert.deepEqual(f.map((x) => x.value), ["#FFE45C", "rgb(255, 228, 92)", "hsl(50, 100%, 68%)"]);
  assert.equal(colorFormats({ r: 0, g: 0, b: 0, a: 0.5 })[0]!.value, "#00000080");
  assert.equal(inkOn({ r: 255, g: 228, b: 92, a: 1 }), "#18181B");
  assert.equal(inkOn({ r: 20, g: 20, b: 60, a: 1 }), "#FFFFFF");
});

test("shortcut recording", () => {
  const ev = (code: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }>) => ({ key: "", code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });
  assert.equal(acceleratorFrom(ev("KeyV", { ctrlKey: true, altKey: true }), false), "Ctrl+Alt+V");
  assert.equal(acceleratorFrom(ev("KeyV", { metaKey: true, shiftKey: true }), true), "Shift+Cmd+V");
  assert.equal(acceleratorFrom(ev("Backquote", { ctrlKey: true }), false), "Ctrl+`");
  assert.equal(acceleratorFrom(ev("KeyV", {}), false), null);
  assert.equal(acceleratorFrom(ev("KeyV", { shiftKey: true }), false), null);
  assert.equal(acceleratorFrom(ev("ControlLeft", { ctrlKey: true }), false), null);
  assert.deepEqual(shortcutKeys("Cmd+Shift+V", true), ["⌘", "⇧", "V"]);
  assert.deepEqual(shortcutKeys("Ctrl+Alt+V", false), ["Ctrl", "Alt", "V"]);
});
