use std::sync::Mutex;

use tauri::async_runtime::JoinHandle;
use tokio::sync::Mutex as AsyncMutex;

use crate::audio::AudioRecorder;
use crate::pipeline::Pipeline;

pub struct AppState {
    pub audio: Mutex<Option<AudioRecorder>>,
    pub audio_task: Mutex<Option<JoinHandle<()>>>,
    pub session_gen: Mutex<Option<u64>>,
    pub pipeline: AsyncMutex<Pipeline>,
    pub audio_device: Option<String>,
}

impl AppState {
    pub fn new(pipeline: Pipeline, audio_device: Option<String>) -> Self {
        Self {
            audio: Mutex::new(None),
            audio_task: Mutex::new(None),
            session_gen: Mutex::new(None),
            pipeline: AsyncMutex::new(pipeline),
            audio_device,
        }
    }
}
