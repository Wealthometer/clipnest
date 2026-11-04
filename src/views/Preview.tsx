import { useEffect, useState } from "react";
import { Copy, ExternalLink, FolderInput, FolderOpen, Pin, PinOff, Sparkles, Trash2 } from "lucide-react";
import { api, errorText, type Clip, type ClipDetail, type Folder } from "../api.ts";
import { bytesText, colorFormats, inkOn, parseColor, timeAgo } from "../lib.ts";
import { attempt } from "../state.ts";
import { Kbd, kindInfo, KindChip, Modal, toast, useNow } from "../ui/kit.tsx";

// Thumbnails are small data URLs; keep the ones already fetched.
const thumbs = new Map<number, string>();

export function Thumb({ id }: { id: number }) {
  const [src, setSrc] = useState(() => thumbs.get(id) ?? null);
  useEffect(() => {
    if (thumbs.has(id)) return;
    let alive = true;
    api.detail(id, false).then(
      (d) => {
        if (d.image) thumbs.set(id, d.image);
        if (alive) setSrc(d.image);
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [id]);
  return src ? <img className="thumb" src={src} alt="" /> : <span className="thumb placeholder" />;
}

export function Preview({ clip, folders, mac, onCopy }: { clip: Clip; folders: Folder[]; mac: boolean; onCopy: () => void }) {
  const [detail, setDetail] = useState<ClipDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [moving, setMoving] = useState(false);
  const [snippet, setSnippet] = useState(false);
  const now = useNow();
  const [reveal, setReveal] = useState(false);
  const info = kindInfo(clip.kind);

  useEffect(() => {
    let alive = true;
    api.detail(clip.id, true).then(
      (d) => alive && setDetail(d),
      (e) => alive && setError(errorText(e)),
    );
    return () => {
      alive = false;
    };
  }, [clip.id, clip.usedAt]);

  const text = detail?.text ?? "";
  const color = clip.kind === "color" ? parseColor(text || clip.preview) : null;

  return (
    <div className="preview" data-testid="preview">
      <div className="preview-head">
        <KindChip kind={clip.kind} size={38} />
        <div className="grow">
          <div className="preview-kind">{info.label}</div>
          <div className="small faint">
            {timeAgo(clip.createdAt, now)}
            {clip.source ? ` from ${clip.source.replace(/\.exe$/i, "")}` : ""} · {clip.kind === "image" ? `${clip.width}×${clip.height}, ` : ""}
            {bytesText(clip.bytes)}
            {clip.lines > 1 ? ` · ${clip.lines} lines` : ""}
            {clip.uses ? ` · used ${clip.uses}×` : ""}
          </div>
        </div>
      </div>

      <div className="actions">
        <button className="btn primary" onClick={onCopy} disabled={clip.locked} data-testid="copy">
          <Copy size={15} /> Copy <Kbd>↵</Kbd>
        </button>
        <button className={`btn${clip.pinned ? " on" : ""}`} onClick={() => void attempt(api.pin([clip.id], !clip.pinned), clip.pinned ? "Unpinned" : "Pinned")} data-testid="pin">
          {clip.pinned ? <PinOff size={15} /> : <Pin size={15} />} {clip.pinned ? "Unpin" : "Pin"} <Kbd>P</Kbd>
        </button>
        <button className="btn" onClick={() => setMoving(true)} data-testid="move">
          <FolderInput size={15} /> {clip.folderId ? "Move" : "File"}
        </button>
        {clip.kind !== "image" ? (
          <button className="btn icon" onClick={() => setSnippet(true)} title="Save as snippet" aria-label="Save as snippet" data-testid="to-snippet">
            <Sparkles size={15} />
          </button>
        ) : null}
        <button className="btn icon danger" onClick={() => void attempt(api.remove([clip.id]), "Deleted")} title="Delete (Del)" aria-label="Delete" data-testid="delete">
          <Trash2 size={15} />
        </button>
      </div>

      <div className="preview-body">
        {error ? <div className="muted">{error}</div> : null}
        {clip.kind === "image" && detail?.image ? (
          <div className="image-frame">
            <img src={detail.image} alt={clip.preview} data-testid="preview-image" />
          </div>
        ) : null}
        {color ? (
          <div className="color-card">
            <div className="color-big" style={{ background: text, color: inkOn(color) }}>
              {text}
            </div>
            {colorFormats(color).map((f) => (
              <button key={f.label} className="color-row" onClick={() => void navigator.clipboard.writeText(f.value).then(() => toast(`${f.label} copied`))} title="Copy">
                <span className="faint">{f.label}</span>
                <code>{f.value}</code>
                <Copy size={13} />
              </button>
            ))}
          </div>
        ) : null}
        {clip.kind === "link" && text ? (
          <div className="link-card">
            <div className="link-host">{hostOf(text)}</div>
            <button className="btn" onClick={() => void attempt(api.openLink(text))}>
              <ExternalLink size={15} /> Open in browser
            </button>
          </div>
        ) : null}
        {clip.kind === "path" && text ? (
          <button className="btn" onClick={() => void attempt(api.revealPath(text))} style={{ marginBottom: 12 }}>
            <FolderOpen size={15} /> Show in {mac ? "Finder" : "folder"}
          </button>
        ) : null}
        {clip.kind !== "image" && detail ? (
          clip.sensitive && !reveal ? (
            <button className="secret" onClick={() => setReveal(true)} data-testid="reveal">
              Hidden because it looks like a password or key. Click to show it. It's deleted 15 minutes after it was copied.
            </button>
          ) : clip.kind === "code" ? (
            <pre className="code" data-testid="preview-text">
              {text.split("\n").map((line, i) => (
                <span key={i} className="line">
                  <i>{i + 1}</i>
                  {line || " "}
                  {"\n"}
                </span>
              ))}
            </pre>
          ) : (
            <pre className={`text${clip.kind === "path" || clip.kind === "number" ? " mono" : ""}`} data-testid="preview-text">
              {text}
            </pre>
          )
        ) : null}
      </div>

      {moving ? <MoveDialog clip={clip} folders={folders} onClose={() => setMoving(false)} /> : null}
      {snippet ? <SnippetDialog body={text} onClose={() => setSnippet(false)} /> : null}
    </div>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(/^https?:\/\//i.test(url) ? url : `https://${url}`).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

function MoveDialog({ clip, folders, onClose }: { clip: Clip; folders: Folder[]; onClose: () => void }) {
  const move = async (folder: number | null, name: string) => {
    await attempt(api.move([clip.id], folder), folder ? `Filed in ${name}` : "Taken out of its folder");
    onClose();
  };
  return (
    <Modal title="File in a folder" onClose={onClose} testId="move-dialog">
      <div className="stack tight">
        {folders.map((f) => (
          <button key={f.id} className={`pick${clip.folderId === f.id ? " on" : ""}`} onClick={() => void move(f.id, f.name)} data-testid={`move-to-${f.name}`}>
            <span className="color-dot" style={{ background: f.color }} /> {f.name} <span className="faint">{f.count}</span>
          </button>
        ))}
        {!folders.length ? <p className="muted">No folders yet. Make one with the + next to Folders in the sidebar.</p> : null}
        {clip.folderId ? (
          <button className="pick" onClick={() => void move(null, "")}>
            Take out of its folder
          </button>
        ) : null}
      </div>
    </Modal>
  );
}

function SnippetDialog({ body, onClose }: { body: string; onClose: () => void }) {
  const [name, setName] = useState(body.split("\n")[0]!.slice(0, 40));
  const save = async () => {
    const id = await attempt(api.saveSnippet({ id: null, name, keyword: "", body }), "Saved as a snippet");
    if (id) onClose();
  };
  return (
    <Modal title="Save as snippet" onClose={onClose} testId="snippet-dialog">
      <div className="stack">
        <p className="muted small">Snippets stay until you delete them, and you can add placeholders like {"{date}"} later.</p>
        <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} aria-label="Snippet name" data-testid="snippet-name" onKeyDown={(e) => e.key === "Enter" && void save()} />
        <div className="row end">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={() => void save()} disabled={!name.trim()} data-testid="snippet-save">
            Save snippet
          </button>
        </div>
      </div>
    </Modal>
  );
}
