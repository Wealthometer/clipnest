//! ClipNest's library: clips, folders, snippets and settings in SQLite, with full-text search,
//! and optional encryption through the vault.

use crate::classify::{self, Kind};
use crate::vault::{self, DataKey, VaultRecord};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{DateTime, Datelike, Local, TimeZone};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Copies bigger than this aren't kept (a whole log file, say).
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
/// Masked secrets are deleted after this long.
pub const SECRET_TTL_MS: i64 = 15 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Global shortcut that opens the quick-paste window.
    pub shortcut: String,
    /// After choosing a clip in quick paste, paste it into the app you were in.
    pub auto_paste: bool,
    /// Delete unpinned clips older than this many days; 0 keeps everything.
    pub keep_days: u32,
    /// At most this many clips (pinned and filed ones don't count).
    pub max_items: u32,
    pub capture_images: bool,
    /// Images larger than this (as PNG) aren't kept.
    pub image_limit_mb: u32,
    /// "skip": don't save things that look like passwords or keys. "mask": keep them hidden for 15 minutes.
    pub secret_policy: String,
    /// Apps whose copies are ignored, by program name ("KeePassXC.exe", "1Password").
    pub excluded_apps: Vec<String>,
    pub paused: bool,
    pub start_hidden: bool,
    pub launch_at_login: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shortcut: if cfg!(target_os = "macos") { "Cmd+Shift+V".into() } else { "Ctrl+Alt+V".into() },
            auto_paste: true,
            keep_days: 30,
            max_items: 5000,
            capture_images: true,
            image_limit_mb: 20,
            secret_policy: "skip".into(),
            excluded_apps: vec!["KeePass.exe".into(), "KeePassXC.exe".into(), "1Password.exe".into(), "Bitwarden.exe".into()],
            paused: false,
            start_hidden: false,
            launch_at_login: false,
            theme: "system".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: i64,
    pub kind: String,
    /// One line for lists. Empty while the vault is locked.
    pub preview: String,
    /// Search matches marked with \u{1} … \u{2}, for highlighting.
    pub snippet: Option<String>,
    pub created_at: i64,
    pub used_at: i64,
    pub uses: i64,
    pub pinned: bool,
    pub folder_id: Option<i64>,
    pub sensitive: bool,
    pub source: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub bytes: i64,
    pub lines: i64,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipDetail {
    #[serde(flatten)]
    pub clip: Clip,
    pub text: Option<String>,
    /// A data: URL of the picture (thumbnail or full size).
    pub image: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Query {
    pub text: String,
    pub kind: Option<String>,
    pub folder: Option<i64>,
    pub pinned: bool,
    /// Only clips copied (or used) in the last N days.
    pub since_days: Option<u32>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: i64,
    pub name: String,
    /// A short word to find it by in quick paste (";addr", "sig").
    pub keyword: String,
    pub body: String,
    pub uses: i64,
    pub used_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total: i64,
    pub today: i64,
    pub pinned: i64,
    pub by_kind: Vec<(String, i64)>,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Added {
    New(i64),
    /// Copied again: moved back to the top.
    Again(i64),
    Skipped(String),
}

pub enum Payload {
    Text(String),
    Image { width: usize, height: usize, rgba: Vec<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
    Off,
    Locked,
    Unlocked,
}

/// Rows read when turning encryption on: id, text, preview, image path, hash.
type TextRow = (i64, Option<String>, String, Option<String>, String);
/// Rows read when turning encryption off: id, cipher, image path, kind.
type ImageRow = (i64, Option<Vec<u8>>, Option<String>, String);

pub struct Store {
    conn: Connection,
    images: PathBuf,
    key: Option<DataKey>,
    /// Argon2 cost; tests use a cheap one.
    kdf: (u32, u32),
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS folders (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  color TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS clips (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  text TEXT,
  preview TEXT NOT NULL DEFAULT '',
  cipher BLOB,
  image TEXT,
  width INTEGER,
  height INTEGER,
  bytes INTEGER NOT NULL DEFAULT 0,
  lines INTEGER NOT NULL DEFAULT 1,
  hash TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  used_at INTEGER NOT NULL,
  uses INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL,
  sensitive INTEGER NOT NULL DEFAULT 0,
  source TEXT
);
CREATE INDEX IF NOT EXISTS clips_used ON clips(used_at DESC);
CREATE INDEX IF NOT EXISTS clips_folder ON clips(folder_id);
CREATE VIRTUAL TABLE IF NOT EXISTS clips_fts USING fts5(text, content='clips', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER IF NOT EXISTS clips_ai AFTER INSERT ON clips BEGIN
  INSERT INTO clips_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS clips_ad AFTER DELETE ON clips BEGIN
  INSERT INTO clips_fts(clips_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER IF NOT EXISTS clips_au AFTER UPDATE OF text ON clips BEGIN
  INSERT INTO clips_fts(clips_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO clips_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TABLE IF NOT EXISTS snippets (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  keyword TEXT NOT NULL DEFAULT '',
  body TEXT,
  cipher BLOB,
  uses INTEGER NOT NULL DEFAULT 0,
  used_at INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

const CLIP_COLS: &str = "c.id, c.kind, c.text, c.preview, c.cipher, c.created_at, c.used_at, c.uses, c.pinned, c.folder_id, c.sensitive, c.source, c.width, c.height, c.bytes, c.lines, c.image";

/// What's inside an encrypted clip: the text and its list preview, together.
#[derive(Serialize, Deserialize)]
struct Sealed {
    text: String,
    preview: String,
}

struct RawClip {
    clip: Clip,
    text: Option<String>,
    cipher: Option<Vec<u8>>,
    image: Option<String>,
}

fn raw(row: &Row) -> rusqlite::Result<RawClip> {
    Ok(RawClip {
        text: row.get(2)?,
        cipher: row.get(4)?,
        image: row.get(16)?,
        clip: Clip {
            id: row.get(0)?,
            kind: row.get(1)?,
            preview: row.get(3)?,
            snippet: None,
            created_at: row.get(5)?,
            used_at: row.get(6)?,
            uses: row.get(7)?,
            pinned: row.get::<_, i64>(8)? != 0,
            folder_id: row.get(9)?,
            sensitive: row.get::<_, i64>(10)? != 0,
            source: row.get(11)?,
            width: row.get(12)?,
            height: row.get(13)?,
            bytes: row.get(14)?,
            lines: row.get(15)?,
            locked: false,
        },
    })
}

/// Words of a search, each matched as a prefix ("inv 20" finds "invoice 2026").
pub fn fts_query(text: &str) -> Option<String> {
    let parts: Vec<String> = text
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@' | '#' | '/')).collect::<String>())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" AND "))
    }
}

/// For searches done in memory (encrypted libraries): all words must appear; returns a marked
/// snippet around the first match.
pub fn match_snippet(text: &str, query: &str) -> Option<String> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return Some(String::new());
    }
    let lower = text.to_lowercase();
    if !words.iter().all(|w| lower.contains(w.as_str())) {
        return None;
    }
    // Build the snippet from the first matching line, marking every word in it.
    let line = text.lines().find(|l| words.iter().any(|w| l.to_lowercase().contains(w.as_str()))).unwrap_or(text);
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let lower_chars: Vec<char> = line.to_lowercase().chars().collect();
    if lower_chars.len() != chars.len() {
        return Some(classify::preview(line, 120));
    }
    let mut i = 0;
    while i < chars.len() {
        let hit = words.iter().find(|w| {
            let wc: Vec<char> = w.chars().collect();
            i + wc.len() <= lower_chars.len() && lower_chars[i..i + wc.len()] == wc[..]
        });
        if let Some(w) = hit {
            let n = w.chars().count();
            out.push('\u{1}');
            out.extend(&chars[i..i + n]);
            out.push('\u{2}');
            i += n;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Some(out.trim().chars().take(240).collect())
}

/// Fills a snippet's placeholders: {date}, {time}, {datetime}, {weekday}, {clipboard}.
/// `{{` and `}}` give literal braces; unknown placeholders are left alone.
pub fn expand<Tz: TimeZone>(body: &str, now: &DateTime<Tz>, clipboard: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(after) = tail.strip_prefix("{{") {
            out.push('{');
            rest = after;
            continue;
        }
        if let Some(after) = tail.strip_prefix("}}") {
            out.push('}');
            rest = after;
            continue;
        }
        if tail.starts_with('{') {
            if let Some(end) = tail.find('}') {
                let name = &tail[1..end];
                let value = match name.trim().to_lowercase().as_str() {
                    "date" => Some(now.format("%Y-%m-%d").to_string()),
                    "time" => Some(now.format("%H:%M").to_string()),
                    "datetime" => Some(now.format("%Y-%m-%d %H:%M").to_string()),
                    "weekday" => Some(now.format("%A").to_string()),
                    "year" => Some(now.year().to_string()),
                    "clipboard" => Some(clipboard.to_string()),
                    _ => None,
                };
                match value {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&tail[..=end]),
                }
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

fn start_of_today(now_ms: i64) -> i64 {
    let now = Local.timestamp_millis_opt(now_ms).single().unwrap_or_else(Local::now);
    now.date_naive().and_hms_opt(0, 0, 0).and_then(|d| Local.from_local_datetime(&d).single()).map(|d| d.timestamp_millis()).unwrap_or(now_ms - 86_400_000)
}

impl Store {
    pub fn open(db: &Path, images: &Path) -> Result<Store> {
        let conn = Connection::open(db).map_err(err)?;
        Store::with(conn, images.to_path_buf(), (vault::M_COST, vault::T_COST))
    }

    pub fn with(conn: Connection, images: PathBuf, kdf: (u32, u32)) -> Result<Store> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=3000;").map_err(err)?;
        conn.execute_batch(SCHEMA).map_err(err)?;
        fs::create_dir_all(&images).map_err(err)?;
        Ok(Store { conn, images, key: None, kdf })
    }

    #[cfg(test)]
    pub fn memory(images: PathBuf) -> Store {
        Store::with(Connection::open_in_memory().unwrap(), images, (256, 1)).unwrap()
    }

    // ── Settings ─────────────────────────────────────────────────────────

    fn get_kv(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value]).map_err(err)?;
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        self.get_kv("settings").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let mut s = s.clone();
        s.keep_days = s.keep_days.min(3650);
        s.max_items = s.max_items.clamp(50, 100_000);
        s.image_limit_mb = s.image_limit_mb.clamp(1, 200);
        if s.secret_policy != "mask" {
            s.secret_policy = "skip".into();
        }
        if !["system", "dark", "light"].contains(&s.theme.as_str()) {
            s.theme = "system".into();
        }
        s.excluded_apps = s.excluded_apps.into_iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).take(100).collect();
        self.set_kv("settings", &serde_json::to_string(&s).map_err(err)?)
    }

    // ── Vault ────────────────────────────────────────────────────────────

    fn vault_record(&self) -> Option<VaultRecord> {
        self.get_kv("vault").and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn vault_state(&self) -> VaultState {
        match (self.vault_record(), &self.key) {
            (None, _) => VaultState::Off,
            (Some(_), None) => VaultState::Locked,
            (Some(_), Some(_)) => VaultState::Unlocked,
        }
    }

    pub fn unlock(&mut self, pass: &str) -> Result<()> {
        let rec = self.vault_record().ok_or("Encryption isn't on")?;
        self.key = Some(vault::unlock(&rec, pass).map_err(err)?);
        Ok(())
    }

    pub fn lock(&mut self) {
        if self.vault_record().is_some() {
            self.key = None;
        }
    }

    /// Turns encryption on: every clip, snippet and saved picture is encrypted in place.
    pub fn enable_vault(&mut self, pass: &str) -> Result<()> {
        if self.vault_record().is_some() {
            return Err("Encryption is already on".into());
        }
        let (rec, key) = vault::create(pass, self.kdf.0, self.kdf.1).map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        {
            let rows: Vec<TextRow> = tx
                .prepare("SELECT id, text, preview, image, hash FROM clips")
                .map_err(err)?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                .map_err(err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(err)?;
            for (id, text, preview, image, hash) in rows {
                let sealed = serde_json::to_vec(&Sealed { text: text.clone().unwrap_or_default(), preview }).map_err(err)?;
                let new_hash = format!("v:{}", key.fingerprint(hash.as_bytes()));
                tx.execute("UPDATE clips SET text = NULL, preview = '', cipher = ?1, hash = ?2 WHERE id = ?3", params![key.seal(&sealed), new_hash, id]).map_err(err)?;
                if let Some(img) = image {
                    for name in [format!("{img}.png"), format!("{img}.thumb.png")] {
                        let p = self.images.join(&name);
                        if let Ok(bytes) = fs::read(&p) {
                            fs::write(self.images.join(format!("{name}.enc")), key.seal(&bytes)).map_err(err)?;
                            let _ = fs::remove_file(&p);
                        }
                    }
                }
            }
            let snippets: Vec<(i64, Option<String>)> = tx
                .prepare("SELECT id, body FROM snippets")
                .map_err(err)?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(err)?;
            for (id, body) in snippets {
                tx.execute("UPDATE snippets SET body = NULL, cipher = ?1 WHERE id = ?2", params![key.seal(body.unwrap_or_default().as_bytes()), id]).map_err(err)?;
            }
            tx.execute("INSERT INTO settings(key, value) VALUES ('vault', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [serde_json::to_string(&rec).map_err(err)?])
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        // Clear what SQLite may still hold of the old plain text.
        let _ = self.conn.execute_batch("INSERT INTO clips_fts(clips_fts) VALUES('rebuild'); VACUUM;");
        self.key = Some(key);
        Ok(())
    }

    pub fn change_passphrase(&mut self, old: &str, new: &str) -> Result<()> {
        let rec = self.vault_record().ok_or("Encryption isn't on")?;
        let key = vault::unlock(&rec, old).map_err(err)?;
        let next = vault::rewrap(&key, new, self.kdf.0, self.kdf.1).map_err(err)?;
        self.set_kv("vault", &serde_json::to_string(&next).map_err(err)?)?;
        self.key = Some(key);
        Ok(())
    }

    /// Turns encryption off again, decrypting everything.
    pub fn disable_vault(&mut self, pass: &str) -> Result<()> {
        let rec = self.vault_record().ok_or("Encryption isn't on")?;
        let key = vault::unlock(&rec, pass).map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        {
            let rows: Vec<ImageRow> = tx
                .prepare("SELECT id, cipher, image, kind FROM clips")
                .map_err(err)?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(err)?;
            for (id, cipher, image, kind) in rows {
                let sealed: Sealed = match cipher.map(|c| key.open(&c)) {
                    Some(Ok(bytes)) => serde_json::from_slice(&bytes).map_err(err)?,
                    Some(Err(e)) => return Err(e.to_string()),
                    None => Sealed { text: String::new(), preview: String::new() },
                };
                let text = if kind == "image" { None } else { Some(sealed.text.clone()) };
                let mut hash = format!("t:{}", vault::plain_fingerprint(sealed.text.as_bytes()));
                if let Some(img) = image {
                    for name in [format!("{img}.png"), format!("{img}.thumb.png")] {
                        let enc = self.images.join(format!("{name}.enc"));
                        if let Ok(bytes) = fs::read(&enc) {
                            let png = key.open(&bytes).map_err(err)?;
                            if !name.contains(".thumb.") {
                                // The same fingerprint a fresh copy of this picture would get.
                                let rgba = image::load_from_memory(&png).map_err(err)?.to_rgba8().into_raw();
                                hash = format!("i:{}", vault::plain_fingerprint(&rgba));
                            }
                            fs::write(self.images.join(&name), png).map_err(err)?;
                            let _ = fs::remove_file(&enc);
                        }
                    }
                }
                tx.execute("UPDATE OR IGNORE clips SET text = ?1, preview = ?2, cipher = NULL, hash = ?3 WHERE id = ?4", params![text, sealed.preview, hash, id]).map_err(err)?;
            }
            let snippets: Vec<(i64, Option<Vec<u8>>)> = tx
                .prepare("SELECT id, cipher FROM snippets WHERE cipher IS NOT NULL")
                .map_err(err)?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(err)?;
            for (id, cipher) in snippets {
                let body = String::from_utf8(key.open(&cipher.unwrap_or_default()).map_err(err)?).map_err(err)?;
                tx.execute("UPDATE snippets SET body = ?1, cipher = NULL WHERE id = ?2", params![body, id]).map_err(err)?;
            }
            tx.execute("DELETE FROM settings WHERE key = 'vault'", []).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        self.key = None;
        Ok(())
    }

    fn locked(&self) -> bool {
        self.vault_state() == VaultState::Locked
    }

    // ── Adding ───────────────────────────────────────────────────────────

    fn fingerprint(&self, tag: &str, data: &[u8]) -> String {
        let plain = format!("{tag}:{}", vault::plain_fingerprint(data));
        match &self.key {
            Some(k) => format!("v:{}", k.fingerprint(plain.as_bytes())),
            None => plain,
        }
    }

    fn again(&self, hash: &str, now: i64) -> Result<Option<i64>> {
        let id: Option<i64> = self.conn.query_row("SELECT id FROM clips WHERE hash = ?1", [hash], |r| r.get(0)).optional().map_err(err)?;
        if let Some(id) = id {
            self.conn.execute("UPDATE clips SET used_at = ?1 WHERE id = ?2", params![now, id]).map_err(err)?;
        }
        Ok(id)
    }

    pub fn add_text(&mut self, text: &str, source: Option<&str>, now: i64) -> Result<Added> {
        if text.trim().is_empty() {
            return Ok(Added::Skipped("empty".into()));
        }
        if text.len() > MAX_TEXT_BYTES {
            return Ok(Added::Skipped("too large".into()));
        }
        if self.locked() {
            return Ok(Added::Skipped("locked".into()));
        }
        let settings = self.settings();
        let mut sensitive = false;
        if let Some(what) = classify::secret_kind(text) {
            if settings.secret_policy == "skip" {
                return Ok(Added::Skipped(format!("looked like {what}")));
            }
            sensitive = true;
        }
        let hash = self.fingerprint("t", text.as_bytes());
        if let Some(id) = self.again(&hash, now)? {
            return Ok(Added::Again(id));
        }
        let kind = classify::classify(text);
        let preview = classify::preview(text, 160);
        let lines = text.lines().count().max(1) as i64;
        let (plain, prev, cipher) = match &self.key {
            Some(k) => (None, String::new(), Some(k.seal(&serde_json::to_vec(&Sealed { text: text.to_string(), preview }).map_err(err)?))),
            None => (Some(text.to_string()), preview, None),
        };
        self.conn
            .execute(
                "INSERT INTO clips(kind, text, preview, cipher, bytes, lines, hash, created_at, used_at, sensitive, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10)",
                params![kind.as_str(), plain, prev, cipher, text.len() as i64, lines, hash, now, sensitive as i64, source],
            )
            .map_err(err)?;
        Ok(Added::New(self.conn.last_insert_rowid()))
    }

    /// Saves a copied picture as PNG (and a small thumbnail), encrypted if the vault is on.
    pub fn add_image(&mut self, width: usize, height: usize, rgba: &[u8], source: Option<&str>, now: i64) -> Result<Added> {
        if self.locked() {
            return Ok(Added::Skipped("locked".into()));
        }
        let settings = self.settings();
        if !settings.capture_images {
            return Ok(Added::Skipped("images are off".into()));
        }
        if width == 0 || height == 0 || rgba.len() != width * height * 4 {
            return Ok(Added::Skipped("not a picture".into()));
        }
        let hash = self.fingerprint("i", rgba);
        if let Some(id) = self.again(&hash, now)? {
            return Ok(Added::Again(id));
        }
        let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec()).ok_or("not a picture")?;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img.clone()).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(err)?;
        if png.len() > settings.image_limit_mb as usize * 1024 * 1024 {
            return Ok(Added::Skipped("picture too large".into()));
        }
        let scale = (360.0 / width.max(height) as f32).min(1.0);
        let thumb = image::imageops::thumbnail(&img, ((width as f32 * scale) as u32).max(1), ((height as f32 * scale) as u32).max(1));
        let mut thumb_png = Vec::new();
        image::DynamicImage::ImageRgba8(thumb).write_to(&mut std::io::Cursor::new(&mut thumb_png), image::ImageFormat::Png).map_err(err)?;
        let name = format!("{}-{}", now, &vault::plain_fingerprint(hash.as_bytes())[..12]);
        let preview = format!("Picture {width}×{height}");
        let cipher = match &self.key {
            Some(k) => {
                fs::write(self.images.join(format!("{name}.png.enc")), k.seal(&png)).map_err(err)?;
                fs::write(self.images.join(format!("{name}.thumb.png.enc")), k.seal(&thumb_png)).map_err(err)?;
                Some(k.seal(&serde_json::to_vec(&Sealed { text: String::new(), preview: preview.clone() }).map_err(err)?))
            }
            None => {
                fs::write(self.images.join(format!("{name}.png")), &png).map_err(err)?;
                fs::write(self.images.join(format!("{name}.thumb.png")), &thumb_png).map_err(err)?;
                None
            }
        };
        let prev = if self.key.is_some() { String::new() } else { preview };
        self.conn
            .execute(
                "INSERT INTO clips(kind, text, preview, cipher, image, width, height, bytes, hash, created_at, used_at, source) VALUES ('image', NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
                params![prev, cipher, name, width as i64, height as i64, png.len() as i64, hash, now, source],
            )
            .map_err(err)?;
        Ok(Added::New(self.conn.last_insert_rowid()))
    }

    // ── Reading ──────────────────────────────────────────────────────────

    fn open_sealed(&self, cipher: &Option<Vec<u8>>) -> Option<Sealed> {
        let (k, c) = (self.key.as_ref()?, cipher.as_ref()?);
        serde_json::from_slice(&k.open(c).ok()?).ok()
    }

    fn finish(&self, mut r: RawClip) -> (Clip, Option<String>) {
        if r.cipher.is_some() {
            match self.open_sealed(&r.cipher) {
                Some(s) => {
                    r.clip.preview = s.preview;
                    r.text = if r.clip.kind == "image" { None } else { Some(s.text) };
                }
                None => {
                    r.clip.locked = true;
                    r.clip.preview = String::new();
                }
            }
        }
        if r.clip.sensitive {
            r.clip.preview = "•••••••• (hidden)".into();
        }
        (r.clip, r.text)
    }

    pub fn list(&self, q: &Query, now: i64) -> Result<Vec<Clip>> {
        let mut filters: Vec<String> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(k) = q.kind.as_deref().and_then(Kind::parse) {
            filters.push(format!("c.kind = ?{}", args.len() + 1));
            args.push(k.as_str().to_string().into());
        }
        if let Some(f) = q.folder {
            filters.push(format!("c.folder_id = ?{}", args.len() + 1));
            args.push(f.into());
        }
        if q.pinned {
            filters.push("c.pinned = 1".into());
        }
        if let Some(d) = q.since_days {
            let since = if d == 1 { start_of_today(now) } else { now - d as i64 * 86_400_000 };
            filters.push(format!("c.used_at >= ?{}", args.len() + 1));
            args.push(since.into());
        }
        let limit = if q.limit == 0 { 200 } else { q.limit.min(1000) } as i64;
        let encrypted = self.vault_record().is_some();
        // A search with no usable words (only punctuation) lists everything.
        let text = if encrypted || fts_query(q.text.trim()).is_some() { q.text.trim() } else { "" };

        // Plain library with a search: SQLite's full-text index.
        if !text.is_empty() && !encrypted {
            if let Some(m) = fts_query(text) {
                let mut where_sql = vec![format!("clips_fts MATCH ?{}", args.len() + 1)];
                args.push(m.into());
                where_sql.extend(filters.iter().cloned());
                let sql = format!(
                    "SELECT {CLIP_COLS}, snippet(clips_fts, 0, char(1), char(2), '…', 16) FROM clips_fts JOIN clips c ON c.id = clips_fts.rowid WHERE {} ORDER BY c.pinned DESC, c.used_at DESC LIMIT {limit} OFFSET {}",
                    where_sql.join(" AND "),
                    q.offset
                );
                let mut stmt = self.conn.prepare(&sql).map_err(err)?;
                let rows = stmt
                    .query_map(params_from_iter(args.iter()), |r| {
                        let mut c = raw(r)?;
                        c.clip.snippet = r.get(17)?;
                        Ok(c)
                    })
                    .map_err(err)?;
                let mut out = Vec::new();
                for r in rows {
                    let (mut clip, _) = self.finish(r.map_err(err)?);
                    if clip.sensitive {
                        clip.snippet = None;
                    }
                    out.push(clip);
                }
                return Ok(out);
            }
        }

        // Everything else, and searches in an encrypted library (decrypted here, in memory).
        let where_sql = if filters.is_empty() { String::new() } else { format!("WHERE {}", filters.join(" AND ")) };
        let paged = text.is_empty();
        let sql = format!(
            "SELECT {CLIP_COLS} FROM clips c {where_sql} ORDER BY c.pinned DESC, c.used_at DESC {}",
            if paged { format!("LIMIT {limit} OFFSET {}", q.offset) } else { String::new() }
        );
        let mut stmt = self.conn.prepare(&sql).map_err(err)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), raw).map_err(err)?;
        let mut out = Vec::new();
        let mut skipped = 0u32;
        for r in rows {
            let (mut clip, body) = self.finish(r.map_err(err)?);
            if !paged {
                if clip.locked || clip.sensitive {
                    continue;
                }
                match match_snippet(body.as_deref().unwrap_or(&clip.preview), text) {
                    Some(s) => clip.snippet = Some(s),
                    None => continue,
                }
                if skipped < q.offset {
                    skipped += 1;
                    continue;
                }
                if out.len() as i64 >= limit {
                    break;
                }
            }
            out.push(clip);
        }
        Ok(out)
    }

    fn raw_one(&self, id: i64) -> Result<RawClip> {
        self.conn.query_row(&format!("SELECT {CLIP_COLS} FROM clips c WHERE c.id = ?1"), [id], raw).map_err(|_| "That clip is gone".to_string())
    }

    fn read_image(&self, name: &str, thumb: bool) -> Result<Vec<u8>> {
        let file = if thumb { format!("{name}.thumb.png") } else { format!("{name}.png") };
        match &self.key {
            Some(k) if self.images.join(format!("{file}.enc")).exists() => k.open(&fs::read(self.images.join(format!("{file}.enc"))).map_err(err)?).map_err(err),
            _ => fs::read(self.images.join(&file)).map_err(|_| "The picture file is missing".to_string()),
        }
    }

    pub fn detail(&self, id: i64, full_image: bool) -> Result<ClipDetail> {
        let r = self.raw_one(id)?;
        let image_name = r.image.clone();
        let (clip, text) = self.finish(r);
        if clip.locked {
            return Err("Unlock ClipNest to see this".into());
        }
        let image = match (&image_name, clip.kind.as_str()) {
            (Some(name), "image") => Some(format!("data:image/png;base64,{}", B64.encode(self.read_image(name, !full_image)?))),
            _ => None,
        };
        Ok(ClipDetail { clip, text, image })
    }

    /// What to put on the clipboard for a clip; counts it as used.
    pub fn payload(&self, id: i64, now: i64) -> Result<Payload> {
        let r = self.raw_one(id)?;
        let image_name = r.image.clone();
        let kind = r.clip.kind.clone();
        let (clip, text) = self.finish(r);
        if clip.locked {
            return Err("Unlock ClipNest first".into());
        }
        let p = if kind == "image" {
            let png = self.read_image(image_name.as_deref().ok_or("The picture file is missing")?, false)?;
            let img = image::load_from_memory(&png).map_err(err)?.to_rgba8();
            Payload::Image { width: img.width() as usize, height: img.height() as usize, rgba: img.into_raw() }
        } else {
            Payload::Text(text.unwrap_or_default())
        };
        self.conn.execute("UPDATE clips SET uses = uses + 1, used_at = ?1 WHERE id = ?2", params![now, id]).map_err(err)?;
        Ok(p)
    }

    // ── Changing ─────────────────────────────────────────────────────────

    fn id_list(ids: &[i64]) -> String {
        ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    }

    pub fn set_pinned(&self, ids: &[i64], pinned: bool) -> Result<()> {
        self.conn.execute(&format!("UPDATE clips SET pinned = ?1 WHERE id IN ({})", Self::id_list(ids)), [pinned as i64]).map_err(err)?;
        Ok(())
    }

    pub fn set_folder(&self, ids: &[i64], folder: Option<i64>) -> Result<()> {
        self.conn.execute(&format!("UPDATE clips SET folder_id = ?1 WHERE id IN ({})", Self::id_list(ids)), [folder]).map_err(err)?;
        Ok(())
    }

    fn remove_files(&self, names: Vec<String>) {
        for n in names {
            for f in [format!("{n}.png"), format!("{n}.thumb.png"), format!("{n}.png.enc"), format!("{n}.thumb.png.enc")] {
                let _ = fs::remove_file(self.images.join(f));
            }
        }
    }

    pub fn delete(&self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let list = Self::id_list(ids);
        let names: Vec<String> = self
            .conn
            .prepare(&format!("SELECT image FROM clips WHERE image IS NOT NULL AND id IN ({list})"))
            .map_err(err)?
            .query_map([], |r| r.get(0))
            .map_err(err)?
            .filter_map(|r| r.ok())
            .collect();
        let n = self.conn.execute(&format!("DELETE FROM clips WHERE id IN ({list})"), []).map_err(err)?;
        self.remove_files(names);
        Ok(n)
    }

    /// Clears history. Pinned clips and clips in folders stay unless `everything`.
    pub fn clear(&self, everything: bool) -> Result<usize> {
        let ids: Vec<i64> = self
            .conn
            .prepare(if everything { "SELECT id FROM clips" } else { "SELECT id FROM clips WHERE pinned = 0 AND folder_id IS NULL" })
            .map_err(err)?
            .query_map([], |r| r.get(0))
            .map_err(err)?
            .filter_map(|r| r.ok())
            .collect();
        self.delete(&ids)
    }

    /// Applies the keep-for and maximum-count settings, and forgets masked secrets after 15 minutes.
    pub fn prune(&self, now: i64) -> Result<usize> {
        let s = self.settings();
        let mut doomed: Vec<i64> = Vec::new();
        let keep = "pinned = 0 AND folder_id IS NULL";
        let mut collect = |sql: &str, args: &[i64]| -> Result<()> {
            let mut stmt = self.conn.prepare(sql).map_err(err)?;
            for id in stmt.query_map(params_from_iter(args.iter()), |r| r.get::<_, i64>(0)).map_err(err)? {
                doomed.push(id.map_err(err)?);
            }
            Ok(())
        };
        collect(&format!("SELECT id FROM clips WHERE sensitive = 1 AND {keep} AND created_at < ?1"), &[now - SECRET_TTL_MS])?;
        if s.keep_days > 0 {
            collect(&format!("SELECT id FROM clips WHERE {keep} AND used_at < ?1"), &[now - s.keep_days as i64 * 86_400_000])?;
        }
        collect(&format!("SELECT id FROM clips WHERE {keep} ORDER BY used_at DESC LIMIT -1 OFFSET ?1"), &[s.max_items as i64])?;
        doomed.sort_unstable();
        doomed.dedup();
        self.delete(&doomed)
    }

    pub fn stats(&self, now: i64) -> Result<Stats> {
        let one = |sql: &str, a: &[i64]| -> Result<i64> { self.conn.query_row(sql, params_from_iter(a.iter()), |r| r.get(0)).map_err(err) };
        let mut stmt = self.conn.prepare("SELECT kind, COUNT(*) FROM clips GROUP BY kind ORDER BY COUNT(*) DESC").map_err(err)?;
        let by_kind = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<rusqlite::Result<_>>().map_err(err)?;
        Ok(Stats {
            total: one("SELECT COUNT(*) FROM clips", &[])?,
            today: one("SELECT COUNT(*) FROM clips WHERE created_at >= ?1", &[start_of_today(now)])?,
            pinned: one("SELECT COUNT(*) FROM clips WHERE pinned = 1", &[])?,
            by_kind,
            bytes: one("SELECT COALESCE(SUM(bytes), 0) FROM clips", &[])?,
        })
    }

    // ── Folders ──────────────────────────────────────────────────────────

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self
            .conn
            .prepare("SELECT f.id, f.name, f.color, (SELECT COUNT(*) FROM clips c WHERE c.folder_id = f.id) FROM folders f ORDER BY f.position, f.name COLLATE NOCASE")
            .map_err(err)?;
        let rows = stmt.query_map([], |r| Ok(Folder { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, count: r.get(3)? })).map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)
    }

    pub fn save_folder(&self, id: Option<i64>, name: &str, color: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err("Give the folder a name (up to 60 characters)".into());
        }
        let color = if color.len() == 7 && color.starts_with('#') { color } else { "#FFE45C" };
        let r = match id {
            Some(id) => self.conn.execute("UPDATE folders SET name = ?1, color = ?2 WHERE id = ?3", params![name, color, id]).map(|_| id),
            None => self.conn.execute("INSERT INTO folders(name, color, position) VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM folders))", params![name, color]).map(|_| self.conn.last_insert_rowid()),
        };
        r.map_err(|e| if e.to_string().contains("UNIQUE") { "There's already a folder with that name".to_string() } else { e.to_string() })
    }

    pub fn delete_folder(&self, id: i64) -> Result<()> {
        // Its clips stay, back in the main history.
        self.conn.execute("UPDATE clips SET folder_id = NULL WHERE folder_id = ?1", [id]).map_err(err)?;
        self.conn.execute("DELETE FROM folders WHERE id = ?1", [id]).map_err(err)?;
        Ok(())
    }

    // ── Snippets ─────────────────────────────────────────────────────────

    pub fn snippets(&self) -> Result<Vec<Snippet>> {
        let mut stmt = self.conn.prepare("SELECT id, name, keyword, body, cipher, uses, used_at FROM snippets ORDER BY used_at DESC, name COLLATE NOCASE").map_err(err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<Vec<u8>>>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?)))
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, keyword, body, cipher, uses, used_at) = row.map_err(err)?;
            let body = match (body, cipher, &self.key) {
                (Some(b), _, _) => b,
                (None, Some(c), Some(k)) => String::from_utf8(k.open(&c).map_err(err)?).map_err(err)?,
                _ => String::new(),
            };
            out.push(Snippet { id, name, keyword, body, uses, used_at });
        }
        Ok(out)
    }

    pub fn save_snippet(&self, id: Option<i64>, name: &str, keyword: &str, body: &str, now: i64) -> Result<i64> {
        if self.locked() {
            return Err("Unlock ClipNest first".into());
        }
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the snippet a name".into());
        }
        if body.is_empty() || body.len() > 100_000 {
            return Err("A snippet needs some text (up to 100,000 characters)".into());
        }
        let keyword = keyword.trim().chars().filter(|c| !c.is_whitespace()).take(30).collect::<String>();
        let (plain, cipher) = match &self.key {
            Some(k) => (None, Some(k.seal(body.as_bytes()))),
            None => (Some(body.to_string()), None),
        };
        match id {
            Some(id) => {
                self.conn.execute("UPDATE snippets SET name = ?1, keyword = ?2, body = ?3, cipher = ?4 WHERE id = ?5", params![name, keyword, plain, cipher, id]).map_err(err)?;
                Ok(id)
            }
            None => {
                self.conn.execute("INSERT INTO snippets(name, keyword, body, cipher, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![name, keyword, plain, cipher, now]).map_err(err)?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn delete_snippet(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM snippets WHERE id = ?1", [id]).map_err(err)?;
        Ok(())
    }

    /// The snippet's text with placeholders filled in; counts it as used.
    pub fn use_snippet(&self, id: i64, clipboard: &str, now: i64) -> Result<String> {
        let s = self.snippets()?.into_iter().find(|s| s.id == id).ok_or("That snippet is gone")?;
        self.conn.execute("UPDATE snippets SET uses = uses + 1, used_at = ?1 WHERE id = ?2", params![now, id]).map_err(err)?;
        let when = Local.timestamp_millis_opt(now).single().unwrap_or_else(Local::now);
        Ok(expand(&s.body, &when, clipboard))
    }

    /// Everything as JSON (plain text), for a backup or moving to another computer.
    pub fn export_json(&self) -> Result<String> {
        if self.locked() {
            return Err("Unlock ClipNest first".into());
        }
        let mut stmt = self.conn.prepare(&format!("SELECT {CLIP_COLS} FROM clips c WHERE c.kind != 'image' ORDER BY c.created_at")).map_err(err)?;
        let rows = stmt.query_map([], raw).map_err(err)?;
        let mut clips = Vec::new();
        for r in rows {
            let (clip, text) = self.finish(r.map_err(err)?);
            if clip.sensitive {
                continue;
            }
            clips.push(serde_json::json!({ "kind": clip.kind, "text": text, "createdAt": clip.created_at, "pinned": clip.pinned, "folderId": clip.folder_id, "uses": clip.uses }));
        }
        let folders = self.folders()?;
        let snippets = self.snippets()?;
        serde_json::to_string_pretty(&serde_json::json!({ "app": "ClipNest", "version": 1, "clips": clips, "folders": folders, "snippets": snippets })).map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Store::memory(dir.path().join("images")), dir)
    }

    const T0: i64 = 1_762_200_000_000; // 3 Nov 2025

    fn ids(list: &[Clip]) -> Vec<i64> {
        list.iter().map(|c| c.id).collect()
    }

    #[test]
    fn adding_dedupes_and_classifies() {
        let (mut s, _d) = store();
        let a = match s.add_text("https://example.com/invoice/2026", Some("chrome.exe"), T0).unwrap() {
            Added::New(id) => id,
            other => panic!("{other:?}"),
        };
        assert!(matches!(s.add_text("Meeting notes\nBring slides", None, T0 + 1).unwrap(), Added::New(_)));
        // Copying the same thing again moves it back to the top instead of adding a copy.
        assert_eq!(s.add_text("https://example.com/invoice/2026", None, T0 + 2).unwrap(), Added::Again(a));
        let all = s.list(&Query::default(), T0 + 3).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, a);
        assert_eq!(all[0].kind, "link");
        assert_eq!(all[0].source.as_deref(), Some("chrome.exe"));
        assert_eq!(all[1].lines, 2);
        assert_eq!(s.add_text("   ", None, T0).unwrap(), Added::Skipped("empty".into()));
        assert!(matches!(s.add_text(&"x".repeat(MAX_TEXT_BYTES + 1), None, T0).unwrap(), Added::Skipped(_)));
    }

    #[test]
    fn secrets_are_skipped_or_masked() {
        let (mut s, _d) = store();
        assert_eq!(s.add_text("4111 1111 1111 1111", None, T0).unwrap(), Added::Skipped("looked like a card number".into()));
        let mut st = s.settings();
        st.secret_policy = "mask".into();
        s.save_settings(&st).unwrap();
        let Added::New(id) = s.add_text("AKIAIOSFODNN7EXAMPLE", None, T0).unwrap() else { panic!() };
        let c = &s.list(&Query::default(), T0).unwrap()[0];
        assert!(c.sensitive);
        assert_eq!(c.preview, "•••••••• (hidden)");
        // Still copyable, then forgotten after 15 minutes.
        assert!(matches!(s.payload(id, T0).unwrap(), Payload::Text(t) if t == "AKIAIOSFODNN7EXAMPLE"));
        assert_eq!(s.prune(T0 + SECRET_TTL_MS + 1).unwrap(), 1);
        // Hidden ones don't show up in searches either.
        s.add_text("AKIAIOSFODNN7EXAMPLF", None, T0).unwrap();
        assert!(s.list(&Query { text: "AKIA".into(), ..Default::default() }, T0).unwrap()[0].snippet.is_none());
    }

    #[test]
    fn search_filters_and_time() {
        let (mut s, _d) = store();
        let day = 86_400_000;
        s.add_text("Invoice #2041 for Kofi Studio", None, T0 - 10 * day).unwrap();
        s.add_text("invoice reminder: café payment due", None, T0 - day / 2).unwrap();
        s.add_text("#FFE45C", None, T0).unwrap();
        s.add_text("fn main() {\n    println!(\"invoice\");\n}", None, T0).unwrap();
        let q = |text: &str, since: Option<u32>, kind: Option<&str>| s.list(&Query { text: text.into(), since_days: since, kind: kind.map(String::from), ..Default::default() }, T0).unwrap();
        assert_eq!(q("invoice", None, None).len(), 3);
        assert_eq!(q("invoice", Some(3), None).len(), 2, "only the last three days");
        assert_eq!(q("inv kofi", None, None).len(), 1, "prefixes, all words");
        assert_eq!(q("cafe", None, None).len(), 1, "accents don't matter");
        assert_eq!(q("invoice", None, Some("code")).len(), 1);
        assert_eq!(q("", None, Some("color")).len(), 1);
        let hit = &q("kofi", None, None)[0];
        assert!(hit.snippet.as_deref().unwrap().contains("\u{1}Kofi\u{2}"));
        assert!(q("\"\"\" ***", None, None).len() >= 4, "punctuation-only searches list everything");
        assert_eq!(fts_query("a\"b c*"), Some("\"ab\"* AND \"c\"*".into()));
    }

    #[test]
    fn pins_folders_prune_and_clear() {
        let (mut s, _d) = store();
        let day = 86_400_000;
        let Added::New(old) = s.add_text("old note", None, T0 - 40 * day).unwrap() else { panic!() };
        let Added::New(kept) = s.add_text("old but pinned", None, T0 - 40 * day).unwrap() else { panic!() };
        let Added::New(filed) = s.add_text("old but filed", None, T0 - 40 * day).unwrap() else { panic!() };
        let Added::New(fresh) = s.add_text("fresh", None, T0).unwrap() else { panic!() };
        s.set_pinned(&[kept], true).unwrap();
        let f = s.save_folder(None, "Work", "#7CC4FF").unwrap();
        assert!(s.save_folder(None, "work", "#000000").unwrap_err().contains("already"));
        s.set_folder(&[filed], Some(f)).unwrap();
        assert_eq!(s.folders().unwrap()[0].count, 1);
        assert_eq!(s.prune(T0).unwrap(), 1, "30 days by default");
        let left = ids(&s.list(&Query::default(), T0).unwrap());
        assert!(!left.contains(&old) && left.contains(&kept) && left.contains(&filed) && left.contains(&fresh));
        assert_eq!(left[0], kept, "pinned first");
        assert_eq!(ids(&s.list(&Query { folder: Some(f), ..Default::default() }, T0).unwrap()), vec![filed]);
        assert_eq!(ids(&s.list(&Query { pinned: true, ..Default::default() }, T0).unwrap()), vec![kept]);

        let mut st = s.settings();
        st.max_items = 50;
        s.save_settings(&st).unwrap();
        for i in 0..60 {
            s.add_text(&format!("clip {i}"), None, T0 + i).unwrap();
        }
        s.prune(T0 + 100).unwrap();
        assert_eq!(s.list(&Query { limit: 1000, ..Default::default() }, T0).unwrap().len(), 52, "50 plus the pinned and filed ones");

        s.delete_folder(f).unwrap();
        assert_eq!(s.clear(false).unwrap(), 51, "everything but the pinned one");
        assert_eq!(s.clear(true).unwrap(), 1);
    }

    #[test]
    fn images_are_saved_with_thumbnails() {
        let (mut s, d) = store();
        let (w, h) = (800, 400);
        let mut rgba = vec![0u8; w * h * 4];
        for (i, px) in rgba.chunks_mut(4).enumerate() {
            px.copy_from_slice(&[(i % 256) as u8, 120, 200, 255]);
        }
        let Added::New(id) = s.add_image(w, h, &rgba, None, T0).unwrap() else { panic!() };
        assert!(matches!(s.add_image(w, h, &rgba, None, T0 + 1).unwrap(), Added::Again(x) if x == id));
        let det = s.detail(id, false).unwrap();
        assert_eq!(det.clip.preview, "Picture 800×400");
        assert!(det.image.unwrap().starts_with("data:image/png;base64,"));
        let Payload::Image { width, height, rgba: back } = s.payload(id, T0).unwrap() else { panic!() };
        assert_eq!((width, height), (w, h));
        assert_eq!(back, rgba);
        assert_eq!(fs::read_dir(d.path().join("images")).unwrap().count(), 2);
        s.delete(&[id]).unwrap();
        assert_eq!(fs::read_dir(d.path().join("images")).unwrap().count(), 0);
        assert!(matches!(s.add_image(2, 2, &[0; 3], None, T0).unwrap(), Added::Skipped(_)));
    }

    #[test]
    fn snippets_and_placeholders() {
        let (s, _d) = store();
        let id = s.save_snippet(None, "Signature", ";sig", "Thanks,\nAda\n{date} · {weekday}\n{{literal}} {unknown} {clipboard}", T0).unwrap();
        let out = s.use_snippet(id, "pasted", T0).unwrap();
        let when = Local.timestamp_millis_opt(T0).unwrap();
        assert_eq!(out, format!("Thanks,\nAda\n{} · {}\n{{literal}} {{unknown}} pasted", when.format("%Y-%m-%d"), when.format("%A")));
        assert_eq!(s.snippets().unwrap()[0].uses, 1);
        assert_eq!(s.snippets().unwrap()[0].keyword, ";sig");
        assert!(s.save_snippet(None, "", "", "x", T0).is_err());
        s.delete_snippet(id).unwrap();
        assert!(s.snippets().unwrap().is_empty());
        let t = Local.with_ymd_and_hms(2026, 11, 4, 9, 5, 0).unwrap();
        assert_eq!(expand("{time} {year} {", &t, ""), "09:05 2026 {");
    }

    #[test]
    fn vault_encrypts_everything_and_search_still_works() {
        let (mut s, d) = store();
        s.add_text("Lagos office wifi: Kofi-5G", None, T0).unwrap();
        let mut rgba = vec![9u8; 64 * 64 * 4];
        rgba[0] = 1;
        let Added::New(img) = s.add_image(64, 64, &rgba, None, T0).unwrap() else { panic!() };
        s.save_snippet(None, "Address", "addr", "12 Marina Road", T0).unwrap();
        s.enable_vault("correct horse").unwrap();
        assert_eq!(s.vault_state(), VaultState::Unlocked);

        // Nothing readable is left in the database or the picture files.
        let plain: i64 = s.conn.query_row("SELECT COUNT(*) FROM clips WHERE text IS NOT NULL OR preview != ''", [], |r| r.get(0)).unwrap();
        assert_eq!(plain, 0);
        let snip_plain: i64 = s.conn.query_row("SELECT COUNT(*) FROM snippets WHERE body IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(snip_plain, 0);
        assert!(fs::read_dir(d.path().join("images")).unwrap().all(|f| f.unwrap().file_name().to_string_lossy().ends_with(".enc")));

        // Unlocked: search (in memory), new clips, pictures and snippets all work.
        assert_eq!(s.list(&Query { text: "kofi".into(), ..Default::default() }, T0).unwrap()[0].snippet.as_deref(), Some("Lagos office wifi: \u{1}Kofi\u{2}-5G"));
        assert!(matches!(s.add_text("second secret note", None, T0 + 1).unwrap(), Added::New(_)));
        assert!(matches!(s.add_text("second secret note", None, T0 + 2).unwrap(), Added::Again(_)));
        assert!(s.detail(img, true).unwrap().image.is_some());
        assert_eq!(s.snippets().unwrap()[0].body, "12 Marina Road");

        // Locked: nothing readable, nothing recorded.
        s.lock();
        assert_eq!(s.vault_state(), VaultState::Locked);
        let locked = s.list(&Query::default(), T0).unwrap();
        assert!(locked.iter().all(|c| c.locked && c.preview.is_empty()));
        assert_eq!(s.add_text("while locked", None, T0).unwrap(), Added::Skipped("locked".into()));
        assert!(s.detail(img, false).is_err());
        assert!(s.unlock("wrong").is_err());
        s.unlock("correct horse").unwrap();
        s.change_passphrase("correct horse", "battery staple").unwrap();
        s.lock();
        s.unlock("battery staple").unwrap();

        // Off again: plain text and full-text search come back.
        s.disable_vault("battery staple").unwrap();
        assert_eq!(s.vault_state(), VaultState::Off);
        assert_eq!(s.list(&Query { text: "secret".into(), ..Default::default() }, T0).unwrap().len(), 1);
        assert_eq!(s.list(&Query { text: "lagos".into(), ..Default::default() }, T0).unwrap()[0].preview, "Lagos office wifi: Kofi-5G");
        assert!(s.detail(img, false).unwrap().image.is_some());
        assert_eq!(s.snippets().unwrap()[0].body, "12 Marina Road");
        assert!(matches!(s.add_text("Lagos office wifi: Kofi-5G", None, T0 + 5).unwrap(), Added::Again(_)), "duplicates are still spotted");
    }

    #[test]
    fn stats_settings_and_export() {
        let (mut s, _d) = store();
        s.add_text("https://a.example", None, T0).unwrap();
        s.add_text("hello", None, T0).unwrap();
        let st = s.stats(T0).unwrap();
        assert_eq!((st.total, st.today), (2, 2));
        let mut set = s.settings();
        set.max_items = 1;
        set.theme = "neon".into();
        set.excluded_apps = vec!["  ".into(), "Keeper.exe ".into()];
        s.save_settings(&set).unwrap();
        let back = s.settings();
        assert_eq!((back.max_items, back.theme.as_str(), back.excluded_apps.clone()), (50, "system", vec!["Keeper.exe".to_string()]));
        let json: serde_json::Value = serde_json::from_str(&s.export_json().unwrap()).unwrap();
        assert_eq!(json["clips"].as_array().unwrap().len(), 2);
    }
}
