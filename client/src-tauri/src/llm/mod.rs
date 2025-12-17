mod ollama;
mod openai_compat;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub fn default_timeout_ms() -> u64 {
    3000
}

#[derive(Debug, Clone)]
pub struct CorrectionResult {
    pub original: String,
    pub corrected: String,
    pub changed: bool,
    pub latency_ms: u64,
}

#[async_trait]
pub trait LlmEngine: Send + Sync {
    async fn correct(&self, text: &str) -> anyhow::Result<CorrectionResult>;
    async fn health_check(&self) -> bool;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmConfig {
    /// 禁用 LLM 校正（只输出 ASR）
    Disabled,
    /// OpenAI 兼容接口（OpenAI/通义千问/DeepSeek/月之暗面等）
    #[serde(rename = "openai_compat", alias = "open_ai_compat")]
    OpenAiCompat {
        endpoint: String,
        api_key: String,
        model: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// 本地 Ollama
    Ollama {
        endpoint: String,
        model: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self::Disabled
    }
}

pub fn create_engine(config: &LlmConfig) -> anyhow::Result<Box<dyn LlmEngine>> {
    match config {
        LlmConfig::Disabled => Ok(Box::new(DisabledEngine)),
        LlmConfig::OpenAiCompat {
            endpoint,
            api_key,
            model,
            timeout_ms,
        } => Ok(Box::new(openai_compat::OpenAiCompatEngine::new(
            endpoint.clone(),
            api_key.clone(),
            model.clone(),
            *timeout_ms,
        )?)),
        LlmConfig::Ollama {
            endpoint,
            model,
            timeout_ms,
        } => Ok(Box::new(ollama::OllamaEngine::new(
            endpoint.clone(),
            model.clone(),
            *timeout_ms,
        )?)),
    }
}

struct DisabledEngine;

#[async_trait]
impl LlmEngine for DisabledEngine {
    async fn correct(&self, text: &str) -> anyhow::Result<CorrectionResult> {
        Ok(CorrectionResult {
            original: text.to_string(),
            corrected: text.to_string(),
            changed: false,
            latency_ms: 0,
        })
    }

    async fn health_check(&self) -> bool {
        true
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_config_serializes_openai_compat_tag() {
        let cfg = LlmConfig::OpenAiCompat {
            endpoint: "https://example/v1".to_string(),
            api_key: "k".to_string(),
            model: "m".to_string(),
            timeout_ms: 3000,
        };
        let value = serde_json::to_value(cfg).expect("serialize");
        assert_eq!(value.get("type").and_then(|v| v.as_str()), Some("openai_compat"));
    }

    #[test]
    fn llm_config_accepts_legacy_open_ai_compat_tag() {
        let raw = r#"
        {
          "type": "open_ai_compat",
          "endpoint": "https://legacy/v1",
          "api_key": "k",
          "model": "m",
          "timeout_ms": 3000
        }
        "#;
        let cfg = serde_json::from_str::<LlmConfig>(raw).expect("deserialize legacy");
        match cfg {
            LlmConfig::OpenAiCompat { endpoint, .. } => assert_eq!(endpoint, "https://legacy/v1"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
