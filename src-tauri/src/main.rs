#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arboard::Clipboard as Arboard;
use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext,
    ContentFormat,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};
use tauri::menu::{MenuBuilder, MenuEvent};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const EVENT: &str = "clipboard://changed";
const CONFIG_FILE: &str = "ctrl-cccc.json";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    position: Corner,
    duration_ms: u64,
    fade_ms: u64,
    autostart: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: Corner::BottomRight,
            duration_ms: 2_200,
            fade_ms: 180,
            autostart: false,
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
    }
}

type SharedConfig = Arc<RwLock<Config>>;

#[derive(Debug, Clone, Serialize)]
struct ToastPayload {
    kind: &'static str,
    message: String,
    duration_ms: u64,
    fade_ms: u64,
}

struct AppState {
    config: SharedConfig,
}

struct ToastWatcher {
    files_clipboard: ClipboardContext,
    text_clipboard: Arboard,
    app: AppHandle,
    config: SharedConfig,
}

impl ClipboardHandler for ToastWatcher {
    fn on_clipboard_change(&mut self) {
        // One short debounce lets multi-format clipboard writers finish before we read.
        thread::sleep(Duration::from_millis(50));
        let config = match self.config.read() {
            Ok(config) => config.clone(),
            Err(_) => return,
        };
        let payload = read_toast_payload(&self.files_clipboard, &mut self.text_clipboard, &config);
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
    Ok(settings)
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

fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn run_clipboard_watcher(
    app: AppHandle,
    config: SharedConfig,
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
    }
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
        return ("图片", format!("图片 [{width}]x[{height}]"));
    }

    if let Ok(image) = arboard.get_image() {
        let (width, height) = (image.width, image.height);
        return ("图片", format!("图片 [{width}]x[{height}]"));
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
        return ("图片", "图片（无法读取尺寸）".to_string());
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

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            config: config.clone(),
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();

                    #[cfg(target_os = "macos")]
                    let _ = window
                        .app_handle()
                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
        })
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let startup_config = config.read().map_err(|_| "settings lock poisoned")?.clone();
            apply_autostart(app.handle(), &startup_config)?;

            let tray_menu = MenuBuilder::new(app)
                .text("main-menu", "主菜单")
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
                if let Err(error) = run_clipboard_watcher(handle, config) {
                    eprintln!("clipboard watcher stopped: {error}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            place_toast,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("Ctrl-CCCC exited unexpectedly");
}
