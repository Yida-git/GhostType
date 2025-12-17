#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod asr;
mod audio;
mod config;
mod input;
mod llm;
mod logging;
mod opus;
mod pipeline;
mod platform;

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

#[derive(serde::Serialize)]
struct RuntimeInfo {
    os: String,
    arch: String,
}

#[derive(serde::Serialize)]
struct PermissionStatus {
    accessibility: bool,
    microphone: bool,
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

#[tauri::command]
fn get_runtime_info() -> RuntimeInfo {
    RuntimeInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

#[tauri::command]
fn list_audio_devices() -> Result<Vec<audio::InputDeviceInfo>, String> {
    audio::list_input_devices().map_err(|err| err.to_string())
}

#[tauri::command]
fn check_permissions(state: tauri::State<'_, Arc<app_state::AppState>>) -> PermissionStatus {
    let accessibility = if cfg!(target_os = "macos") {
        platform::ensure_accessibility(false)
    } else {
        true
    };

    let microphone = audio::check_microphone_access(state.audio_device.as_deref());

    PermissionStatus {
        accessibility,
        microphone,
    }
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    platform::open_accessibility_settings()
}

#[tauri::command]
fn open_microphone_settings() -> Result<(), String> {
    platform::open_microphone_settings()
}

#[tauri::command]
fn open_sound_settings() -> Result<(), String> {
    platform::open_sound_settings()
}

#[tauri::command]
async fn test_server_connection(endpoint: String) -> Result<bool, String> {
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    let endpoint = endpoint.trim().to_string();
    if endpoint.is_empty() {
        return Err("服务器地址为空 | Endpoint is empty".to_string());
    }

    let connect_result = tokio::time::timeout(Duration::from_secs(3), tokio_tungstenite::connect_async(&endpoint))
        .await
        .map_err(|_| "连接超时 | Connect timeout".to_string())?;

    let (ws, _) = connect_result.map_err(|err| err.to_string())?;
    let (mut write, mut read) = ws.split();

    let payload = serde_json::json!({ "type": "ping" }).to_string();
    write
        .send(Message::Text(payload))
        .await
        .map_err(|err| err.to_string())?;

    let incoming = tokio::time::timeout(Duration::from_secs(3), read.next())
        .await
        .map_err(|_| "等待响应超时 | Wait timeout".to_string())?;

    let Some(incoming) = incoming else {
        return Ok(false);
    };
    let Ok(incoming) = incoming else {
        return Ok(false);
    };

    let Message::Text(text) = incoming else {
        return Ok(false);
    };

    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(false);
    };
    Ok(value.get("type").and_then(|v| v.as_str()) == Some("pong"))
}

#[tauri::command]
async fn test_llm_health(llm_config: llm::LlmConfig) -> Result<bool, String> {
    let engine = llm::create_engine(&llm_config).map_err(|err| err.to_string())?;
    Ok(engine.health_check().await)
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
        .invoke_handler(tauri::generate_handler![
            load_client_config,
            save_client_config,
            get_runtime_info,
            list_audio_devices,
            check_permissions,
            open_accessibility_settings,
            open_microphone_settings,
            open_sound_settings,
            test_server_connection,
            test_llm_health
        ])
        .setup(|app| {
            let (config, config_path) = config::load_with_path();
            let hotkey = config.hotkey.clone();
            let audio_device = config.audio_device.clone();

            let server_endpoints = match &config.asr {
                asr::AsrConfig::WebSocket { endpoint } => vec![endpoint.clone()],
                _ => vec![asr::default_websocket_endpoint()],
            };
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
                audio_device = audio_device.as_deref().unwrap_or("(default)"),
                use_cloud_api = config.use_cloud_api,
                asr = %format!("{:?}", config.asr),
                llm = %format!("{:?}", config.llm),
                "配置已加载 | Config loaded"
            );
            setup_tray(app)?;
            let tray = Arc::new(TrayController::new(app.handle().clone()));
            tray.set_idle();

            let injector = input::spawn_injector();
            let pipeline = pipeline::Pipeline::new(&config.asr, &config.llm, injector.clone()).unwrap_or_else(|err| {
                tracing::error!(
                    target: "pipeline",
                    error = %err,
                    "Pipeline 初始化失败，回退默认配置 | Pipeline init failed, falling back to defaults"
                );
                pipeline::Pipeline::new(&asr::AsrConfig::default(), &llm::LlmConfig::default(), injector.clone())
                    .expect("pipeline fallback")
            });

            let state = Arc::new(app_state::AppState::new(pipeline, audio_device.clone()));

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

            // 如果权限缺失，自动弹出窗口提示（否则托盘模式下用户可能不知道）。
            let accessibility_ok = if cfg!(target_os = "macos") {
                platform::ensure_accessibility(false)
            } else {
                true
            };
            let microphone_ok = audio::check_microphone_access(audio_device.as_deref());
            if !accessibility_ok || !microphone_ok {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

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
    let (recorder, mut pcm_rx) = match audio::start_audio(trace_id.clone(), state.audio_device.clone()) {
        Ok(parts) => parts,
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
    let session_gen = {
        let mut pipeline = state.pipeline.lock().await;
        match pipeline.start(trace_id.clone(), sample_rate, context).await {
            Ok(gen) => gen,
            Err(err) => {
                error!(
                    target: "pipeline",
                    trace_id = trace_id.as_str(),
                    error = %err,
                    "ASR 会话启动失败 | ASR session start failed"
                );
                recorder.stop();
                tray.set_error();
                return;
            }
        }
    };

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

    *state.session_gen.lock().expect("session gen lock") = Some(session_gen);

    let state_for_task = state.clone();
    let task = tauri::async_runtime::spawn(async move {
        while let Some(frame) = pcm_rx.recv().await {
            let mut pipeline = state_for_task.pipeline.lock().await;
            if let Err(err) = pipeline.feed_audio(&frame).await {
                tracing::warn!(
                    target: "audio",
                    error = %err,
                    "ASR 音频发送失败 | ASR feed_audio failed"
                );
                break;
            }
        }
    });
    *state.audio_task.lock().expect("audio task lock") = Some(task);
}

async fn handle_stop(state: &Arc<app_state::AppState>, tray: &Arc<TrayController>) {
    let recorder = state.audio.lock().expect("audio lock").take();
    let Some(recorder) = recorder else {
        // 没有正在进行的录音，不发送 Stop
        return;
    };

    let task = state.audio_task.lock().expect("audio task lock").take();
    let session_gen = state.session_gen.lock().expect("session gen lock").take().unwrap_or(0);

    recorder.stop();

    tray.set_processing();

    if let Some(task) = task {
        let _ = task.await;
    }

    let mut pipeline = state.pipeline.lock().await;
    let stop_result = pipeline.stop(session_gen).await;
    match stop_result {
        Ok(()) => tray.set_idle(),
        Err(err) => {
            error!(
                target: "pipeline",
                error = %err,
                "会话处理失败 | Session failed"
            );
            tray.set_error();
        }
    }
}

fn generate_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    const BASE62: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    let mut n = micros;
    let mut out = [b'0'; 6];
    for slot in out.iter_mut().rev() {
        *slot = BASE62[(n % 62) as usize];
        n /= 62;
    }

    String::from_utf8_lossy(&out).to_string()
}

fn get_active_context() -> Option<asr::AsrContext> {
    let ActiveWindow {
        app_name,
        title,
        ..
    } = active_win_pos_rs::get_active_window().ok()?;

    Some(asr::AsrContext {
        app_name,
        window_title: title,
    })
}
