use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::audio::AudioRecorder;
use crate::network::{NetworkCommand, NetworkHandle};

pub struct AppState {
    pub audio: Mutex<Option<AudioRecorder>>,
    pub use_cloud_api: bool,
    pub tx: mpsc::Sender<NetworkCommand>,
}

impl AppState {
    pub fn new(network: NetworkHandle, use_cloud_api: bool) -> Self {
        Self {
            tx: network.tx.clone(),
            audio: Mutex::new(None),
            use_cloud_api,
        }
    }
}
