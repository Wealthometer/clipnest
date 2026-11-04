#!/usr/bin/env node
/**
 * End-to-end tests against the real ClipNest binary.
 *
 * tauri-driver (cargo install tauri-driver) starts the app through WebKitWebDriver
 * (apt install webkit2gtk-driver) and we speak plain W3C WebDriver to it over HTTP: no test
 * framework, no extra dependencies. The clipboard is fed with xclip, so the Rust watcher sees
 * real copies. Run under a display, e.g. `xvfb-run -a npm run test:e2e`.
 *
 * Build first: `npx tauri build --debug --no-bundle`. Screenshots land in e2e/shots/.
 */
import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, rmSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Buffer } from "node:buffer";
import process from "node:process";

const root = resolve(import.meta.dirname, "..");
const exe = process.env.CLIPNEST_BIN ?? join(root, "src-tauri/target/debug/clipnest");
const shots = join(root, "e2e/shots");
const PORT = 4444;
const base = `http://127.0.0.1:${PORT}`;

if (!existsSync(exe)) {
  console.error(`No app binary at ${exe}. Run: npx tauri build --debug --no-bundle`);
  process.exit(1);
}
mkdirSync(shots, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── Tiny WebDriver client ────────────────────────────────────────────────

async function wd(method, path, body) {
  const res = await fetch(base + path, { method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path}: ${json.value?.message ?? res.status}`);
  return json.value;
}

const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";
let sid = "";

const s = {
  find: async (css) => (await wd("POST", `/session/${sid}/element`, { using: "css selector", value: css }))[ELEMENT],
  findAll: async (css) => (await wd("POST", `/session/${sid}/elements`, { using: "css selector", value: css })).map((e) => e[ELEMENT]),
  click: (id) => wd("POST", `/session/${sid}/element/${id}/click`, {}),
  type: (id, text) => wd("POST", `/session/${sid}/element/${id}/value`, { text }),
  // WebDriver's own clear doesn't fire React's onChange, so select all and delete like a person would.
  clear: async (id) => {
    await wd("POST", `/session/${sid}/element/${id}/click`, {});
    await s.chord(["\uE009"], "a");
    await s.keys("\uE003");
  },
  text: (id) => wd("POST", `/session/${sid}/execute/sync`, { script: "return arguments[0].innerText", args: [{ [ELEMENT]: id }] }),
  exec: (script, args = []) => wd("POST", `/session/${sid}/execute/sync`, { script, args }),
  keys: (value) =>
    wd("POST", `/session/${sid}/actions`, {
      actions: [{ type: "key", id: "kb", actions: [...value].flatMap((k) => [{ type: "keyDown", value: k }, { type: "keyUp", value: k }]) }],
    }),
  chord: (mods, key) =>
    wd("POST", `/session/${sid}/actions`, {
      actions: [{ type: "key", id: "kb", actions: [...mods.map((m) => ({ type: "keyDown", value: m })), { type: "keyDown", value: key }, { type: "keyUp", value: key }, ...mods.map((m) => ({ type: "keyUp", value: m }))] }],
    }),
  shot: async (name) => writeFileSync(join(shots, `${name}.png`), Buffer.from(await wd("GET", `/session/${sid}/screenshot`), "base64")),
};
const KEY = { enter: "", down: "", up: "", esc: "", ctrl: "", shift: "", backspace: "" };

async function waitFor(what, fn, timeout = 8000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {
      last = e;
    }
    await sleep(120);
  }
  throw new Error(`Timed out waiting for ${what}${last ? ` (${last.message})` : ""}`);
}
const el = (css) => waitFor(css, () => s.find(css));
const clickOn = async (css) => s.click(await el(css));
const count = async (css) => (await s.findAll(css)).length;
const bodyText = () => s.exec("return document.body.innerText");
const waitText = (needle) => waitFor(`text "${needle}"`, async () => (await bodyText()).includes(needle));

// ── Clipboard (X11) ──────────────────────────────────────────────────────

function copyText(text) {
  // xclip forks to own the selection; ignore its output streams or execFileSync waits on it forever.
  execFileSync("xclip", ["-selection", "clipboard", "-i"], { input: text, stdio: ["pipe", "ignore", "ignore"] });
}
function copyPng(file) {
  execFileSync("xclip", ["-selection", "clipboard", "-t", "image/png", "-i", file], { stdio: ["ignore", "ignore", "ignore"] });
}
function readClipboard() {
  return execFileSync("xclip", ["-selection", "clipboard", "-o"], { encoding: "utf8", timeout: 3000 });
}

// A 64×40 PNG with two colour bands, written by hand so the tests need no image tools.
function writePng(file) {
  const crc = (buf) => {
    let c,
      crc = 0xffffffff;
    for (const b of buf) {
      c = (crc ^ b) & 0xff;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      crc = (crc >>> 8) ^ c;
    }
    return (crc ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4);
    c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const w = 64,
    h = 40;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const o = y * (w * 3 + 1) + 1 + x * 3;
      const top = y < h / 2;
      raw[o] = top ? 255 : 30;
      raw[o + 1] = top ? 228 : 30;
      raw[o + 2] = top ? 92 : 40;
    }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return import("node:zlib").then(({ deflateSync }) =>
    writeFileSync(file, Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))])),
  );
}

// ── App lifecycle ────────────────────────────────────────────────────────

const dataDir = mkdtempSync(join(tmpdir(), "clipnest-e2e-"));
let driver;

async function startDriver() {
  driver = spawn("tauri-driver", ["--port", String(PORT)], { env: { ...process.env, CLIPNEST_DATA_DIR: dataDir }, stdio: ["ignore", "ignore", "pipe"] });
  driver.stderr.on("data", (d) => process.env.E2E_VERBOSE && process.stderr.write(d));
  await waitFor("tauri-driver", async () => (await fetch(`${base}/status`)).ok, 15000);
}

async function openApp() {
  const v = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: exe } } } });
  sid = v.sessionId;
  await el(".app, .lock");
}
async function closeApp() {
  if (!sid) return;
  await wd("DELETE", `/session/${sid}`).catch(() => {});
  sid = "";
  await sleep(600);
}
async function switchTo(kind) {
  const handles = await wd("GET", `/session/${sid}/window/handles`);
  for (const h of handles) {
    await wd("POST", `/session/${sid}/window`, { handle: h });
    const isQuick = await s.exec("return location.search.includes('window=quick')");
    if ((kind === "quick") === isQuick) return;
  }
  throw new Error(`No ${kind} window`);
}

// ── Tests ────────────────────────────────────────────────────────────────

const tests = [];
const test = (name, fn) => tests.push({ name, fn });

test("records copies, newest first, and classifies them", async () => {
  await waitText("Copy something");
  await s.shot("01-empty");
  copyText("Meeting notes: ship the release on Friday");
  await waitFor("first clip", async () => (await count("[data-testid=clip]")) === 1);
  copyText("https://github.com/tauri-apps/tauri/releases");
  await sleep(700);
  copyText("#FFE45C");
  await sleep(700);
  copyText("fn main() {\n    let x = vec![1, 2, 3];\n    println!(\"{:?}\", x);\n}");
  await sleep(700);
  copyText("hello@clipnest.app");
  await sleep(700);
  copyText("4012 8888 8888 1881"); // a card number: skipped by default
  await sleep(700);
  copyText("/home/me/projects/clipnest/README.md");
  await waitFor("6 clips", async () => (await count("[data-testid=clip]")) === 6);
  const first = await s.text((await s.findAll("[data-testid=clip-text]"))[0]);
  if (!first.includes("README.md")) throw new Error(`newest clip should be first, got ${first}`);
  if ((await bodyText()).includes("4012 8888")) throw new Error("card number should not be recorded");
  for (const kind of ["link", "color", "code", "email", "path"]) {
    if (!(await count(`.clip .k-${kind}`))) throw new Error(`no ${kind} clip`);
  }
});

test("copying the same thing again moves it to the top instead of duplicating", async () => {
  copyText("Meeting notes: ship the release on Friday");
  await waitFor("moved to top", async () => (await s.text((await s.findAll("[data-testid=clip-text]"))[0])).includes("Meeting notes"));
  if ((await count("[data-testid=clip]")) !== 6) throw new Error("should still be 6 clips");
});

test("search highlights matches and the preview follows the selection", async () => {
  const box = await el("[data-testid=search]");
  await s.type(box, "release");
  await waitFor("2 results", async () => (await count("[data-testid=clip]")) === 2);
  if ((await count("mark.hl")) < 2) throw new Error("matches should be highlighted");
  await s.keys(KEY.down);
  await waitFor("link preview", async () => (await bodyText()).includes("github.com"));
  await s.shot("02-search");
  await s.clear(box);
  await waitFor("all back", async () => (await count("[data-testid=clip]")) === 6);
});

test("previews colours with every format", async () => {
  await clickOn("[data-testid=nav-kind-color]");
  await waitFor("one colour", async () => (await count("[data-testid=clip]")) === 1);
  await clickOn("[data-testid=clip]");
  await waitText("rgb(255, 228, 92)");
  await s.shot("03-colour");
  await clickOn("[data-testid=nav-kind-code]");
  await clickOn("[data-testid=clip]");
  await waitFor("line numbers", () => count("pre.code .line"));
  await s.shot("04-code");
  await clickOn("[data-testid=nav-all]");
});

test("copy puts the clip back on the clipboard", async () => {
  const rows = await s.findAll("[data-testid=clip]");
  await s.click(rows[rows.length - 1]); // the oldest: the GitHub link
  await waitText("github.com");
  await clickOn("[data-testid=copy]");
  await waitFor("clipboard", () => readClipboard().includes("github.com/tauri-apps"));
  await sleep(900); // the watcher must not re-record our own copy as new
  if ((await count("[data-testid=clip]")) !== 6) throw new Error("copying back must not add a clip");
});

test("pin keeps a clip in Pinned", async () => {
  await clickOn("[data-testid=pin]");
  await clickOn("[data-testid=nav-pinned]");
  await waitFor("one pinned", async () => (await count("[data-testid=clip]")) === 1);
  await clickOn("[data-testid=nav-all]");
});

test("folders: create one and move a clip into it", async () => {
  await clickOn("[data-testid=new-folder]");
  await s.type(await el("[data-testid=folder-name]"), "Work");
  await clickOn("[data-testid=folder-save]");
  await el("[data-testid=nav-folder-Work]");
  await clickOn("[data-testid=clip]");
  await clickOn("[data-testid=move]");
  await clickOn("[data-testid=move-to-Work]");
  await waitFor("folder tag", () => count(".folder-tag"));
  await clickOn("[data-testid=nav-folder-Work]");
  await waitFor("1 in folder", async () => (await count("[data-testid=clip]")) === 1);
  await clickOn("[data-testid=nav-all]");
});

test("images are captured with a thumbnail", async () => {
  const png = join(dataDir, "band.png");
  await writePng(png);
  copyPng(png);
  await waitFor("image clip", () => count(".clip .k-image"), 10000);
  await clickOn("[data-testid=nav-kind-image]");
  await clickOn("[data-testid=clip]");
  await waitFor("image preview", () => count("img[data-testid=preview-image]"));
  await waitText("64×40");
  await clickOn("[data-testid=nav-all]");
  await s.shot("05-history");
});

test("snippets: create one with placeholders and use it", async () => {
  await clickOn("[data-testid=nav-snippets]");
  await clickOn("[data-testid=new-snippet]");
  await s.type(await el("[data-testid=snippet-name-input]"), "Sign-off");
  await s.type(await el("[data-testid=snippet-keyword]"), ";bye");
  await s.type(await el("[data-testid=snippet-body]"), `Thanks,${KEY.enter}Wealth — {{not a placeholder}} {year}`);
  await clickOn("[data-testid=snippet-save-btn]");
  await el("[data-testid=snippet-Sign-off]");
  await clickOn("[data-testid=snippet-copy]");
  const year = String(new Date().getFullYear());
  await waitFor("expanded snippet", () => readClipboard() === `Thanks,\nWealth — {not a placeholder} ${year}`);
  await s.shot("06-snippets");
});

test("quick paste: search, arrows, Enter copies", async () => {
  await s.exec("window.__TAURI_INTERNALS__.invoke('toggle_quick_window')");
  await sleep(600);
  await switchTo("quick");
  const box = await el("[data-testid=quick-search]");
  await s.type(box, "hello");
  await waitFor("one result", async () => (await count("[data-testid=quick-item]")) === 1);
  await s.clear(box);
  await waitFor("list", async () => (await count("[data-testid=quick-item]")) > 3);
  await s.shot("07-quick");
  await s.keys(KEY.down + KEY.down);
  const chosen = await s.text((await s.findAll(".quick-item.on .quick-text"))[0]);
  await s.chord([KEY.shift], KEY.enter); // copy without pasting
  await waitFor("clipboard has choice", () => readClipboard().trim().startsWith(chosen.trim().slice(0, 12)));
  await switchTo("main");
});

test("settings: theme switch and excluded apps", async () => {
  await clickOn("[data-testid=nav-settings]");
  await clickOn("[data-testid=theme-light]");
  await waitFor("light theme", () => s.exec("return document.documentElement.dataset.theme === 'light'"));
  await s.shot("08-settings-light");
  await clickOn("[data-testid=nav-all]");
  await s.shot("09-history-light");
  await clickOn("[data-testid=nav-settings]");
  await clickOn("[data-testid=theme-dark]");
  await clickOn("[data-testid=nav-all]");
  await clickOn("[data-testid=clip]");
  await sleep(700); // let the theme and rows finish animating
  await s.shot("11-history-dark");
  await clickOn("[data-testid=nav-kind-code]");
  await clickOn("[data-testid=clip]");
  await s.shot("12-code-dark");
});

test("pausing stops recording", async () => {
  await clickOn("[data-testid=recording]");
  await waitText("Paused");
  copyText("this must not be saved");
  await sleep(1200);
  await clickOn("[data-testid=recording]");
  await clickOn("[data-testid=nav-all]");
  await sleep(300);
  if ((await bodyText()).includes("this must not be saved")) throw new Error("recorded while paused");
});

test("encryption: enable, lock, unlock, and history survives", async () => {
  await clickOn("[data-testid=nav-settings]");
  await clickOn("[data-testid=vault-enable]");
  await s.type(await el("[data-testid=vault-pass]"), "correct horse battery");
  await s.type(await el("[data-testid=vault-pass2]"), "correct horse battery");
  await clickOn("[data-testid=vault-go]");
  await el("[data-testid=vault-lock]");
  await clickOn("[data-testid=vault-lock]");
  await el("[data-testid=lock]");
  await s.shot("10-locked");
  await s.type(await el("[data-testid=unlock-pass]"), "wrong");
  await clickOn("[data-testid=unlock]");
  await waitFor("error toast", () => count(".toast.error"));
  const pass = await el("[data-testid=unlock-pass]");
  await s.clear(pass);
  await s.type(pass, "correct horse battery");
  await clickOn("[data-testid=unlock]");
  await el(".app");
  await clickOn("[data-testid=nav-all]");
  // Search works over encrypted history too (in memory).
  await s.type(await el("[data-testid=search]"), "friday");
  await waitFor("encrypted search", async () => (await count("[data-testid=clip]")) === 1);
});

test("everything is still there after a restart (and locked)", async () => {
  await closeApp();
  await openApp();
  await el("[data-testid=lock]");
  await s.type(await el("[data-testid=unlock-pass]"), "correct horse battery");
  await clickOn("[data-testid=unlock]");
  await waitFor("history back", async () => (await count("[data-testid=clip]")) >= 7);
  await el("[data-testid=nav-folder-Work]");
});

// ── Runner ───────────────────────────────────────────────────────────────

let failed = 0;
try {
  for (const f of readdirSync(shots)) rmSync(join(shots, f));
  await startDriver();
  await openApp();
  for (const t of tests) {
    const started = Date.now();
    try {
      await t.fn();
      console.log(`  ✓ ${t.name} (${Date.now() - started}ms)`);
    } catch (e) {
      failed++;
      console.log(`  ✗ ${t.name}\n      ${e.message}`);
      await s.shot(`fail-${tests.indexOf(t)}`).catch(() => {});
      await switchTo("main").catch(() => {});
    }
  }
} finally {
  await closeApp();
  driver?.kill();
  rmSync(dataDir, { recursive: true, force: true });
}
console.log(`\n${tests.length - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
