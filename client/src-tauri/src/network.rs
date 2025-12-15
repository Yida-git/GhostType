use crate::input::{InjectCommand, Injector};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientControl {
    Ping,
    Start {
        sample_rate: u32,
        context: ClientContext,
        use_cloud_api: bool,
    },
    Stop,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClientContext {
    pub app_name: String,
    pub window_title: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent {
    Pong,
    FastText { content: String, is_final: Option<bool> },
    Correction {
        original_text: String,
        replaced_text: String,
        delete_count: usize,
    },
    Error { message: String },
}

#[derive(Debug)]
pub enum NetworkCommand {
    SendControl(ClientControl),
    SendAudio(Vec<u8>),
}

#[derive(Clone)]
pub struct NetworkHandle {
    pub tx: mpsc::Sender<NetworkCommand>,
}

pub fn spawn_network(endpoints: Vec<String>, injector: Injector) -> NetworkHandle {
    let (tx, mut rx) = mpsc::channel::<NetworkCommand>(1024);

    tauri::async_runtime::spawn(async move {
        let endpoints = if endpoints.is_empty() {
            vec!["ws://127.0.0.1:8000/ws".to_string()]
        } else {
            endpoints
        };

        let mut backoff_ms: u64 = 200;
        loop {
            for endpoint in &endpoints {
                let connect_result = tokio_tungstenite::connect_async(endpoint).await;
                let Ok((ws, _)) = connect_result else {
                    continue;
                };

                backoff_ms = 200;
                let (mut write, mut read) = ws.split();

                loop {
                    tokio::select! {
                        Some(cmd) = rx.recv() => {
                            let msg = match cmd {
                                NetworkCommand::SendControl(control) => {
                                    let Ok(text) = serde_json::to_string(&control) else { continue; };
                                    Message::Text(text)
                                }
                                NetworkCommand::SendAudio(bytes) => Message::Binary(bytes),
                            };

                            if write.send(msg).await.is_err() {
                                break;
                            }
                        }
                        incoming = read.next() => {
                            let Some(incoming) = incoming else { break; };
                            let Ok(incoming) = incoming else { break; };

                            match incoming {
                                Message::Text(text) => handle_server_text(&text, &injector).await,
                                Message::Close(_) => break,
                                _ => {}
                            }
                        }
                    }
                }
            }

            while rx.try_recv().is_ok() {}

            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(5000);
        }
    });

    NetworkHandle { tx }
}

async fn handle_server_text(text: &str, injector: &Injector) {
    let Ok(event) = serde_json::from_str::<ServerEvent>(text) else {
        return;
    };

    match event {
        ServerEvent::Pong => {}
        ServerEvent::FastText { content, .. } => {
            let _ = injector.tx.send(InjectCommand::TypeText(content)).await;
        }
        ServerEvent::Correction {
            replaced_text,
            delete_count,
            ..
        } => {
            let _ = injector.tx.send(InjectCommand::Backspace(delete_count)).await;
            let _ = injector.tx.send(InjectCommand::TypeText(replaced_text)).await;
        }
        ServerEvent::Error { message } => {
            eprintln!("server error: {message}");
        }
    }
}

