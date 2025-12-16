use crate::input::{InjectCommand, Injector};
use crate::TrayController;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientControl {
    Start {
        trace_id: String,
        sample_rate: u32,
        context: ClientContext,
        use_cloud_api: bool,
    },
    Stop {
        #[serde(skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
    },
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
    FastText {
        trace_id: Option<String>,
        content: String,
        is_final: Option<bool>,
    },
    Correction {
        trace_id: Option<String>,
        original_text: String,
        replaced_text: String,
        delete_count: usize,
    },
    Error { trace_id: Option<String>, message: String },
}

#[derive(Debug)]
pub enum NetworkCommand {
    SendControl(ClientControl),
    SendAudio { trace_id: String, seq: u64, bytes: Vec<u8> },
}

#[derive(Clone)]
pub struct NetworkHandle {
    pub tx: mpsc::Sender<NetworkCommand>,
}

pub fn spawn_network(endpoints: Vec<String>, injector: Injector, tray: Arc<TrayController>) -> NetworkHandle {
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
                info!(
                    target: "network",
                    endpoint = %endpoint,
                    "正在连接服务器 | Connecting to server"
                );
                let connect_result = tokio_tungstenite::connect_async(endpoint).await;
                let Ok((ws, _)) = connect_result else {
                    warn!(
                        target: "network",
                        endpoint = %endpoint,
                        "服务器连接失败 | Server connect failed"
                    );
                    tray.set_error();
                    continue;
                };

                info!(
                    target: "network",
                    endpoint = %endpoint,
                    "服务器已连接 | Server connected"
                );
                tray.clear_error();
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
                                NetworkCommand::SendAudio { trace_id, seq, bytes } => {
                                    debug!(
                                        target: "network",
                                        trace_id = %trace_id,
                                        bytes = bytes.len(),
                                        seq = seq,
                                        "音频包已发送 | Audio packet sent"
                                    );
                                    Message::Binary(bytes)
                                }
                            };

                            if write.send(msg).await.is_err() {
                                break;
                            }
                        }
                        incoming = read.next() => {
                            let Some(incoming) = incoming else { break; };
                            let Ok(incoming) = incoming else { break; };

                            match incoming {
                                Message::Text(text) => handle_server_text(&text, &injector, &tray).await,
                                Message::Close(_) => break,
                                _ => {}
                            }
                        }
                    }
                }

                warn!(
                    target: "network",
                    endpoint = %endpoint,
                    "服务器连接断开 | Server disconnected"
                );
                tray.set_error();
            }

            while rx.try_recv().is_ok() {}

            info!(
                target: "network",
                delay_ms = backoff_ms,
                "正在重连 | Reconnecting"
            );
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(5000);
        }
    });

    NetworkHandle { tx }
}

async fn handle_server_text(text: &str, injector: &Injector, tray: &Arc<TrayController>) {
    let Ok(event) = serde_json::from_str::<ServerEvent>(text) else {
        return;
    };

    match event {
        ServerEvent::Pong => {}
        ServerEvent::FastText {
            trace_id,
            content,
            is_final,
        } => {
            let _ = is_final;
            let trace_id = trace_id.filter(|v| !v.is_empty());
            if let Some(tid) = trace_id.as_deref() {
                info!(
                    target: "network",
                    trace_id = %tid,
                    text_len = content.len(),
                    "收到识别结果 | Recognition result received"
                );
            } else {
                info!(target: "network", text_len = content.len(), "收到识别结果 | Recognition result received");
            }
            let _ = injector
                .tx
                .send(InjectCommand::TypeText {
                    trace_id,
                    text: content,
                })
                .await;
            tray.set_idle();
        }
        ServerEvent::Correction {
            trace_id,
            original_text,
            replaced_text,
            delete_count,
        } => {
            let _ = original_text;
            let _ = injector
                .tx
                .send(InjectCommand::Backspace {
                    trace_id: trace_id.clone(),
                    count: delete_count,
                })
                .await;
            let _ = injector
                .tx
                .send(InjectCommand::TypeText {
                    trace_id,
                    text: replaced_text,
                })
                .await;
            tray.set_idle();
        }
        ServerEvent::Error { trace_id, message } => {
            tray.set_error();
            let trace_id = trace_id.filter(|v| !v.is_empty());
            if let Some(tid) = trace_id.as_deref() {
                error!(
                    target: "network",
                    trace_id = %tid,
                    message = %message,
                    "服务端错误 | Server error"
                );
                return;
            }
            error!(
                target: "network",
                message = %message,
                "服务端错误 | Server error"
            );
        }
    }
}
