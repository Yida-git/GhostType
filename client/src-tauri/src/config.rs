use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientConfig {
    pub server_endpoints: Vec<String>,
    #[serde(default)]
    pub use_cloud_api: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_endpoints: vec!["ws://127.0.0.1:8000/ws".to_string()],
            use_cloud_api: false,
            hotkey: default_hotkey(),
        }
    }
}

pub fn load_with_path() -> (ClientConfig, Option<PathBuf>) {
    for path in candidate_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<ClientConfig>(&content) {
                return (config, Some(path));
            }
        }
    }

    (ClientConfig::default(), None)
}

pub fn save_to_path(config: &ClientConfig, path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = path.unwrap_or_else(default_save_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("create config dir")?;
        }
    }

    let content = serde_json::to_string_pretty(config).context("serialize config")?;
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
