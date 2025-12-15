use enigo::{Enigo, Key, KeyboardControllable};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum InjectCommand {
    TypeText(String),
    Backspace(usize),
}

#[derive(Clone)]
pub struct Injector {
    pub tx: mpsc::Sender<InjectCommand>,
}

pub fn spawn_injector() -> Injector {
    let (tx, mut rx) = mpsc::channel::<InjectCommand>(256);

    tauri::async_runtime::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            let _ = tokio::task::spawn_blocking(move || apply_command(cmd)).await;
        }
    });

    Injector { tx }
}

fn apply_command(cmd: InjectCommand) {
    let mut enigo = Enigo::new();

    match cmd {
        InjectCommand::TypeText(text) => {
            enigo.text(&text);
        }
        InjectCommand::Backspace(count) => {
            for _ in 0..count {
                enigo.key_click(Key::Backspace);
            }
        }
    }
}

