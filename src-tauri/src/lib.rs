mod classify;
mod platform;
mod store;
mod vault;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use store::{Added, Clip, ClipDetail, Folder, Payload, Query, Settings, Snippet, Stats, Store, VaultState};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;

/// Everything the commands share. The store is behind one lock; the clipboard watcher takes it
/// only for the moment it saves a copy.
struct App {
    store: Mutex<Store>,
    settings: Mutex<Settings>,
    /// Fingerprint of what ClipNest itself just put on the clipboard, so it isn't recorded again.
    ignore: Mutex<Option<String>>,
}

impl App {
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn text_fingerprint(t: &str) -> String {
    vault::plain_fingerprint(t.as_bytes())
}

// ── Watching the clipboard ─────────────────────────────────────────────────

fn spawn_watcher(app: AppHandle) {
    thread::spawn(move || {
        let mut clipboard: Option<arboard::Clipboard> = None;
        let mut last_seq: Option<u32> = None;
        let mut last_text = String::new();
        let mut last_image = String::new();
        let mut tick: u64 = 0;
        // After a pause or while locked, whatever is on the clipboard when recording resumes was
        // copied while the user asked us not to look: take it as the new baseline, don't save it.
        let mut rebase = false;
        loop {
            thread::sleep(Duration::from_millis(450));
            tick = tick.wrapping_add(1);
            let state = app.state::<App>();
            let settings = state.settings();
            if settings.paused || matches!(state.store().vault_state(), store::VaultState::Locked) {
                rebase = true;
                last_seq = None;
                continue;
            }
            // Windows tells us cheaply when nothing has changed.
            let seq = platform::change_count();
            if seq.is_some() && seq == last_seq {
                continue;
            }
            last_seq = seq;
            if platform::copy_asks_not_to_be_recorded() {
                continue;
            }
            if clipboard.is_none() {
                clipboard = arboard::Clipboard::new().ok();
            }
            let Some(cb) = clipboard.as_mut() else { continue };
            let mut changed = false;
            match cb.get_text() {
                Ok(text) => {
                    let fp = text_fingerprint(&text);
                    if fp != last_text {
                        last_text = fp.clone();
                        last_image.clear();
                        let ours = state.ignore.lock().map(|mut g| if g.as_deref() == Some(fp.as_str()) { g.take().is_some() } else { false }).unwrap_or(false);
                        let source = platform::source_app();
                        if !ours && !rebase && !platform::is_excluded(source.as_deref(), &settings.excluded_apps) {
                            changed = matches!(state.store().add_text(&text, source.as_deref(), now()), Ok(Added::New(_) | Added::Again(_)));
                        }
                    }
                }
                Err(_) => {
                    // Reading a picture copies all of it, so without a change counter it's checked less often.
                    if settings.capture_images && (seq.is_some() || rebase || tick % 3 == 0) {
                        if let Ok(img) = cb.get_image() {
                            let fp = vault::plain_fingerprint(&img.bytes);
                            if fp != last_image {
                                last_image = fp.clone();
                                last_text.clear();
                                let ours = state.ignore.lock().map(|mut g| if g.as_deref() == Some(fp.as_str()) { g.take().is_some() } else { false }).unwrap_or(false);
                                let source = platform::source_app();
                                if !ours && !rebase && !platform::is_excluded(source.as_deref(), &settings.excluded_apps) {
                                    changed = matches!(state.store().add_image(img.width, img.height, &img.bytes, source.as_deref(), now()), Ok(Added::New(_) | Added::Again(_)));
                                }
                            }
                        }
                    }
                }
            }
            rebase = false;
            if changed {
                let _ = app.emit("clips-changed", ());
                refresh_tray(&app);
            }
        }
    });
}

fn spawn_pruner(app: AppHandle) {
    thread::spawn(move || loop {
        let removed = app.state::<App>().store().prune(now()).unwrap_or(0);
        if removed > 0 {
            let _ = app.emit("clips-changed", ());
            refresh_tray(&app);
        }
        thread::sleep(Duration::from_secs(60));
    });
}

// ── Putting things on the clipboard (and pasting) ──────────────────────────

fn put_on_clipboard(state: &App, payload: Payload) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    match payload {
        Payload::Text(t) => {
            *state.ignore.lock().map_err(|e| e.to_string())? = Some(text_fingerprint(&t));
            cb.set_text(t).map_err(|e| e.to_string())
        }
        Payload::Image { width, height, rgba } => {
            *state.ignore.lock().map_err(|e| e.to_string())? = Some(vault::plain_fingerprint(&rgba));
            cb.set_image(arboard::ImageData { width, height, bytes: Cow::Owned(rgba) }).map_err(|e| e.to_string())
        }
    }
}

/// Hides quick paste so focus returns to the app underneath, then presses Ctrl+V (Cmd+V on a Mac).
fn paste_into_previous_app(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.hide();
    }
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(180));
        use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
        if let Ok(mut e) = Enigo::new(&EnigoSettings::default()) {
            let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
            let _ = e.key(modifier, Direction::Press);
            let _ = e.key(Key::Unicode('v'), Direction::Click);
            let _ = e.key(modifier, Direction::Release);
        }
    });
}

// ── Windows ────────────────────────────────────────────────────────────────

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Opens quick paste in the middle of the screen the mouse is on, or hides it if it's open.
fn toggle_quick(app: &AppHandle) {
    let Some(w) = app.get_webview_window("quick") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    if let Ok(cursor) = app.cursor_position() {
        if let Ok(Some(m)) = app.monitor_from_point(cursor.x, cursor.y) {
            let area = m.work_area();
            if let Ok(size) = w.outer_size() {
                let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
                let y = area.position.y + (area.size.height as i32 - size.height as i32) / 3;
                let _ = w.set_position(PhysicalPosition::new(x, y));
            }
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to("quick", "quick-shown", ());
}

// ── Tray ───────────────────────────────────────────────────────────────────

fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<App>();
    let settings = state.settings();
    let recent: Vec<Clip> = state.store().list(&Query { limit: 6, ..Default::default() }, now()).unwrap_or_default();
    let open = MenuItem::with_id(app, "open", "Open ClipNest", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", format!("Quick paste    {}", settings.shortcut), true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause recording", true, settings.paused, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit ClipNest", true, None::<&str>)?;
    let mut items: Vec<MenuItem<tauri::Wry>> = Vec::new();
    for c in &recent {
        let label = if c.locked {
            "Locked".to_string()
        } else {
            let p: String = c.preview.chars().take(48).collect();
            if c.preview.chars().count() > 48 { format!("{p}…") } else { p }
        };
        items.push(MenuItem::with_id(app, format!("clip:{}", c.id), label, !c.locked, None::<&str>)?);
    }
    let menu = Menu::new(app)?;
    menu.append(&open)?;
    menu.append(&quick)?;
    menu.append(&pause)?;
    menu.append(&sep1)?;
    for i in &items {
        menu.append(i)?;
    }
    if !items.is_empty() {
        menu.append(&sep2)?;
    }
    menu.append(&quit)?;
    Ok(menu)
}

fn refresh_tray(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(menu) = tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

// ── Commands ───────────────────────────────────────────────────────────────

#[tauri::command]
fn list_clips(state: State<App>, query: Query) -> Result<Vec<Clip>, String> {
    state.store().list(&query, now())
}

#[tauri::command]
fn clip_detail(state: State<App>, id: i64, full: bool) -> Result<ClipDetail, String> {
    state.store().detail(id, full)
}

#[tauri::command]
fn copy_clip(app: AppHandle, state: State<App>, id: i64, paste: bool) -> Result<(), String> {
    let payload = state.store().payload(id, now())?;
    put_on_clipboard(&state, payload)?;
    let _ = app.emit("clips-changed", ());
    if paste {
        paste_into_previous_app(&app);
    }
    Ok(())
}

#[tauri::command]
fn pin_clips(app: AppHandle, state: State<App>, ids: Vec<i64>, pinned: bool) -> Result<(), String> {
    state.store().set_pinned(&ids, pinned)?;
    let _ = app.emit("clips-changed", ());
    Ok(())
}

#[tauri::command]
fn move_clips(app: AppHandle, state: State<App>, ids: Vec<i64>, folder: Option<i64>) -> Result<(), String> {
    state.store().set_folder(&ids, folder)?;
    let _ = app.emit("clips-changed", ());
    Ok(())
}

#[tauri::command]
fn delete_clips(app: AppHandle, state: State<App>, ids: Vec<i64>) -> Result<usize, String> {
    let n = state.store().delete(&ids)?;
    let _ = app.emit("clips-changed", ());
    refresh_tray(&app);
    Ok(n)
}

#[tauri::command]
fn clear_history(app: AppHandle, state: State<App>, everything: bool) -> Result<usize, String> {
    let n = state.store().clear(everything)?;
    let _ = app.emit("clips-changed", ());
    refresh_tray(&app);
    Ok(n)
}

#[tauri::command]
fn folders(state: State<App>) -> Result<Vec<Folder>, String> {
    state.store().folders()
}

#[tauri::command]
fn save_folder(app: AppHandle, state: State<App>, id: Option<i64>, name: String, color: String) -> Result<i64, String> {
    let r = state.store().save_folder(id, &name, &color)?;
    let _ = app.emit("clips-changed", ());
    Ok(r)
}

#[tauri::command]
fn delete_folder(app: AppHandle, state: State<App>, id: i64) -> Result<(), String> {
    state.store().delete_folder(id)?;
    let _ = app.emit("clips-changed", ());
    Ok(())
}

#[tauri::command]
fn snippets(state: State<App>) -> Result<Vec<Snippet>, String> {
    state.store().snippets()
}

#[tauri::command]
fn save_snippet(app: AppHandle, state: State<App>, id: Option<i64>, name: String, keyword: String, body: String) -> Result<i64, String> {
    let r = state.store().save_snippet(id, &name, &keyword, &body, now())?;
    let _ = app.emit("snippets-changed", ());
    Ok(r)
}

#[tauri::command]
fn delete_snippet(app: AppHandle, state: State<App>, id: i64) -> Result<(), String> {
    state.store().delete_snippet(id)?;
    let _ = app.emit("snippets-changed", ());
    Ok(())
}

#[tauri::command]
fn use_snippet(app: AppHandle, state: State<App>, id: i64, paste: bool) -> Result<String, String> {
    let current = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
    let text = state.store().use_snippet(id, &current, now())?;
    put_on_clipboard(&state, Payload::Text(text.clone()))?;
    let _ = app.emit("snippets-changed", ());
    if paste {
        paste_into_previous_app(&app);
    }
    Ok(text)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    settings: Settings,
    vault: VaultState,
    platform: &'static str,
    version: String,
    data_dir: String,
}

fn info(app: &AppHandle, state: &App) -> AppInfo {
    AppInfo {
        settings: state.settings(),
        vault: state.store().vault_state(),
        platform: if cfg!(target_os = "windows") { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" },
        version: app.package_info().version.to_string(),
        data_dir: data_dir(app).display().to_string(),
    }
}

#[tauri::command]
fn app_info(app: AppHandle, state: State<App>) -> AppInfo {
    info(&app, &state)
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.register(shortcut).map_err(|e| format!("Couldn't use {shortcut} as a shortcut ({e}). Another app may already use it."))
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<App>, settings: Settings) -> Result<AppInfo, String> {
    let before = state.settings();
    if settings.shortcut != before.shortcut {
        if let Err(e) = register_shortcut(&app, &settings.shortcut) {
            let _ = register_shortcut(&app, &before.shortcut);
            return Err(e);
        }
    }
    if settings.launch_at_login != before.launch_at_login {
        let al = app.autolaunch();
        let r = if settings.launch_at_login { al.enable() } else { al.disable() };
        r.map_err(|e| format!("Couldn't change start-up setting: {e}"))?;
    }
    state.store().save_settings(&settings)?;
    *state.settings.lock().map_err(|e| e.to_string())? = state.store().settings();
    refresh_tray(&app);
    let _ = app.emit("state-changed", ());
    Ok(info(&app, &state))
}

#[tauri::command]
fn set_paused(app: AppHandle, state: State<App>, paused: bool) -> Result<(), String> {
    let mut s = state.settings();
    s.paused = paused;
    state.store().save_settings(&s)?;
    *state.settings.lock().map_err(|e| e.to_string())? = s;
    refresh_tray(&app);
    let _ = app.emit("state-changed", ());
    Ok(())
}

#[tauri::command]
fn stats(state: State<App>) -> Result<Stats, String> {
    state.store().stats(now())
}

fn vault_changed(app: &AppHandle) {
    let _ = app.emit("state-changed", ());
    let _ = app.emit("clips-changed", ());
    let _ = app.emit("snippets-changed", ());
    refresh_tray(app);
}

#[tauri::command]
fn vault_enable(app: AppHandle, state: State<App>, passphrase: String) -> Result<(), String> {
    state.store().enable_vault(&passphrase)?;
    vault_changed(&app);
    Ok(())
}

#[tauri::command]
fn vault_unlock(app: AppHandle, state: State<App>, passphrase: String) -> Result<(), String> {
    state.store().unlock(&passphrase)?;
    vault_changed(&app);
    Ok(())
}

#[tauri::command]
fn vault_lock(app: AppHandle, state: State<App>) {
    state.store().lock();
    vault_changed(&app);
}

#[tauri::command]
fn vault_change(app: AppHandle, state: State<App>, old: String, new: String) -> Result<(), String> {
    state.store().change_passphrase(&old, &new)?;
    vault_changed(&app);
    Ok(())
}

#[tauri::command]
fn vault_disable(app: AppHandle, state: State<App>, passphrase: String) -> Result<(), String> {
    state.store().disable_vault(&passphrase)?;
    vault_changed(&app);
    Ok(())
}

#[tauri::command]
fn export_history(app: AppHandle, state: State<App>) -> Result<String, String> {
    let json = state.store().export_json()?;
    let dir = app.path().download_dir().or_else(|_| app.path().document_dir()).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M");
    let path = dir.join(format!("ClipNest-export-{stamp}.json"));
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Opens a copied link in the browser (web links only).
#[tauri::command]
fn open_link(app: AppHandle, url: String) -> Result<(), String> {
    let u = url.trim();
    let lower = u.to_lowercase();
    let full = if lower.starts_with("https://") || lower.starts_with("http://") {
        u.to_string()
    } else if lower.starts_with("www.") {
        format!("https://{u}")
    } else {
        return Err("Only web links can be opened".into());
    };
    app.opener().open_url(full, None::<&str>).map_err(|e| e.to_string())
}

/// Shows a copied file path in Explorer / Finder / the file manager.
#[tauri::command]
fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    let p = PathBuf::from(path.trim());
    if !p.exists() {
        return Err("That file or folder isn't there any more".into());
    }
    app.opener().reveal_item_in_dir(p).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_main(app: AppHandle) {
    show_main(&app);
}

#[tauri::command]
fn hide_quick(app: AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn toggle_quick_window(app: AppHandle) {
    toggle_quick(&app);
}

fn data_dir(app: &AppHandle) -> PathBuf {
    std::env::var_os("CLIPNEST_DATA_DIR").map(PathBuf::from).or_else(|| app.path().app_data_dir().ok()).unwrap_or_else(|| PathBuf::from("."))
}

// ── Start ──────────────────────────────────────────────────────────────────

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle_quick(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = data_dir(&handle);
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(&dir.join("clipnest.db"), &dir.join("images")).map_err(std::io::Error::other)?;
            let settings = store.settings();
            app.manage(App { store: Mutex::new(store), settings: Mutex::new(settings.clone()), ignore: Mutex::new(None) });

            if let Err(e) = register_shortcut(&handle, &settings.shortcut) {
                eprintln!("{e}");
            }

            let menu = tray_menu(&handle)?;
            let mut tray = TrayIconBuilder::with_id("main").tooltip("ClipNest").menu(&menu).show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                match id {
                    "open" => show_main(app),
                    "quick" => toggle_quick(app),
                    "pause" => {
                        let state = app.state::<App>();
                        let paused = !state.settings().paused;
                        let _ = set_paused(app.clone(), state, paused);
                    }
                    "quit" => app.exit(0),
                    other => {
                        if let Some(clip) = other.strip_prefix("clip:").and_then(|s| s.parse::<i64>().ok()) {
                            let state = app.state::<App>();
                            let payload = state.store().payload(clip, now());
                            if let Ok(p) = payload {
                                let _ = put_on_clipboard(&state, p);
                            }
                        }
                    }
                }
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            let hidden = std::env::args().any(|a| a == "--hidden") || settings.start_hidden;
            if !hidden {
                show_main(&handle);
            }
            spawn_watcher(handle.clone());
            spawn_pruner(handle);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the main window keeps ClipNest in the tray, still recording.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            // Quick paste goes away when you click elsewhere.
            WindowEvent::Focused(false) if window.label() == "quick" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            list_clips,
            clip_detail,
            copy_clip,
            pin_clips,
            move_clips,
            delete_clips,
            clear_history,
            folders,
            save_folder,
            delete_folder,
            snippets,
            save_snippet,
            delete_snippet,
            use_snippet,
            app_info,
            save_settings,
            set_paused,
            stats,
            vault_enable,
            vault_unlock,
            vault_lock,
            vault_change,
            vault_disable,
            export_history,
            open_link,
            reveal_path,
            open_main,
            hide_quick,
            toggle_quick_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipNest");
}
