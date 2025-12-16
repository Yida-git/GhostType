use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
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

pub fn load() -> ClientConfig {
    for path in candidate_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<ClientConfig>(&content) {
                return config;
            }
        }
    }

    ClientConfig::default()
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
