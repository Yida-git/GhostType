use active_win_pos_rs::ActiveWindow;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[derive(Debug)]
pub enum InjectCommand {
    TypeText { trace_id: Option<String>, text: String },
    Backspace { trace_id: Option<String>, count: usize },
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
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        error!(
            target: "input",
            trace_id = cmd.trace_id_for_log(),
            "键盘注入初始化失败 | Keyboard injection failed"
        );
        return;
    };

    match cmd {
        InjectCommand::TypeText { trace_id, text } => {
            let target_app = get_active_app_name();
            let len = text.chars().count();
            match enigo.text(&text) {
                Ok(()) => {
                    if let Some(tid) = trace_id.as_deref() {
                        info!(
                            target: "input",
                            trace_id = %tid,
                            len = len,
                            target_app = %target_app.as_deref().unwrap_or(""),
                            "文字已注入 | Text injected"
                        );
                    } else {
                        info!(
                            target: "input",
                            len = len,
                            target_app = %target_app.as_deref().unwrap_or(""),
                            "文字已注入 | Text injected"
                        );
                    }
                }
                Err(err) => {
                    if let Some(tid) = trace_id.as_deref() {
                        error!(
                            target: "input",
                            trace_id = %tid,
                            error = %err,
                            "文字注入失败 | Text injection failed"
                        );
                    } else {
                        error!(
                            target: "input",
                            error = %err,
                            "文字注入失败 | Text injection failed"
                        );
                    }
                }
            }
        }
        InjectCommand::Backspace { trace_id, count } => {
            if let Some(tid) = trace_id.as_deref() {
                debug!(
                    target: "input",
                    trace_id = %tid,
                    count = count,
                    "退格注入 | Backspace injected"
                );
            }
            for _ in 0..count {
                let _ = enigo.key(Key::Backspace, Direction::Click);
            }
        }
    }
}

fn get_active_app_name() -> Option<String> {
    let ActiveWindow { app_name, .. } = active_win_pos_rs::get_active_window().ok()?;
    Some(app_name)
}

impl InjectCommand {
    fn trace_id_for_log(&self) -> &str {
        match self {
            InjectCommand::TypeText { trace_id, .. } => trace_id.as_deref().unwrap_or(""),
            InjectCommand::Backspace { trace_id, .. } => trace_id.as_deref().unwrap_or(""),
        }
    }
}
