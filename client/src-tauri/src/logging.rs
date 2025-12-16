use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Metadata, Record};

/// 一个尽量“零依赖”的日志实现，用于替代零散的 `println!/eprintln!`。
///
/// 说明：
/// - 不引入 `env_logger`/`tracing`，避免额外依赖与网络下载。
/// - 通过 `GHOSTTYPE_LOG` 或 `RUST_LOG` 控制日志级别（支持 error/warn/info/debug/trace）。
pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;

    let level = std::env::var("GHOSTTYPE_LOG")
        .ok()
        .and_then(parse_level)
        .or_else(|| std::env::var("RUST_LOG").ok().and_then(parse_level))
        .unwrap_or_else(|| {
            if cfg!(debug_assertions) {
                LevelFilter::Info
            } else {
                LevelFilter::Warn
            }
        });

    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        eprintln!("[{ts_ms}][{}][{}] {}", record.level(), record.target(), record.args());
    }

    fn flush(&self) {}
}

fn parse_level(raw: String) -> Option<LevelFilter> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

