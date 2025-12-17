use anyhow::Context as _;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::llm::{elapsed_ms, CorrectionResult, LlmEngine};

pub struct OllamaEngine {
    client: Client,
    endpoint: String,
    model: String,
    timeout: Duration,
}

#[derive(Debug, Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    response: String,
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    #[serde(default)]
    version: String,
}

#[async_trait]
impl LlmEngine for OllamaEngine {
    async fn correct(&self, text: &str) -> anyhow::Result<CorrectionResult> {
        let started = Instant::now();
        let input = text.trim();
        if input.is_empty() {
            return Ok(CorrectionResult {
                original: text.to_string(),
                corrected: text.to_string(),
                changed: false,
                latency_ms: 0,
            });
        }

        let url = format!("{}/api/generate", self.endpoint.trim_end_matches('/'));
        let prompt = format!("{SYSTEM_PROMPT}\n\n{input}");
        let request = GenerateRequest {
            model: self.model.clone(),
            prompt,
            stream: false,
        };

        let resp = self
            .client
            .post(url)
            .json(&request)
            .timeout(self.timeout)
            .send()
            .await
            .context("send ollama request")?;

        let status = resp.status();
        let body = resp.text().await.context("read ollama response")?;
        if !status.is_success() {
            anyhow::bail!("ollama http error: status={status} body={body}");
        }

        let parsed = serde_json::from_str::<GenerateResponse>(&body).context("parse ollama json")?;
        let corrected = parsed.response.trim().to_string();
        let corrected = if corrected.is_empty() { input.to_string() } else { corrected };

        Ok(CorrectionResult {
            original: input.to_string(),
            changed: corrected != input,
            corrected,
            latency_ms: elapsed_ms(started),
        })
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/api/version", self.endpoint.trim_end_matches('/'));
        let resp = self.client.get(url).timeout(self.timeout).send().await;
        let Ok(resp) = resp else {
            return false;
        };
        if !resp.status().is_success() {
            return false;
        }
        let body = resp.text().await.unwrap_or_default();
        serde_json::from_str::<VersionResponse>(&body)
            .map(|v| !v.version.trim().is_empty())
            .unwrap_or(false)
    }
}

impl OllamaEngine {
    pub fn new(endpoint: String, model: String, timeout_ms: u64) -> anyhow::Result<Self> {
        let endpoint = endpoint.trim().trim_end_matches('/').to_string();
        if endpoint.is_empty() {
            anyhow::bail!("LLM endpoint 不能为空");
        }

        let model = model.trim().to_string();
        if model.is_empty() {
            anyhow::bail!("LLM model 不能为空");
        }

        let client = Client::builder().build().context("build reqwest client")?;
        Ok(Self {
            client,
            endpoint,
            model,
            timeout: Duration::from_millis(timeout_ms.max(200)),
        })
    }
}

const SYSTEM_PROMPT: &str = "你是中文文本校正助手。修正语音识别文本的错别字和语法错误，保持原意。只输出修正后的文本，无需解释。若无需修正则原样输出。";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_engine_new_validates_required_fields() {
        assert!(OllamaEngine::new("".to_string(), "m".to_string(), 3000).is_err());
        assert!(OllamaEngine::new("http://localhost:11434".to_string(), "".to_string(), 3000).is_err());
    }
}
