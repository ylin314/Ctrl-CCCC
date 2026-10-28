#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arboard::Clipboard as Arboard;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext,
    ContentFormat,
};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering},
        Arc, RwLock,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{MenuBuilder, MenuEvent},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    State, WebviewWindow, Window, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(target_os = "windows")]
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, POINT, RPC_E_CHANGED_MODE},
        Graphics::Gdi::ClientToScreen,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        System::Ole::{
            SafeArrayAccessData, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayUnaccessData,
        },
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationTextPattern2, UIA_TextPattern2Id,
        },
        UI::WindowsAndMessaging::{
            GetAncestor, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo,
            GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow, WindowFromPoint,
            GA_ROOT, GUITHREADINFO, GUI_CARETBLINKING, SW_RESTORE,
        },
    },
};

const EVENT: &str = "clipboard://changed";
const CONFIG_FILE: &str = "ctrl-cccc.json";
const HISTORY_FILE: &str = "history.json";
const CF_PNG: &str = "PNG";
static QUICK_DRAGGING_UNTIL: AtomicU64 = AtomicU64::new(0);
static QUICK_PASTE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static QUICK_FOCUS_MONITOR_TOKEN: AtomicU64 = AtomicU64::new(0);
static QUICK_WAS_FOCUSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static QUICK_PREVIOUS_FOCUS: AtomicIsize = AtomicIsize::new(0);

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn quick_dragging() -> bool {
    let until = QUICK_DRAGGING_UNTIL.load(Ordering::Acquire);
    until != 0 && now_millis() < until
}

#[cfg(target_os = "windows")]
fn cursor_is_over_quick_window(window_hwnd: isize) -> bool {
    let hwnd = HWND(window_hwnd as *mut core::ffi::c_void);

    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return true;
    }

    let hit_window = unsafe { WindowFromPoint(cursor) };
    if hit_window.is_invalid() {
        return false;
    }

    let root_window = unsafe { GetAncestor(hit_window, GA_ROOT) };
    !root_window.is_invalid() && root_window.0 == hwnd.0
}

/// Native window dragging does not reliably deliver a final pointer event to
/// the webview. If focus is lost while the cursor is already over another
/// window, the drag must have ended, so clear its protection immediately.
#[cfg(target_os = "windows")]
fn clear_stale_quick_drag(window_hwnd: isize) {
    if quick_dragging() && !cursor_is_over_quick_window(window_hwnd) {
        QUICK_DRAGGING_UNTIL.store(0, Ordering::Release);
    }
}

#[cfg(not(target_os = "windows"))]
fn clear_stale_quick_drag(_window_hwnd: isize) {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl FromStr for Corner {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "top-left" | "topleft" => Ok(Self::TopLeft),
            "top-right" | "topright" => Ok(Self::TopRight),
            "bottom-left" | "bottomleft" => Ok(Self::BottomLeft),
            "bottom-right" | "bottomright" => Ok(Self::BottomRight),
            _ => Err(()),
        }
    }
}

fn default_history_limit() -> usize {
    50
}

fn default_quick_hotkey() -> String {
    "super+c".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    position: Corner,
    duration_ms: u64,
    fade_ms: u64,
    autostart: bool,
    #[serde(default = "default_history_limit")]
    history_limit: usize,
    #[serde(default = "default_quick_hotkey")]
    quick_hotkey: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: Corner::BottomRight,
            duration_ms: 2_200,
            fade_ms: 180,
            autostart: false,
            history_limit: 50,
            quick_hotkey: "super+c".to_string(),
        }
    }
}

impl Config {
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("ctrl-cccc").join(CONFIG_FILE))
    }

    fn load_or_create() -> Self {
        let default = Self::default();
        let Some(path) = Self::config_path() else {
            return default;
        };

        if !path.exists() {
            let _ = Self::write(&default);
            return default;
        }

        match fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
        {
            Some(mut config) => {
                config.normalize();
                config
            }
            None => default,
        }
    }

    fn write(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(path) = Self::config_path() else {
            return Err("config directory is unavailable".into());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(config)?)?;
        Ok(())
    }

    fn normalize(&mut self) {
        self.duration_ms = self.duration_ms.clamp(300, 30_000);
        self.fade_ms = self.fade_ms.min(self.duration_ms / 2);
        self.history_limit = self.history_limit.clamp(5, 500);
        self.quick_hotkey = self.quick_hotkey.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize)]
struct ToastPayload {
    kind: &'static str,
    message: String,
    duration_ms: u64,
    fade_ms: u64,
    /// History entry whose PNG snapshot should be rendered as a thumbnail.
    /// `None` for text, file and empty toasts.
    image_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClipboardEntry {
    id: String,
    kind: String,
    title: String,
    preview: String,
    timestamp_ms: u64,
    text: Option<String>,
    files: Option<Vec<String>>,
    image_path: Option<String>,
    image_width: Option<u32>,
    image_height: Option<u32>,
}

impl ClipboardEntry {
    fn signature(&self) -> String {
        match (&self.text, &self.files, self.image_width, self.image_height) {
            (Some(text), _, _, _) => format!("text:{text}"),
            (_, Some(files), _, _) => format!("files:{}", files.join("\n")),
            (_, _, Some(width), Some(height)) => format!("image:{width}x{height}"),
            _ => format!("preview:{}", self.preview),
        }
    }
}

type SharedConfig = Arc<RwLock<Config>>;
type SharedHistory = Arc<RwLock<Vec<ClipboardEntry>>>;

struct AppState {
    config: SharedConfig,
    history: SharedHistory,
}

struct ToastWatcher {
    files_clipboard: ClipboardContext,
    text_clipboard: Arboard,
    app: AppHandle,
    config: SharedConfig,
    history: SharedHistory,
}

fn history_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("ctrl-cccc").join("history"))
}

fn history_file() -> Option<PathBuf> {
    history_dir().map(|dir| dir.join(HISTORY_FILE))
}

fn load_history() -> SharedHistory {
    let entries = history_file()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Vec<ClipboardEntry>>(&text).ok())
        .unwrap_or_default();
    Arc::new(RwLock::new(entries))
}

fn save_history(entries: &[ClipboardEntry]) -> Result<(), String> {
    let Some(dir) = history_dir() else {
        return Err("history directory is unavailable".to_string());
    };
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(HISTORY_FILE);
    let json = serde_json::to_string_pretty(entries).map_err(|error| error.to_string())?;
    fs::write(&path, json).map_err(|error| error.to_string())
}

fn add_history_entry(history: &SharedHistory, entry: ClipboardEntry, limit: usize) {
    let Ok(mut entries) = history.write() else {
        return;
    };

    let signature = entry.signature();
    entries.retain(|item| item.signature() != signature);
    entries.insert(0, entry);
    entries.sort_by(|left, right| right.timestamp_ms.cmp(&left.timestamp_ms));
    entries.truncate(limit);

    let image_paths = entries
        .iter()
        .filter_map(|item| item.image_path.clone())
        .collect::<Vec<_>>();
    if let Err(error) = save_history(&entries) {
        eprintln!("failed to save clipboard history: {error}");
    }

    if let Some(dir) = history_dir() {
        if let Ok(paths) = fs::read_dir(&dir) {
            for path in paths.flatten() {
                let path = path.path();
                if path.extension().is_some_and(|extension| extension == "png")
                    && !image_paths
                        .iter()
                        .any(|image_path| image_path == &path.to_string_lossy())
                {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}

fn next_entry_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    format!("{}-{}", timestamp, SEQUENCE.fetch_add(1, Ordering::Relaxed))
}

fn now_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

impl ClipboardHandler for ToastWatcher {
    fn on_clipboard_change(&mut self) {
        thread::sleep(Duration::from_millis(50));
        let config = match self.config.read() {
            Ok(config) => config.clone(),
            Err(_) => return,
        };

        let entry = capture_entry(&self.files_clipboard, &mut self.text_clipboard);
        let payload = match &entry {
            Ok(entry) => toast_payload(entry, &config),
            Err(_) => read_toast_payload(&self.files_clipboard, &mut self.text_clipboard, &config),
        };

        // Register the entry before announcing it so the toast can resolve its
        // thumbnail snapshot, and so the history views already list the newest
        // item when they refresh on the same event.
        if let Ok(entry) = entry {
            add_history_entry(&self.history, entry, config.history_limit);
        }

        let _ = self.app.emit(EVENT, payload);
    }
}
#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Config, String> {
    state
        .config
        .read()
        .map(|config| config.clone())
        .map_err(|_| "settings lock poisoned".to_string())
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Config,
) -> Result<Config, String> {
    settings.normalize();
    Config::write(&settings).map_err(|error| error.to_string())?;

    {
        let mut config = state
            .config
            .write()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *config = settings.clone();
    }

    apply_autostart(&app, &settings)?;
    apply_quick_hotkey(&app, &settings.quick_hotkey)?;

    {
        let Ok(mut history) = state.history.write() else {
            return Err("history lock poisoned".to_string());
        };
        history.truncate(settings.history_limit);
        if let Err(error) = save_history(&history) {
            eprintln!("failed to save clipboard history: {error}");
        }
    }

    Ok(settings)
}

#[tauri::command]
fn clear_quick_hotkey(app: AppHandle, state: State<'_, AppState>) -> Result<Config, String> {
    let mut config = state
        .config
        .read()
        .map_err(|_| "settings lock poisoned".to_string())?
        .clone();

    apply_quick_hotkey(&app, "")?;

    config.quick_hotkey.clear();
    Config::write(&config).map_err(|error| error.to_string())?;

    {
        let mut stored = state
            .config
            .write()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *stored = config.clone();
    }

    Ok(config)
}

#[tauri::command]
fn set_quick_hotkey_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    paused: bool,
) -> Result<(), String> {
    if paused {
        app.global_shortcut()
            .unregister_all()
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let shortcut = state
        .config
        .read()
        .map_err(|_| "settings lock poisoned".to_string())?
        .quick_hotkey
        .clone();
    apply_quick_hotkey(&app, &shortcut)
}
#[tauri::command]
fn get_history(state: State<'_, AppState>) -> Result<Vec<ClipboardEntry>, String> {
    let limit = state
        .config
        .read()
        .map_err(|_| "settings lock poisoned".to_string())?
        .history_limit;
    let history = state
        .history
        .read()
        .map_err(|_| "history lock poisoned".to_string())?;
    Ok(history.iter().take(limit).cloned().collect())
}

#[tauri::command]
fn get_history_image(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let path = state
        .history
        .read()
        .map_err(|_| "history lock poisoned".to_string())?
        .iter()
        .find(|entry| entry.id == id)
        .and_then(|entry| entry.image_path.clone())
        .ok_or("image not found")?;
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
fn copy_history(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let entry = find_history_entry(state.history.clone(), id)?;
    let mut clipboard = ClipboardContext::new().map_err(|error| error.to_string())?;
    write_entry_to_clipboard(&mut clipboard, &entry)
}

#[tauri::command]
fn open_history(app: AppHandle) -> Result<(), String> {
    show_history_window(&app)
}

#[tauri::command]
fn open_quick_paste(app: AppHandle) -> Result<(), String> {
    show_quick_window(&app)
}

#[tauri::command]
fn hide_quick_paste(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("quick") {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn hide_toast(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("toast") {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn set_quick_paste_dragging(dragging: bool) {
    let lifetime = if dragging { now_millis() + 5_000 } else { 0 };
    QUICK_DRAGGING_UNTIL.store(lifetime, Ordering::Release);
}

#[tauri::command]
fn begin_quick_paste_drag(window: WebviewWindow) -> Result<(), String> {
    // The focus-loss handler may run as soon as the native drag starts. Mark
    // the drag first, then hand control to the OS, so the two operations
    // cannot race in the frontend.
    QUICK_DRAGGING_UNTIL.store(now_millis() + 5_000, Ordering::Release);
    if let Err(error) = window.start_dragging() {
        QUICK_DRAGGING_UNTIL.store(0, Ordering::Release);
        return Err(error.to_string());
    }
    Ok(())
}
#[tauri::command]
fn quick_paste(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let entry = find_history_entry(state.history.clone(), id)?;
    if QUICK_PASTE_IN_PROGRESS.swap(true, Ordering::Relaxed) {
        return Err("quick paste is already in progress".to_string());
    }

    // Prevent the focus monitor from closing the panel while focus is being
    // handed back to the previous target window.
    let result = (|| {
        let mut clipboard = ClipboardContext::new().map_err(|error| error.to_string())?;
        write_entry_to_clipboard(&mut clipboard, &entry)?;

        #[cfg(target_os = "windows")]
        restore_previous_focus();

        thread::sleep(Duration::from_millis(160));

        let mut enigo = Enigo::new(&Settings::default()).map_err(|error| error.to_string())?;
        let modifier = if cfg!(target_os = "macos") {
            Key::Meta
        } else {
            Key::Control
        };
        enigo
            .key(modifier, Direction::Press)
            .map_err(|error| error.to_string())?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|error| error.to_string())?;
        enigo
            .key(modifier, Direction::Release)
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    QUICK_PASTE_IN_PROGRESS.store(false, Ordering::Relaxed);

    if result.is_ok() {
        if let Some(window) = app.get_webview_window("quick") {
            let _ = window.hide();
        }
    }
    result
}

#[tauri::command]
fn place_toast(
    window: WebviewWindow,
    state: State<'_, AppState>,
    avail_left: f64,
    avail_top: f64,
    avail_width: f64,
    avail_height: f64,
    content_height: f64,
) -> Result<(), String> {
    let config = state
        .config
        .read()
        .map_err(|_| "settings lock poisoned".to_string())?
        .clone();
    let width = 380.0_f64;
    let height = content_height.clamp(48.0, 640.0).ceil();

    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|error| error.to_string())?;

    let x = match config.position {
        Corner::TopLeft | Corner::BottomLeft => avail_left + 20.0,
        Corner::TopRight | Corner::BottomRight => avail_left + avail_width - width - 20.0,
    };

    let y = match config.position {
        Corner::TopLeft | Corner::TopRight => avail_top + 20.0,
        Corner::BottomLeft | Corner::BottomRight => avail_top + avail_height - height - 20.0,
    };

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;

    Ok(())
}

fn apply_autostart(app: &AppHandle, config: &Config) -> Result<(), String> {
    let autostart = app.autolaunch();
    let result = if config.autostart {
        autostart.enable()
    } else {
        autostart.disable()
    };
    result.map_err(|error| error.to_string())
}

fn apply_quick_hotkey(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let global_shortcut = app.global_shortcut();
    global_shortcut
        .unregister_all()
        .map_err(|error| error.to_string())?;
    if shortcut.is_empty() {
        return Ok(());
    }
    global_shortcut
        .register(shortcut)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_history_window(app: &AppHandle) -> Result<(), String> {
    show_main_window(app);
    if let Some(window) = app.get_webview_window("main") {
        window
            .eval("window.ctrlCCCC?.showHistory?.()")
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn show_quick_window(app: &AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("quick") else {
        return Err("quick paste window not found".to_string());
    };
    QUICK_WAS_FOCUSED.store(false, Ordering::Relaxed);

    #[cfg(target_os = "windows")]
    if !window.is_visible().unwrap_or(false) {
        let foreground = unsafe { GetForegroundWindow() };
        if !foreground.is_invalid() {
            QUICK_PREVIOUS_FOCUS.store(foreground.0 as isize, Ordering::Relaxed);
        }
    }

    monitor_quick_focus(app.app_handle().clone());

    let (anchor_x, anchor_y) = quick_anchor_position(app);
    let x = anchor_x + 14.0;
    let y = anchor_y + 20.0;
    let outer_size = window
        .outer_size()
        .unwrap_or_else(|_| PhysicalSize::new(360, 460));
    let monitor = match window.monitor_from_point(x, y) {
        Ok(Some(monitor)) => Some(monitor),
        _ => window.primary_monitor().ok().flatten(),
    };

    let (x, y) = if let Some(monitor) = monitor {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let min_x = monitor_position.x as f64 + 8.0;
        let min_y = monitor_position.y as f64 + 8.0;
        let max_x =
            (monitor_position.x + monitor_size.width as i32) as f64 - outer_size.width as f64 - 8.0;
        let max_y = (monitor_position.y + monitor_size.height as i32) as f64
            - outer_size.height as f64
            - 8.0;
        (
            x.clamp(min_x, max_x.max(min_x)),
            y.clamp(min_y, max_y.max(min_y)),
        )
    } else {
        (x, y)
    };

    window
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .map_err(|error| error.to_string())?;
    window
        .eval("window.ctrlCCCC?.refresh?.()")
        .map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn monitor_quick_focus(app: AppHandle) {
    let token = QUICK_FOCUS_MONITOR_TOKEN.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        // Give the webview one short scheduling cycle to receive focus.
        thread::sleep(Duration::from_millis(80));

        loop {
            if QUICK_FOCUS_MONITOR_TOKEN.load(Ordering::Relaxed) != token {
                return;
            }

            let Some(window) = app.get_webview_window("quick") else {
                return;
            };
            if !window.is_visible().unwrap_or(false) {
                return;
            }
            if !QUICK_WAS_FOCUSED.load(Ordering::Relaxed)
                || QUICK_PASTE_IN_PROGRESS.load(Ordering::Relaxed)
                || quick_dragging()
            {
                if let Ok(hwnd) = window.hwnd() {
                    clear_stale_quick_drag(hwnd.0 as isize);
                }
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            if !window.is_focused().unwrap_or(false) {
                let _ = window.hide();
                return;
            }

            thread::sleep(Duration::from_millis(50));
        }
    });
}
#[cfg(target_os = "windows")]
fn restore_previous_focus() {
    let value = QUICK_PREVIOUS_FOCUS.load(Ordering::Relaxed);
    if value == 0 {
        return;
    }

    let hwnd = HWND(value as *mut core::ffi::c_void);
    if hwnd.is_invalid() {
        return;
    }

    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
fn constrain_quick_window(window: &Window) {
    let Ok(current) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };

    let center_x = current.x as f64 + size.width as f64 / 2.0;
    let center_y = current.y as f64 + size.height as f64 / 2.0;
    let monitor = match window.monitor_from_point(center_x, center_y) {
        Ok(Some(monitor)) => Some(monitor),
        _ => window.primary_monitor().ok().flatten(),
    };
    let Some(monitor) = monitor else {
        return;
    };

    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let x = (current.x.max(monitor_position.x + 8)).min(
        (monitor_position.x + monitor_size.width as i32 - size.width as i32 - 8)
            .max(monitor_position.x + 8),
    );
    let y = (current.y.max(monitor_position.y + 8)).min(
        (monitor_position.y + monitor_size.height as i32 - size.height as i32 - 8)
            .max(monitor_position.y + 8),
    );

    if x != current.x || y != current.y {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

#[cfg(target_os = "windows")]
fn quick_anchor_position(app: &AppHandle) -> (f64, f64) {
    caret_rect_windows()
        .map(caret_open_position)
        .unwrap_or_else(|| cursor_position(app))
}

#[cfg(not(target_os = "windows"))]
fn quick_anchor_position(app: &AppHandle) -> (f64, f64) {
    cursor_position(app)
}

fn caret_open_position(rect: (f64, f64, f64, f64)) -> (f64, f64) {
    let (x, y, width, height) = rect;
    if width > 2.0 && height > 2.0 {
        (x + width + 14.0, y + height / 2.0 - 230.0)
    } else {
        (x + 14.0, y + 20.0)
    }
}

fn cursor_position(app: &AppHandle) -> (f64, f64) {
    app.cursor_position()
        .map(|position| (position.x as f64, position.y as f64))
        .unwrap_or((200.0, 200.0))
}

#[cfg(target_os = "windows")]
fn caret_rect_windows() -> Option<(f64, f64, f64, f64)> {
    let native = unsafe { native_caret_rect() };
    native
        .or_else(|| unsafe { uia_caret_rect() })
        .or_else(|| unsafe { uia_focused_rect() })
}

#[cfg(target_os = "windows")]
unsafe fn native_caret_rect() -> Option<(f64, f64, f64, f64)> {
    let foreground = GetForegroundWindow();
    if foreground.is_invalid() {
        return None;
    }

    let thread_id = GetWindowThreadProcessId(foreground, None);
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..std::mem::zeroed()
    };

    GetGUIThreadInfo(thread_id, &mut info).ok()?;
    if (info.flags & GUI_CARETBLINKING).0 == 0 || info.hwndCaret.is_invalid() {
        return None;
    }

    let mut top_left = POINT {
        x: info.rcCaret.left,
        y: info.rcCaret.top,
    };
    let mut bottom_right = POINT {
        x: info.rcCaret.right,
        y: info.rcCaret.bottom,
    };
    if !ClientToScreen(info.hwndCaret, &mut top_left).as_bool()
        || !ClientToScreen(info.hwndCaret, &mut bottom_right).as_bool()
    {
        return None;
    }

    Some((
        top_left.x as f64,
        top_left.y as f64,
        (bottom_right.x - top_left.x) as f64,
        (bottom_right.y - top_left.y) as f64,
    ))
}

#[cfg(target_os = "windows")]
unsafe fn uia_caret_rect() -> Option<(f64, f64, f64, f64)> {
    let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return None;
    }

    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
    let focused = automation.GetFocusedElement().ok()?;
    let pattern: IUIAutomationTextPattern2 =
        focused.GetCurrentPatternAs(UIA_TextPattern2Id).ok()?;

    let mut active = BOOL(0);
    let range = pattern.GetCaretRange(&mut active).ok()?;
    if !active.as_bool() {
        return None;
    }

    let rect_array = range.GetBoundingRectangles().ok()?;
    if rect_array.is_null() {
        return None;
    }

    let lower = SafeArrayGetLBound(rect_array, 1).ok()?;
    let upper = SafeArrayGetUBound(rect_array, 1).ok()?;
    let count = (upper - lower + 1).max(0) as usize;
    if count < 4 {
        return None;
    }

    let mut data = std::ptr::null_mut();
    SafeArrayAccessData(rect_array, &mut data).ok()?;
    let values = std::slice::from_raw_parts(data as *const f64, count);
    let result = if values.len() >= 4 && values[2] >= 0.0 && values[3] >= 0.0 {
        Some((values[0], values[1], values[2], values[3]))
    } else {
        None
    };
    SafeArrayUnaccessData(rect_array).ok()?;
    result
}

#[cfg(target_os = "windows")]
unsafe fn uia_focused_rect() -> Option<(f64, f64, f64, f64)> {
    let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return None;
    }

    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
    let focused = automation.GetFocusedElement().ok()?;
    let rect = focused.CurrentBoundingRectangle().ok()?;
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }

    Some((
        rect.left as f64,
        rect.top as f64,
        (rect.right - rect.left) as f64,
        (rect.bottom - rect.top) as f64,
    ))
}
fn find_history_entry(history: SharedHistory, id: String) -> Result<ClipboardEntry, String> {
    history
        .read()
        .map_err(|_| "history lock poisoned".to_string())?
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or_else(|| "history entry not found".to_string())
}

fn write_entry_to_clipboard(
    clipboard: &mut ClipboardContext,
    entry: &ClipboardEntry,
) -> Result<(), String> {
    if let Some(text) = &entry.text {
        clipboard
            .set_text(text.clone())
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    if let Some(files) = &entry.files {
        clipboard
            .set_files(files.clone())
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    if let Some(path) = &entry.image_path {
        let image = RustImage::from_path(path).map_err(|error| error.to_string())?;
        clipboard
            .set_image(image)
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    Err("this history entry cannot be copied".to_string())
}
fn run_clipboard_watcher(
    app: AppHandle,
    config: SharedConfig,
    history: SharedHistory,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let files_clipboard = ClipboardContext::new()?;
    let text_clipboard = Arboard::new()?;
    let watcher = ClipboardWatcherContext::new()?;
    let mut watcher = watcher;

    watcher.add_handler(ToastWatcher {
        files_clipboard,
        text_clipboard,
        app,
        config,
        history,
    });
    watcher.start_watch();
    Ok(())
}

fn read_toast_payload(
    files_clipboard: &ClipboardContext,
    text_clipboard: &mut Arboard,
    config: &Config,
) -> ToastPayload {
    let (kind, message) = read_content(files_clipboard, text_clipboard);
    ToastPayload {
        kind,
        message,
        duration_ms: config.duration_ms,
        fade_ms: config.fade_ms,
        image_id: None,
    }
}

/// Builds the toast for a freshly captured entry. The entry is registered in
/// the history before the event is emitted, so `image_id` always resolves to a
/// PNG snapshot the toast window can fetch.
fn toast_payload(entry: &ClipboardEntry, config: &Config) -> ToastPayload {
    let (kind, message) = match entry.kind.as_str() {
        "image" => (
            "图片",
            match (entry.image_width, entry.image_height) {
                (Some(width), Some(height)) => format!("[{width}]x[{height}]"),
                _ => "（无法读取尺寸）".to_string(),
            },
        ),
        "files" => ("文件", format!("文件：{}", entry.preview)),
        _ => (
            "文本",
            match &entry.text {
                Some(text) if !text.is_empty() => truncate_chars(text, 200),
                _ => "（空）".to_string(),
            },
        ),
    };

    ToastPayload {
        kind,
        message,
        duration_ms: config.duration_ms,
        fade_ms: config.fade_ms,
        image_id: entry.image_path.as_ref().map(|_| entry.id.clone()),
    }
}

/// Width and height of a PNG byte stream, read straight from its IHDR chunk.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // 8 byte signature + 4 byte chunk length + 4 byte "IHDR" + 13 byte header.
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Copies the PNG payload published on the clipboard to `path` unchanged.
///
/// Returns `None` when the clipboard has no PNG format, so the caller can fall
/// back to decoding the raw image data.
fn save_clipboard_png(clipboard: &ClipboardContext, path: &std::path::Path) -> Option<(u32, u32)> {
    let bytes = clipboard.get_buffer(CF_PNG).ok()?;
    // Validate before touching the disk so a malformed payload never lands in
    // the history directory.
    let size = png_dimensions(&bytes)?;
    fs::write(path, &bytes).ok()?;
    Some(size)
}
fn capture_entry(
    clipboard: &ClipboardContext,
    arboard: &mut Arboard,
) -> Result<ClipboardEntry, String> {
    let id = next_entry_id();
    let timestamp = now_timestamp_ms();

    if clipboard.has(ContentFormat::Files) {
        if let Ok(files) = clipboard.get_files() {
            if !files.is_empty() {
                let name = files
                    .first()
                    .map(|path| file_display_name(path))
                    .unwrap_or_else(|| files[0].clone());
                return Ok(ClipboardEntry {
                    id,
                    kind: "files".to_string(),
                    title: "文件".to_string(),
                    preview: format!("{name} 等 {} 个", files.len()),
                    timestamp_ms: timestamp,
                    text: None,
                    files: Some(files),
                    image_path: None,
                    image_width: None,
                    image_height: None,
                });
            }
        }
    }

    // Fast path: browsers, the Snipping Tool and most apps publish the encoded
    // PNG on the clipboard. Keeping those bytes verbatim is lossless and avoids
    // decoding and re-encoding a large screenshot, which costs about a second
    // and would delay every toast.
    if let Some(image_path) = history_dir().map(|dir| dir.join(format!("{id}.png"))) {
        if let Some(parent) = image_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Some((width, height)) = save_clipboard_png(clipboard, &image_path) {
            return Ok(ClipboardEntry {
                id,
                kind: "image".to_string(),
                title: "图片".to_string(),
                preview: format!("[{width}]x[{height}]"),
                timestamp_ms: timestamp,
                text: None,
                files: None,
                image_path: Some(image_path.to_string_lossy().into_owned()),
                image_width: Some(width),
                image_height: Some(height),
            });
        }
    }

    if let Ok(image) = clipboard.get_image() {
        let (width, height) = image.get_size();
        let image_path = history_dir().map(|dir| dir.join(format!("{id}.png")));
        if let Some(path) = &image_path {
            let _ = fs::create_dir_all(path.parent().unwrap_or(&dir_path(path)));
            image
                .save_to_path(&path.to_string_lossy())
                .map_err(|error| error.to_string())?;
        }
        return Ok(ClipboardEntry {
            id,
            kind: "image".to_string(),
            title: "图片".to_string(),
            preview: format!("[{width}]x[{height}]"),
            timestamp_ms: timestamp,
            text: None,
            files: None,
            image_path: image_path.map(|path| path.to_string_lossy().into_owned()),
            image_width: Some(width),
            image_height: Some(height),
        });
    }

    if let Ok(image) = arboard.get_image() {
        let (width, height) = (image.width as u32, image.height as u32);
        return Ok(ClipboardEntry {
            id,
            kind: "image".to_string(),
            title: "图片".to_string(),
            preview: format!("[{width}]x[{height}]"),
            timestamp_ms: timestamp,
            text: None,
            files: None,
            image_path: None,
            image_width: Some(width),
            image_height: Some(height),
        });
    }

    if let Ok(text) = arboard.get_text() {
        if !text.is_empty() {
            let first_line = text.lines().next().unwrap_or(&text).trim();
            let preview = truncate_chars(first_line, 200);
            return Ok(ClipboardEntry {
                id,
                kind: "text".to_string(),
                title: "文本".to_string(),
                preview,
                timestamp_ms: timestamp,
                text: Some(text),
                files: None,
                image_path: None,
                image_width: None,
                image_height: None,
            });
        }
    }

    Err("clipboard is empty".to_string())
}

fn dir_path(path: &std::path::Path) -> PathBuf {
    path.parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_default()
}

fn read_content(clipboard: &ClipboardContext, arboard: &mut Arboard) -> (&'static str, String) {
    if clipboard.has(ContentFormat::Files) {
        if let Ok(files) = clipboard.get_files() {
            if !files.is_empty() {
                let name = files
                    .first()
                    .map(|path| file_display_name(path))
                    .unwrap_or_else(|| files[0].clone());
                return ("文件", format!("文件：{name} 等 {} 个", files.len()));
            }
        }
    }

    if let Ok(image) = clipboard.get_image() {
        let (width, height) = image.get_size();
        return ("图片", format!("[{width}]x[{height}]"));
    }

    if let Ok(image) = arboard.get_image() {
        let (width, height) = (image.width, image.height);
        return ("图片", format!("[{width}]x[{height}]"));
    }

    let image_hint = clipboard.has(ContentFormat::Image)
        || clipboard
            .available_formats()
            .map(|formats| {
                formats.iter().any(|format| {
                    let format = format.to_ascii_uppercase();
                    format == "PNG"
                        || format == "CF_BITMAP"
                        || format == "CF_DIB"
                        || format == "CF_DIBV5"
                })
            })
            .unwrap_or(false);
    if image_hint {
        return ("图片", "（无法读取尺寸）".to_string());
    }

    if let Ok(text) = arboard.get_text() {
        if text.is_empty() {
            return ("文本", "（空）".to_string());
        }
        return ("文本", truncate_chars(&text, 200));
    }

    ("剪贴板", "（空）".to_string())
}

fn file_display_name(path: &str) -> String {
    let trimmed = path.strip_prefix("file://").unwrap_or(path);
    PathBuf::from(trimmed)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| trimmed.to_string())
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(limit).collect();
        format!("{truncated}…")
    }
}

fn main() {
    let config: SharedConfig = Arc::new(RwLock::new(Config::load_or_create()));
    let history = load_history();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if let Err(error) = show_quick_window(app) {
                            eprintln!("failed to show quick paste: {error}");
                        }
                    }
                })
                .build(),
        )
        .manage(AppState {
            config: config.clone(),
            history: history.clone(),
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == "main" || window.label() == "quick" {
                    api.prevent_close();
                    let _ = window.hide();

                    #[cfg(target_os = "macos")]
                    if window.label() == "main" {
                        let _ = window
                            .app_handle()
                            .set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                }
            }
            WindowEvent::Moved(_) => {
                if window.label() == "quick" {
                    constrain_quick_window(window);
                    if quick_dragging() {
                        QUICK_DRAGGING_UNTIL.store(now_millis() + 1_000, Ordering::Release);
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                if window.label() == "quick" {
                    if *focused {
                        QUICK_WAS_FOCUSED.store(true, Ordering::Relaxed);
                    } else if QUICK_WAS_FOCUSED.load(Ordering::Relaxed)
                        && !QUICK_PASTE_IN_PROGRESS.load(Ordering::Relaxed)
                    {
                        // The monitor is the continuous safety net; this event
                        // path closes the panel immediately on focus loss.
                        if let Ok(hwnd) = window.hwnd() {
                            clear_stale_quick_drag(hwnd.0 as isize);
                        }
                        if !quick_dragging() {
                            let _ = window.hide();
                        }
                    }
                }
            }
            _ => {}
        })
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let startup_config = config.read().map_err(|_| "settings lock poisoned")?.clone();
            apply_autostart(app.handle(), &startup_config)?;
            if let Err(error) = apply_quick_hotkey(app.handle(), &startup_config.quick_hotkey) {
                eprintln!(
                    "failed to register quick-paste hotkey {}: {error}",
                    startup_config.quick_hotkey
                );
            }

            let tray_menu = MenuBuilder::new(app)
                .text("main-menu", "主菜单")
                .text("history-menu", "剪切板历史")
                .text("quit", "退出")
                .build()?;

            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("default tray icon is unavailable")?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(icon)
                .tooltip("Ctrl-CCCC")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event: MenuEvent| match event.id().as_ref() {
                    "main-menu" => show_main_window(app),
                    "history-menu" => {
                        if let Err(error) = show_history_window(app) {
                            eprintln!("failed to open history: {error}");
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if let Err(error) = run_clipboard_watcher(handle, config, history) {
                    eprintln!("clipboard watcher stopped: {error}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            copy_history,
            get_history,
            get_history_image,
            get_settings,
            hide_quick_paste,
            hide_toast,
            open_history,
            open_quick_paste,
            place_toast,
            quick_paste,
            save_settings,
            clear_quick_hotkey,
            set_quick_hotkey_capture,
            set_quick_paste_dragging,
            begin_quick_paste_drag
        ])
        .run(tauri::generate_context!())
        .expect("Ctrl-CCCC exited unexpectedly");
}
