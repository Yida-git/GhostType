use anyhow::Context as _;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::llm::{elapsed_ms, CorrectionResult, LlmEngine};

pub struct OpenAiCompatEngine {
    client: Client,
    endpoint: String,
    api_key: String,
    model: String,
    timeout: Duration,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    content: String,
}

#[async_trait]
impl LlmEngine for OpenAiCompatEngine {
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

        let url = format!("{}/chat/completions", self.endpoint.trim_end_matches('/'));
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: SYSTEM_PROMPT.to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: input.to_string(),
                },
            ],
            temperature: 0.1,
            max_tokens: 200,
        };

        let resp = self
            .client
            .post(url)
            .json(&request)
            .timeout(self.timeout)
            .send()
            .await
            .context("send openai compat request")?;

        let status = resp.status();
        let body = resp.text().await.context("read openai compat response")?;
        if !status.is_success() {
            anyhow::bail!("openai compat http error: status={status} body={body}");
        }

        let parsed = serde_json::from_str::<ChatResponse>(&body).context("parse openai compat json")?;
        let corrected = parsed
            .choices
            .first()
            .map(|c| c.message.content.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| input.to_string());

        Ok(CorrectionResult {
            original: input.to_string(),
            changed: corrected != input,
            corrected,
            latency_ms: elapsed_ms(started),
        })
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/models", self.endpoint.trim_end_matches('/'));
        let resp = self
            .client
            .get(url)
            .timeout(self.timeout)
            .send()
            .await;
        resp.map(|r| r.status().is_success()).unwrap_or(false)
    }
}

impl OpenAiCompatEngine {
    pub fn new(endpoint: String, api_key: String, model: String, timeout_ms: u64) -> anyhow::Result<Self> {
        let endpoint = endpoint.trim().trim_end_matches('/').to_string();
        if endpoint.is_empty() {
            anyhow::bail!("LLM endpoint 不能为空");
        }

        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            anyhow::bail!("LLM api_key 不能为空");
        }

        let model = model.trim().to_string();
        if model.is_empty() {
            anyhow::bail!("LLM model 不能为空");
        }

        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_str(&format!("Bearer {api_key}")).context("invalid api key header")?;
        headers.insert(AUTHORIZATION, value);

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("build reqwest client")?;

        Ok(Self {
            client,
            endpoint,
            api_key,
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
    fn openai_compat_engine_new_validates_required_fields() {
        assert!(OpenAiCompatEngine::new("".to_string(), "k".to_string(), "m".to_string(), 3000).is_err());
        assert!(OpenAiCompatEngine::new("https://x".to_string(), "".to_string(), "m".to_string(), 3000).is_err());
        assert!(OpenAiCompatEngine::new("https://x".to_string(), "k".to_string(), "".to_string(), 3000).is_err());
    }
}
