use anyhow::Context as _;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::asr::{AsrContext, AsrEngine, AsrEvent};
use crate::opus::OpusEncoder;

pub struct WebSocketAsrEngine {
    endpoint: String,
    ws: Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
    trace_id: Option<String>,
    sample_rate: u32,
    encoder: Option<OpusEncoder>,
    frame_size: usize,
    pcm_buf: Vec<i16>,
    out_buf: Vec<u8>,
    tx: mpsc::Sender<AsrEvent>,
    rx: mpsc::Receiver<AsrEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct ClientContextPayload {
    app_name: String,
    window_title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientControlPayload {
    Start {
        trace_id: String,
        sample_rate: u32,
        context: ClientContextPayload,
        use_cloud_api: bool,
    },
    Stop {
        #[serde(skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
    },
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEventPayload {
    Pong,
    FastText {
        trace_id: Option<String>,
        content: String,
        is_final: Option<bool>,
    },
    Error {
        trace_id: Option<String>,
        message: String,
    },
}

impl WebSocketAsrEngine {
    pub fn new(endpoint: String) -> Self {
        let (tx, rx) = mpsc::channel::<AsrEvent>(64);
        Self {
            endpoint,
            ws: None,
            trace_id: None,
            sample_rate: 0,
            encoder: None,
            frame_size: 0,
            pcm_buf: Vec::new(),
            out_buf: vec![0u8; 4096],
            tx,
            rx,
        }
    }

    async fn disconnect(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close(None).await;
        }
    }

    async fn ensure_connected(&mut self) -> anyhow::Result<()> {
        if self.ws.is_some() {
            return Ok(());
        }

        let (ws, _) = tokio_tungstenite::connect_async(&self.endpoint)
            .await
            .context("connect websocket")?;
        self.ws = Some(ws);
        Ok(())
    }

    async fn send_text(&mut self, text: String) -> anyhow::Result<()> {
        let Some(ws) = self.ws.as_mut() else {
            anyhow::bail!("websocket not connected");
        };
        ws.send(Message::Text(text)).await.context("ws send text")?;
        Ok(())
    }

    async fn send_binary(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let Some(ws) = self.ws.as_mut() else {
            anyhow::bail!("websocket not connected");
        };
        ws.send(Message::Binary(bytes))
            .await
            .context("ws send binary")?;
        Ok(())
    }

    async fn recv_event(&mut self) -> anyhow::Result<ServerEventPayload> {
        let Some(ws) = self.ws.as_mut() else {
            anyhow::bail!("websocket not connected");
        };

        loop {
            let Some(msg) = ws.next().await else {
                anyhow::bail!("websocket closed");
            };
            let msg = msg.context("ws recv")?;
            match msg {
                Message::Text(text) => {
                    if let Ok(event) = serde_json::from_str::<ServerEventPayload>(&text) {
                        return Ok(event);
                    }
                    continue;
                }
                Message::Close(_) => anyhow::bail!("websocket closed"),
                _ => continue,
            }
        }
    }

    fn push_pcm_and_drain_frames(&mut self, pcm: &[i16]) -> Vec<Vec<u8>> {
        self.pcm_buf.extend_from_slice(pcm);

        let mut out_packets = Vec::new();
        if self.frame_size == 0 {
            return out_packets;
        }

        while self.pcm_buf.len() >= self.frame_size {
            let frame: Vec<i16> = self.pcm_buf.drain(..self.frame_size).collect();
            let Some(encoder) = self.encoder.as_mut() else {
                break;
            };
            let Ok(len) = encoder.encode(&frame, &mut self.out_buf) else {
                continue;
            };
            if len == 0 {
                continue;
            }
            out_packets.push(self.out_buf[..len].to_vec());
        }

        out_packets
    }
}

#[async_trait]
impl AsrEngine for WebSocketAsrEngine {
    async fn start(&mut self, trace_id: String, sample_rate: u32, context: AsrContext) -> anyhow::Result<()> {
        // 为了避免跨会话残留消息导致混淆，每次会话都重新建立连接。
        self.disconnect().await;
        self.ensure_connected().await?;

        self.trace_id = Some(trace_id.clone());
        self.sample_rate = sample_rate;
        self.encoder = Some(OpusEncoder::new(sample_rate)?);
        self.frame_size = (sample_rate / 50) as usize;
        self.pcm_buf.clear();

        let payload = ClientControlPayload::Start {
            trace_id,
            sample_rate,
            context: ClientContextPayload {
                app_name: context.app_name,
                window_title: context.window_title,
            },
            use_cloud_api: false,
        };
        let text = serde_json::to_string(&payload).context("serialize start payload")?;
        self.send_text(text).await?;
        Ok(())
    }

    async fn feed_audio(&mut self, pcm: &[i16]) -> anyhow::Result<()> {
        let packets = self.push_pcm_and_drain_frames(pcm);
        for pkt in packets {
            self.send_binary(pkt).await?;
        }
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<String> {
        let trace_id = self.trace_id.clone();

        let payload = ClientControlPayload::Stop { trace_id };
        let text = serde_json::to_string(&payload).context("serialize stop payload")?;
        self.send_text(text).await?;

        loop {
            let event = self.recv_event().await?;
            match event {
                ServerEventPayload::Pong => continue,
                ServerEventPayload::FastText { trace_id, content, .. } => {
                    if let (Some(expected), Some(got)) = (self.trace_id.as_deref(), trace_id.as_deref()) {
                        if got != expected {
                            continue;
                        }
                    }
                    let _ = self.tx.try_send(AsrEvent::Final { text: content.clone() });
                    self.trace_id = None;
                    self.encoder = None;
                    self.frame_size = 0;
                    self.pcm_buf.clear();
                    self.disconnect().await;
                    return Ok(content);
                }
                ServerEventPayload::Error { trace_id, message } => {
                    if let (Some(expected), Some(got)) = (self.trace_id.as_deref(), trace_id.as_deref()) {
                        if got != expected {
                            continue;
                        }
                    }
                    let _ = self.tx.try_send(AsrEvent::Error {
                        message: message.clone(),
                    });
                    self.trace_id = None;
                    self.encoder = None;
                    self.frame_size = 0;
                    self.pcm_buf.clear();
                    self.disconnect().await;
                    anyhow::bail!(message);
                }
            }
        }
    }

    fn events(&mut self) -> &mut mpsc::Receiver<AsrEvent> {
        &mut self.rx
    }
}
