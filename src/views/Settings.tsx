import { useState } from "react";
import { Download, KeyRound, Lock, ShieldCheck, Trash2 } from "lucide-react";
import { api, type AppInfo, type Settings } from "../api.ts";
import { acceleratorFrom } from "../lib.ts";
import { attempt } from "../state.ts";
import { Kbd, Modal, Shortcut, Toggle, toast } from "../ui/kit.tsx";

const KEEP = [
  { value: 1, label: "1 day" },
  { value: 7, label: "1 week" },
  { value: 30, label: "30 days" },
  { value: 90, label: "3 months" },
  { value: 365, label: "1 year" },
  { value: 0, label: "Forever" },
];

export function SettingsView({ info }: { info: AppInfo }) {
  const s = info.settings;
  const mac = info.platform === "macos";
  const save = (patch: Partial<Settings>, ok?: string) => attempt(api.saveSettings({ ...s, ...patch }), ok);
  const [recording, setRecording] = useState(false);
  const [vault, setVault] = useState<null | "enable" | "change" | "disable">(null);
  const [confirmClear, setConfirmClear] = useState<null | "history" | "everything">(null);
  const [app, setApp] = useState("");

  const presets = mac ? ["Cmd+Shift+V", "Cmd+Option+V", "Ctrl+`"] : ["Ctrl+Alt+V", "Ctrl+Shift+V", "Ctrl+`", "Alt+V"];

  return (
    <div className="settings" data-testid="settings">
      <h1>Settings</h1>

      <section>
        <h2>Quick paste</h2>
        <div className="card">
          <div className="setting">
            <div className="grow">
              <div className="toggle-label">Shortcut</div>
              <div className="toggle-detail">Opens quick paste from any app.</div>
            </div>
            {recording ? (
              <input
                className="input recorder"
                autoFocus
                placeholder="Press keys…"
                onBlur={() => setRecording(false)}
                onKeyDown={(e) => {
                  e.preventDefault();
                  if (e.key === "Escape") return setRecording(false);
                  const accel = acceleratorFrom(e, mac);
                  if (accel) {
                    setRecording(false);
                    void save({ shortcut: accel }, `Quick paste is now ${accel}`);
                  }
                }}
                data-testid="shortcut-recorder"
              />
            ) : (
              <button className="btn" onClick={() => setRecording(true)} data-testid="shortcut">
                <Shortcut accel={s.shortcut} mac={mac} /> Change
              </button>
            )}
          </div>
          <div className="presets">
            {presets.map((p) => (
              <button key={p} className={`chip${p === s.shortcut ? " on" : ""}`} onClick={() => void save({ shortcut: p }, `Quick paste is now ${p}`)}>
                {p}
              </button>
            ))}
          </div>
          <Toggle label="Paste straight away" detail="After you pick a clip in quick paste, it's pasted into the app you were using. Off: it's only copied." value={s.autoPaste} onChange={(v) => void save({ autoPaste: v })} testId="auto-paste" />
        </div>
      </section>

      <section>
        <h2>Recording</h2>
        <div className="card">
          <Toggle label="Pause recording" detail="Nothing new is saved until you turn this off." value={s.paused} onChange={(v) => void api.setPaused(v)} testId="pause" />
          <Toggle label="Keep pictures" detail={`Screenshots and copied images, up to ${s.imageLimitMb} MB each.`} value={s.captureImages} onChange={(v) => void save({ captureImages: v })} testId="capture-images" />
          <div className="setting">
            <div className="grow">
              <div className="toggle-label">Passwords and keys</div>
              <div className="toggle-detail">Card numbers, API keys, private keys and tokens are spotted before they're saved.</div>
            </div>
            <select className="input" value={s.secretPolicy} onChange={(e) => void save({ secretPolicy: e.target.value as Settings["secretPolicy"] })} data-testid="secret-policy" aria-label="Passwords and keys">
              <option value="skip">Don't save them</option>
              <option value="mask">Keep hidden for 15 minutes</option>
            </select>
          </div>
          <div className="setting column">
            <div>
              <div className="toggle-label">Ignore copies from these apps</div>
              <div className="toggle-detail">{info.platform === "windows" ? "By program name. Copies that password managers mark as private are always ignored." : "By program name. (Which app copied something is only known on Windows.)"}</div>
            </div>
            <div className="tags">
              {s.excludedApps.map((a) => (
                <span key={a} className="tag">
                  {a}
                  <button aria-label={`Stop ignoring ${a}`} onClick={() => void save({ excludedApps: s.excludedApps.filter((x) => x !== a) })}>
                    ×
                  </button>
                </span>
              ))}
              <input
                className="input small"
                value={app}
                onChange={(e) => setApp(e.target.value)}
                placeholder="Add, e.g. KeePassXC.exe"
                onKeyDown={(e) => {
                  if (e.key === "Enter" && app.trim()) {
                    void save({ excludedApps: [...s.excludedApps, app.trim()] });
                    setApp("");
                  }
                }}
                data-testid="exclude-app"
              />
            </div>
          </div>
        </div>
      </section>

      <section>
        <h2>Keeping</h2>
        <div className="card">
          <div className="setting">
            <div className="grow">
              <div className="toggle-label">Keep copies for</div>
              <div className="toggle-detail">Pinned clips and clips in folders are kept forever.</div>
            </div>
            <select className="input" value={s.keepDays} onChange={(e) => void save({ keepDays: Number(e.target.value) })} data-testid="keep-days" aria-label="Keep copies for">
              {KEEP.map((k) => (
                <option key={k.value} value={k.value}>
                  {k.label}
                </option>
              ))}
            </select>
          </div>
          <div className="setting">
            <div className="grow">
              <div className="toggle-label">At most</div>
              <div className="toggle-detail">The oldest go first when there are more.</div>
            </div>
            <select className="input" value={s.maxItems} onChange={(e) => void save({ maxItems: Number(e.target.value) })} aria-label="At most">
              {[500, 1000, 5000, 20000, 100000].map((n) => (
                <option key={n} value={n}>
                  {n.toLocaleString()} clips
                </option>
              ))}
            </select>
          </div>
        </div>
      </section>

      <section>
        <h2>Encryption</h2>
        <div className="card">
          {info.vault === "off" ? (
            <div className="setting">
              <ShieldCheck size={22} className="faint" />
              <div className="grow">
                <div className="toggle-label">Encrypt my history</div>
                <div className="toggle-detail">Clips, pictures and snippets are encrypted on disk with a passphrase (Argon2id + XChaCha20-Poly1305). You'll enter it each time ClipNest starts. There's no way to recover a forgotten passphrase.</div>
              </div>
              <button className="btn primary" onClick={() => setVault("enable")} data-testid="vault-enable">
                <Lock size={15} /> Turn on
              </button>
            </div>
          ) : (
            <>
              <div className="setting">
                <ShieldCheck size={22} style={{ color: "var(--ok)" }} />
                <div className="grow">
                  <div className="toggle-label">Your history is encrypted</div>
                  <div className="toggle-detail">Search still works: clips are decrypted in memory while ClipNest is unlocked.</div>
                </div>
                <button className="btn" onClick={() => void attempt(api.vaultLock(), "Locked")} data-testid="vault-lock">
                  <Lock size={15} /> Lock now
                </button>
              </div>
              <div className="row" style={{ padding: "4px 0 6px" }}>
                <button className="btn small" onClick={() => setVault("change")}>
                  <KeyRound size={14} /> Change passphrase
                </button>
                <button className="btn small ghost" onClick={() => setVault("disable")} data-testid="vault-disable">
                  Turn encryption off
                </button>
              </div>
            </>
          )}
        </div>
      </section>

      <section>
        <h2>Start-up and look</h2>
        <div className="card">
          <Toggle label={`Start with ${info.platform === "windows" ? "Windows" : info.platform === "macos" ? "your Mac" : "your computer"}`} detail="Opens quietly in the tray, recording." value={s.launchAtLogin} onChange={(v) => void save({ launchAtLogin: v })} />
          <Toggle label="Start hidden" detail="Don't show this window when ClipNest starts." value={s.startHidden} onChange={(v) => void save({ startHidden: v })} />
          <div className="setting">
            <div className="grow toggle-label">Theme</div>
            <div className="seg" role="group" aria-label="Theme">
              {(["system", "dark", "light"] as const).map((t) => (
                <button key={t} className={s.theme === t ? "on" : ""} onClick={() => void save({ theme: t })} data-testid={`theme-${t}`}>
                  {t === "system" ? "Match system" : t[0]!.toUpperCase() + t.slice(1)}
                </button>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section>
        <h2>Your data</h2>
        <div className="card">
          <div className="row wrap">
            <button
              className="btn"
              onClick={async () => {
                const p = await attempt(api.exportHistory());
                if (p) toast(`Saved to ${p}`);
              }}
              data-testid="export"
            >
              <Download size={15} /> Export as JSON
            </button>
            <button className="btn" onClick={() => setConfirmClear("history")} data-testid="clear">
              <Trash2 size={15} /> Clear history
            </button>
            <button className="btn ghost danger" onClick={() => setConfirmClear("everything")}>
              Delete everything
            </button>
          </div>
          <p className="small faint" style={{ marginTop: 10 }}>
            Stored only on this computer, in <code>{info.dataDir}</code>. ClipNest never goes online. Version {info.version}.
          </p>
        </div>
      </section>

      {vault ? <VaultDialog mode={vault} onClose={() => setVault(null)} /> : null}
      {confirmClear ? (
        <Modal title={confirmClear === "history" ? "Clear history?" : "Delete everything?"} onClose={() => setConfirmClear(null)} testId="confirm-clear">
          <p className="muted">{confirmClear === "history" ? "Removes every clip that isn't pinned or in a folder." : "Removes every clip, including pinned ones and those in folders. Snippets stay."} This can't be undone.</p>
          <div className="row end">
            <button className="btn" onClick={() => setConfirmClear(null)}>
              Cancel
            </button>
            <button
              className="btn danger-fill"
              onClick={async () => {
                const n = await attempt(api.clear(confirmClear === "everything"));
                if (n !== undefined) toast(`Removed ${n} clip${n === 1 ? "" : "s"}`);
                setConfirmClear(null);
              }}
              data-testid="confirm-clear-btn"
            >
              {confirmClear === "history" ? "Clear history" : "Delete everything"}
            </button>
          </div>
        </Modal>
      ) : null}
    </div>
  );
}

function VaultDialog({ mode, onClose }: { mode: "enable" | "change" | "disable"; onClose: () => void }) {
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [busy, setBusy] = useState(false);
  const mismatch = mode !== "disable" && b.length > 0 && a !== b && mode === "enable";
  const go = async () => {
    setBusy(true);
    const r =
      mode === "enable" ? await attempt(api.vaultEnable(a), "Your history is now encrypted") : mode === "change" ? await attempt(api.vaultChange(a, b), "Passphrase changed") : await attempt(api.vaultDisable(a), "Encryption turned off");
    setBusy(false);
    if (r !== undefined) onClose();
  };
  return (
    <Modal title={mode === "enable" ? "Encrypt your history" : mode === "change" ? "Change passphrase" : "Turn encryption off"} onClose={onClose} testId="vault-dialog">
      <div className="stack">
        {mode === "enable" ? <p className="muted small">At least 8 characters. A few random words is strong and easy to type. If you forget it, your history can't be recovered.</p> : null}
        {mode === "disable" ? <p className="muted small">Everything is decrypted and stored as plain text again.</p> : null}
        <input className="input" type="password" autoFocus value={a} onChange={(e) => setA(e.target.value)} placeholder={mode === "change" ? "Current passphrase" : "Passphrase"} aria-label="Passphrase" data-testid="vault-pass" />
        {mode !== "disable" ? (
          <input
            className="input"
            type="password"
            value={b}
            onChange={(e) => setB(e.target.value)}
            placeholder={mode === "change" ? "New passphrase" : "Same again"}
            aria-label={mode === "change" ? "New passphrase" : "Repeat passphrase"}
            onKeyDown={(e) => e.key === "Enter" && void go()}
            data-testid="vault-pass2"
          />
        ) : null}
        {mismatch ? <div className="small danger-text">The two don't match yet.</div> : null}
        <div className="row end">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={() => void go()} disabled={busy || !a || (mode !== "disable" && (!b || (mode === "enable" && a !== b)))} data-testid="vault-go">
            {busy ? "Working…" : mode === "enable" ? "Encrypt" : mode === "change" ? "Change" : "Decrypt everything"} <Kbd>↵</Kbd>
          </button>
        </div>
      </div>
    </Modal>
  );
}
