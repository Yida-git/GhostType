use anyhow::Context as _;
use crate::asr;
use crate::llm;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientConfig {
    /// 配置结构版本号（用于未来的迁移/兼容）
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default)]
    pub audio_device: Option<String>,
    #[serde(default)]
    pub asr: asr::AsrConfig,
    #[serde(default)]
    pub llm: llm::LlmConfig,

    // === legacy fields (兼容旧版 config.json) ===
    #[serde(default, skip_serializing)]
    pub server_endpoints: Vec<String>,
    #[serde(default, skip_serializing)]
    pub use_cloud_api: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            hotkey: default_hotkey(),
            audio_device: None,
            asr: asr::AsrConfig::default(),
            llm: llm::LlmConfig::default(),
            server_endpoints: Vec::new(),
            use_cloud_api: false,
        }
    }
}

fn default_schema_version() -> u32 {
    210
}

pub fn load_with_path() -> (ClientConfig, Option<PathBuf>) {
    for path in candidate_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<ClientConfig>(&content) {
                return (normalize_legacy_config(config), Some(path));
            }
        }
    }

    (ClientConfig::default(), None)
}

pub fn save_to_path(config: &ClientConfig, path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let config = normalize_legacy_config(config.clone());
    let path = path.unwrap_or_else(default_save_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("create config dir")?;
        }
    }

    let content = serde_json::to_string_pretty(&config).context("serialize config")?;
    std::fs::write(&path, content).context("write config")?;
    Ok(path)
}

fn default_hotkey() -> String {
    if cfg!(target_os = "macos") {
        "f8".to_string()
    } else {
        "capslock".to_string()
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(explicit) = std::env::var("GHOSTTYPE_CONFIG") {
        paths.push(PathBuf::from(explicit));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("config.json"));

            #[cfg(target_os = "macos")]
            if let Some(contents_dir) = dir.parent() {
                paths.push(contents_dir.join("Resources").join("config.json"));
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("config.json"));
        paths.push(cwd.join("..").join("config.json"));
    }

    paths.push(PathBuf::from("client").join("config.json"));
    paths
}

fn default_save_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("config.json");
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join("config.json");
    }

    PathBuf::from("config.json")
}

fn normalize_legacy_config(mut config: ClientConfig) -> ClientConfig {
    // 旧版字段：server_endpoints → asr.websocket.endpoint
    if let asr::AsrConfig::WebSocket { endpoint } = &config.asr {
        let is_default = endpoint.trim().is_empty() || endpoint.trim() == asr::default_websocket_endpoint();
        if is_default && !config.server_endpoints.is_empty() {
            let endpoint = config.server_endpoints[0].trim().to_string();
            if !endpoint.is_empty() {
                config.asr = asr::AsrConfig::WebSocket { endpoint };
            }
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_server_endpoints_overrides_default_asr_endpoint() {
        let config = ClientConfig {
            schema_version: default_schema_version(),
            hotkey: "f8".to_string(),
            audio_device: None,
            asr: asr::AsrConfig::default(),
            llm: llm::LlmConfig::default(),
            server_endpoints: vec!["ws://10.0.0.1:8000/ws".to_string()],
            use_cloud_api: false,
        };

        let normalized = normalize_legacy_config(config);
        match normalized.asr {
            asr::AsrConfig::WebSocket { endpoint } => assert_eq!(endpoint, "ws://10.0.0.1:8000/ws"),
            other => panic!("unexpected asr config: {other:?}"),
        }
    }

    #[test]
    fn legacy_does_not_override_custom_asr_endpoint() {
        let config = ClientConfig {
            schema_version: default_schema_version(),
            hotkey: "f8".to_string(),
            audio_device: None,
            asr: asr::AsrConfig::WebSocket {
                endpoint: "ws://192.168.1.8:8000/ws".to_string(),
            },
            llm: llm::LlmConfig::default(),
            server_endpoints: vec!["ws://10.0.0.1:8000/ws".to_string()],
            use_cloud_api: false,
        };

        let normalized = normalize_legacy_config(config);
        match normalized.asr {
            asr::AsrConfig::WebSocket { endpoint } => assert_eq!(endpoint, "ws://192.168.1.8:8000/ws"),
            other => panic!("unexpected asr config: {other:?}"),
        }
    }
}
