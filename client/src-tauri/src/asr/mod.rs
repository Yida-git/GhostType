mod websocket;

use async_trait::async_trait;
use tokio::sync::mpsc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AsrContext {
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub window_title: String,
}

/// ASR 事件（为未来流式识别预留）
#[derive(Debug, Clone)]
pub enum AsrEvent {
    Partial { text: String },
    Final { text: String },
    Error { message: String },
}

#[async_trait]
pub trait AsrEngine: Send {
    async fn start(&mut self, trace_id: String, sample_rate: u32, context: AsrContext) -> anyhow::Result<()>;
    async fn feed_audio(&mut self, pcm: &[i16]) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<String>;

    fn events(&mut self) -> &mut mpsc::Receiver<AsrEvent>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AsrConfig {
    /// 系统原生 ASR（不同平台使用不同实现）
    Native,
    /// 云端 ASR（不同厂商）
    Cloud {
        provider: CloudProvider,
        api_key: String,
        #[serde(default)]
        region: Option<String>,
    },
    /// 自建服务端（WebSocket）
    #[serde(rename = "websocket", alias = "web_socket")]
    WebSocket { endpoint: String },
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self::WebSocket {
            endpoint: default_websocket_endpoint(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    Deepgram,
    Xfyun,
    Aliyun,
}

pub fn default_websocket_endpoint() -> String {
    "ws://127.0.0.1:8000/ws".to_string()
}

pub fn create_engine(config: &AsrConfig) -> anyhow::Result<Box<dyn AsrEngine>> {
    match config {
        AsrConfig::WebSocket { endpoint } => Ok(Box::new(websocket::WebSocketAsrEngine::new(endpoint.clone()))),
        AsrConfig::Native => anyhow::bail!("系统原生 ASR 尚未实现"),
        AsrConfig::Cloud { provider, .. } => anyhow::bail!("云端 ASR 尚未实现: {provider:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asr_config_serializes_websocket_tag() {
        let cfg = AsrConfig::WebSocket {
            endpoint: "ws://example/ws".to_string(),
        };
        let value = serde_json::to_value(cfg).expect("serialize");
        assert_eq!(value.get("type").and_then(|v| v.as_str()), Some("websocket"));
    }

    #[test]
    fn asr_config_accepts_legacy_web_socket_tag() {
        let raw = r#"{ "type": "web_socket", "endpoint": "ws://legacy/ws" }"#;
        let cfg = serde_json::from_str::<AsrConfig>(raw).expect("deserialize legacy");
        match cfg {
            AsrConfig::WebSocket { endpoint } => assert_eq!(endpoint, "ws://legacy/ws"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
