#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod audio;
mod config;
mod input;
mod network;

use crate::network::{ClientContext, ClientControl, NetworkCommand};
use active_win::ActiveWindow;
use rdev::{EventType, Key};
use std::sync::Arc;
use tokio::sync::mpsc;
use tauri::Manager;

#[derive(Debug)]
enum HotkeyEvent {
    Start,
    Stop,
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let config::ClientConfig {
                server_endpoints,
                use_cloud_api,
                hotkey,
            } = config::load();
            let injector = input::spawn_injector();
            let network = network::spawn_network(server_endpoints, injector.clone());

            let state = Arc::new(app_state::AppState::new(network.clone(), injector, use_cloud_api));

            let (hk_tx, mut hk_rx) = mpsc::channel::<HotkeyEvent>(32);
            spawn_hotkey_listener(hk_tx, hotkey);

            let state_for_task = state.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(evt) = hk_rx.recv().await {
                    match evt {
                        HotkeyEvent::Start => {
                            handle_start(&state_for_task).await;
                        }
                        HotkeyEvent::Stop => {
                            handle_stop(&state_for_task).await;
                        }
                    }
                }
            });

            app.manage(state);
            setup_tray(app)?;
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

    TrayIconBuilder::new()
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
        let _ = rdev::listen(move |event| match event.event_type {
            EventType::KeyPress(key) if key == hotkey => {
                let _ = tx.blocking_send(HotkeyEvent::Start);
            }
            EventType::KeyRelease(key) if key == hotkey => {
                let _ = tx.blocking_send(HotkeyEvent::Stop);
            }
            _ => {}
        });
    });
}

fn parse_hotkey(raw: &str) -> Key {
    match raw.trim().to_ascii_lowercase().as_str() {
        "capslock" | "caps_lock" => Key::CapsLock,
        "f8" => Key::F8,
        _ => Key::CapsLock,
    }
}

async fn handle_start(state: &Arc<app_state::AppState>) {
    let mut guard = state.audio.lock().expect("audio lock");
    if guard.is_some() {
        return;
    }

    let context = get_active_context().unwrap_or_default();
    let recorder = match audio::start_audio(state.tx.clone()) {
        Ok(recorder) => recorder,
        Err(err) => {
            eprintln!("start_audio failed: {err:#}");
            return;
        }
    };

    let sample_rate = recorder.sample_rate;
    *guard = Some(recorder);

    let _ = state
        .tx
        .send(NetworkCommand::SendControl(ClientControl::Start {
            sample_rate,
            context,
            use_cloud_api: state.use_cloud_api,
        }))
        .await;
}

async fn handle_stop(state: &Arc<app_state::AppState>) {
    let recorder = state.audio.lock().expect("audio lock").take();
    if let Some(recorder) = recorder {
        recorder.stop();
    }

    let _ = state
        .tx
        .send(NetworkCommand::SendControl(ClientControl::Stop))
        .await;
}

fn get_active_context() -> Option<ClientContext> {
    let ActiveWindow {
        app_name,
        title,
        ..
    } = active_win::get_active_window().ok()?;

    Some(ClientContext {
        app_name,
        window_title: title,
    })
}
