#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod audio;
mod config;
mod input;
mod logging;
mod network;
mod opus;
mod platform;

use crate::network::{ClientContext, ClientControl, NetworkCommand};
use active_win_pos_rs::ActiveWindow;
use rdev::{EventType, Key};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tauri::Manager;
use tracing::{debug, error, info};

#[derive(Debug)]
enum HotkeyEvent {
    Start,
    Stop,
}

const TRAY_ID: &str = "ghosttype-tray";

#[cfg(target_os = "macos")]
const TRAY_IDLE: &[u8] = include_bytes!("../icons/tray_idle@2x.png");
#[cfg(target_os = "macos")]
const TRAY_RECORDING: &[u8] = include_bytes!("../icons/tray_recording@2x.png");
#[cfg(target_os = "macos")]
const TRAY_PROCESSING: &[u8] = include_bytes!("../icons/tray_processing@2x.png");
#[cfg(target_os = "macos")]
const TRAY_ERROR: &[u8] = include_bytes!("../icons/tray_error@2x.png");

#[cfg(not(target_os = "macos"))]
const TRAY_IDLE: &[u8] = include_bytes!("../icons/tray_idle.png");
#[cfg(not(target_os = "macos"))]
const TRAY_RECORDING: &[u8] = include_bytes!("../icons/tray_recording.png");
#[cfg(not(target_os = "macos"))]
const TRAY_PROCESSING: &[u8] = include_bytes!("../icons/tray_processing.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ERROR: &[u8] = include_bytes!("../icons/tray_error.png");

#[derive(Debug, Clone, Copy)]
enum TrayMode {
    Idle,
    Recording,
    Processing,
}

#[derive(Debug)]
struct TrayControllerState {
    mode: TrayMode,
    error: bool,
}

#[derive(Debug)]
struct TrayController {
    app: tauri::AppHandle,
    state: Mutex<TrayControllerState>,
}

impl TrayController {
    fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            state: Mutex::new(TrayControllerState {
                mode: TrayMode::Idle,
                error: false,
            }),
        }
    }

    fn set_idle(&self) {
        self.set_mode(TrayMode::Idle);
        self.clear_error();
    }

    fn set_recording(&self) {
        self.set_mode(TrayMode::Recording);
        self.clear_error();
    }

    fn set_processing(&self) {
        self.set_mode(TrayMode::Processing);
        self.clear_error();
    }

    fn set_error(&self) {
        let mut guard = self.state.lock().expect("tray state lock");
        guard.error = true;
        drop(guard);
        self.apply();
    }

    fn clear_error(&self) {
        let mut guard = self.state.lock().expect("tray state lock");
        guard.error = false;
        drop(guard);
        self.apply();
    }

    fn set_mode(&self, mode: TrayMode) {
        let mut guard = self.state.lock().expect("tray state lock");
        guard.mode = mode;
        drop(guard);
        self.apply();
    }

    fn apply(&self) {
        let guard = self.state.lock().expect("tray state lock");
        let bytes = match (guard.mode, guard.error) {
            (TrayMode::Recording, _) => TRAY_RECORDING,
            (_, true) => TRAY_ERROR,
            (TrayMode::Processing, false) => TRAY_PROCESSING,
            (TrayMode::Idle, false) => TRAY_IDLE,
        };
        drop(guard);

        let Some(tray) = self.app.tray_by_id(TRAY_ID) else {
            return;
        };

        let icon = tauri::image::Image::from_bytes(bytes).expect("tray icon");
        if let Err(err) = tray.set_icon(Some(icon)) {
            tracing::warn!(target: "tray", error = %err, "tray icon set failed");
        }
    }
}

#[derive(serde::Serialize)]
struct ClientConfigResponse {
    config: config::ClientConfig,
    path: Option<String>,
}

#[tauri::command]
fn load_client_config() -> ClientConfigResponse {
    let (config, path) = config::load_with_path();
    ClientConfigResponse {
        config,
        path: path.map(|p| p.display().to_string()),
    }
}

#[tauri::command]
fn save_client_config(config: config::ClientConfig) -> Result<ClientConfigResponse, String> {
    let (_, path) = config::load_with_path();
    let saved = config::save_to_path(&config, path).map_err(|err| err.to_string())?;
    Ok(ClientConfigResponse {
        config,
        path: Some(saved.display().to_string()),
    })
}

fn main() {
    logging::init();

    info!(
        target: "app",
        version = env!("CARGO_PKG_VERSION"),
        platform = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "应用启动 | App starting"
    );

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![load_client_config, save_client_config])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                if !platform::ensure_accessibility(true) {
                    tracing::warn!(
                        target: "app",
                        "macOS 未授予辅助功能权限：全局热键监听/键盘注入可能无效 | Accessibility permission missing on macOS"
                    );
                }
            }

            let (config, config_path) = config::load_with_path();
            let config::ClientConfig {
                server_endpoints,
                use_cloud_api,
                hotkey,
            } = config;
            let config_path = config_path
                .map(|p| p.display().to_string())
                .unwrap_or_default();

            #[cfg(target_os = "macos")]
            {
                if matches!(parse_hotkey(&hotkey), Key::CapsLock) {
                    tracing::warn!(
                        target: "config",
                        hotkey = %hotkey,
                        "macOS 上使用 CapsLock 会切换系统大写锁定，建议改为 F8 | CapsLock toggles system caps on macOS; prefer F8"
                    );
                }
            }

            info!(
                target: "config",
                path = config_path.as_str(),
                hotkey = %hotkey,
                server = %server_endpoints.get(0).map(String::as_str).unwrap_or(""),
                "配置已加载 | Config loaded"
            );
            setup_tray(app)?;
            let tray = Arc::new(TrayController::new(app.handle().clone()));
            tray.set_idle();

            let injector = input::spawn_injector();
            let network = network::spawn_network(server_endpoints, injector, tray.clone());

            let state = Arc::new(app_state::AppState::new(network, use_cloud_api));

            let (hk_tx, mut hk_rx) = mpsc::channel::<HotkeyEvent>(32);
            spawn_hotkey_listener(hk_tx, hotkey);

            let state_for_task = state.clone();
            let tray_for_task = tray.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(evt) = hk_rx.recv().await {
                    match evt {
                        HotkeyEvent::Start => {
                            handle_start(&state_for_task, &tray_for_task).await;
                        }
                        HotkeyEvent::Stop => {
                            handle_stop(&state_for_task, &tray_for_task).await;
                        }
                    }
                }
            });

            app.manage(state);
            info!(target: "tray", "托盘已就绪 | Tray ready");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::image::Image::from_bytes(TRAY_IDLE).expect("tray icon"))
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "hide" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn spawn_hotkey_listener(tx: mpsc::Sender<HotkeyEvent>, hotkey: String) {
    std::thread::spawn(move || {
        let hotkey = parse_hotkey(&hotkey);
        info!(
            target: "hotkey",
            key = ?hotkey,
            "热键监听器已启动 | Hotkey listener started"
        );
        let listen_result = rdev::listen(move |event| match event.event_type {
            EventType::KeyPress(key) if key == hotkey => {
                debug!(
                    target: "hotkey",
                    action = "press",
                    key = ?key,
                    "热键事件 | Hotkey event"
                );
                let _ = tx.blocking_send(HotkeyEvent::Start);
            }
            EventType::KeyRelease(key) if key == hotkey => {
                debug!(
                    target: "hotkey",
                    action = "release",
                    key = ?key,
                    "热键事件 | Hotkey event"
                );
                let _ = tx.blocking_send(HotkeyEvent::Stop);
            }
            _ => {}
        });

        if let Err(err) = listen_result {
            error!(
                target: "hotkey",
                error = %format!("{err:?}"),
                "热键监听器启动失败 | Hotkey listener failed"
            );
        }
    });
}

fn parse_hotkey(raw: &str) -> Key {
    match raw.trim().to_ascii_lowercase().as_str() {
        // === 推荐热键 (业界已验证) ===
        "capslock" | "caps_lock" | "caps lock" | "caps" => Key::CapsLock,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,

        // === 备选热键 ===
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "right_shift" | "right shift" | "rshift" => Key::ShiftRight,
        "left_shift" | "left shift" | "lshift" => Key::ShiftLeft,

        _ => {
            tracing::warn!(
                target: "config",
                hotkey = %raw,
                "未知热键，使用默认 | Unknown hotkey, using default"
            );
            if cfg!(target_os = "macos") {
                Key::F8
            } else {
                Key::CapsLock
            }
        }
    }
}

async fn handle_start(state: &Arc<app_state::AppState>, tray: &Arc<TrayController>) {
    {
        let guard = state.audio.lock().expect("audio lock");
        if guard.is_some() {
            return;
        }
    }

    let trace_id = generate_trace_id();
    let context = get_active_context().unwrap_or_default();
    let recorder = match audio::start_audio(state.tx.clone(), trace_id.clone()) {
        Ok(recorder) => recorder,
        Err(err) => {
            error!(
                target: "audio",
                error = %err,
                "麦克风访问失败 | Microphone access failed"
            );
            tray.set_error();
            return;
        }
    };

    let sample_rate = recorder.sample_rate;
    {
        let mut guard = state.audio.lock().expect("audio lock");
        if guard.is_some() {
            // 竞态：另一个 Start 已经抢先，停止当前 recorder 避免泄漏
            drop(guard);
            recorder.stop();
            return;
        }
        *guard = Some(recorder);
    }

    tray.set_recording();

    let _ = state
        .tx
        .send(NetworkCommand::SendControl(ClientControl::Start {
            trace_id,
            sample_rate,
            context,
            use_cloud_api: state.use_cloud_api,
        }))
        .await;
}

async fn handle_stop(state: &Arc<app_state::AppState>, tray: &Arc<TrayController>) {
    let recorder = state.audio.lock().expect("audio lock").take();
    let Some(recorder) = recorder else {
        // 没有正在进行的录音，不发送 Stop
        return;
    };

    let trace_id = recorder.trace_id.clone();
    recorder.stop();

    tray.set_processing();

    let _ = state
        .tx
        .send(NetworkCommand::SendControl(ClientControl::Stop {
            trace_id: Some(trace_id),
        }))
        .await;
}

fn generate_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    const BASE62: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    let mut n = micros as u32;
    let mut out = [b'0'; 6];
    for slot in out.iter_mut().rev() {
        *slot = BASE62[(n % 62) as usize];
        n /= 62;
    }

    String::from_utf8_lossy(&out).to_string()
}

fn get_active_context() -> Option<ClientContext> {
    let ActiveWindow {
        app_name,
        title,
        ..
    } = active_win_pos_rs::get_active_window().ok()?;

    Some(ClientContext {
        app_name,
        window_title: title,
    })
}
