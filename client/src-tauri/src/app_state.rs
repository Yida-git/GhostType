use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::audio::AudioRecorder;
use crate::input::Injector;
use crate::network::{NetworkCommand, NetworkHandle};

pub struct AppState {
    pub network: NetworkHandle,
    pub injector: Injector,
    pub audio: Mutex<Option<AudioRecorder>>,
    pub use_cloud_api: bool,
    pub tx: mpsc::Sender<NetworkCommand>,
}

impl AppState {
    pub fn new(network: NetworkHandle, injector: Injector, use_cloud_api: bool) -> Self {
        Self {
            tx: network.tx.clone(),
            network,
            injector,
            audio: Mutex::new(None),
            use_cloud_api,
        }
    }
}

