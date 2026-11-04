import { useState } from "react";
import { LockKeyhole } from "lucide-react";
import { api } from "../api.ts";
import { attempt } from "../state.ts";

/** Shown while the encrypted history is locked. Nothing is recorded until it's unlocked. */
export function Lock() {
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const unlock = async () => {
    setBusy(true);
    await attempt(api.vaultUnlock(pass), "Unlocked");
    setBusy(false);
    setPass("");
  };
  return (
    <div className="lock" data-testid="lock">
      <div className="lock-card">
        <LockKeyhole size={34} />
        <h1>
          Your history is <span className="hl">locked</span>
        </h1>
        <p className="muted">It's encrypted on this computer. Enter your passphrase to see it; ClipNest starts recording again once it's unlocked.</p>
        <input className="input" type="password" autoFocus value={pass} onChange={(e) => setPass(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void unlock()} placeholder="Passphrase" aria-label="Passphrase" data-testid="unlock-pass" />
        <button className="btn primary" onClick={() => void unlock()} disabled={!pass || busy} data-testid="unlock">
          {busy ? "Checking…" : "Unlock"}
        </button>
      </div>
    </div>
  );
}
