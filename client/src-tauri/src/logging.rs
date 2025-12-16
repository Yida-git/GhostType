use std::fmt;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

pub fn init() {
    let filter = env_filter();
    let fmt_layer = tracing_subscriber::fmt::layer().event_format(GhostTypeFormat);

    // NOTE: Use `try_init` to avoid panicking when Tauri reloads in dev.
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init();
}

fn env_filter() -> EnvFilter {
    let raw = std::env::var("GHOSTTYPE_LOG")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok());

    let default_level = if cfg!(debug_assertions) { "debug" } else { "info" };

    let Some(raw) = raw else {
        return EnvFilter::new(default_level);
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return EnvFilter::new(default_level);
    }

    let normalized = if trimmed.contains('=') || trimmed.contains(',') {
        trimmed.to_string()
    } else {
        trimmed.to_ascii_lowercase()
    };

    EnvFilter::try_new(normalized).unwrap_or_else(|_| EnvFilter::new(default_level))
}

struct GhostTypeFormat;

impl<S, N> FormatEvent<S, N> for GhostTypeFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let now = chrono::Local::now();
        let ts = now.format("%Y-%m-%d %H:%M:%S%.3f");

        let level = level_str(event.metadata().level());
        let module = module_name(event.metadata().target());

        let mut fields = FieldVisitor::default();
        event.record(&mut fields);

        write!(writer, "[{ts}] [{level:<5}] [{module:<8}] ")?;
        if let Some(trace_id) = fields.trace_id.as_deref().filter(|v| !v.is_empty()) {
            write!(writer, "[t:{trace_id}] ")?;
        }

        if let Some(message) = fields.message.as_deref() {
            write!(writer, "{message}")?;
        }

        if !fields.kvs.is_empty() {
            write!(writer, " | ")?;
            for (idx, (key, value)) in fields.kvs.iter().enumerate() {
                if idx > 0 {
                    write!(writer, " ")?;
                }
                write!(writer, "{key}={}", quote_value_if_needed(value))?;
            }
        }

        writeln!(writer)
    }
}

fn level_str(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

fn module_name(target: &str) -> &str {
    // Prefer the short module id (app/hotkey/audio/...), but gracefully shorten
    // long targets from dependencies.
    let last = target.rsplit("::").next().unwrap_or(target);
    if last.is_empty() {
        target
    } else {
        last
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    trace_id: Option<String>,
    kvs: Vec<(String, String)>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "trace_id" => self.trace_id = Some(value.to_string()),
            name => self.kvs.push((name.to_string(), value.to_string())),
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.record_str(field, if value { "true" } else { "false" });
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.record_str(field, &value.to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.record_str(field, &value.to_string());
    }

    fn record_i128(&mut self, field: &tracing::field::Field, value: i128) {
        self.record_str(field, &value.to_string());
    }

    fn record_u128(&mut self, field: &tracing::field::Field, value: u128) {
        self.record_str(field, &value.to_string());
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.record_str(field, &value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let rendered = format!("{value:?}");
        match field.name() {
            "message" => self.message = Some(unquote_debug_string(&rendered)),
            "trace_id" => self.trace_id = Some(unquote_debug_string(&rendered)),
            name => self.kvs.push((name.to_string(), rendered)),
        }
    }
}

fn unquote_debug_string(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].replace("\\\"", "\"");
    }
    trimmed.to_string()
}

fn quote_value_if_needed(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "\"\"".to_string();
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed.to_string();
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
    {
        return trimmed.to_string();
    }
    format!("\"{}\"", trimmed.replace('\\', "\\\\").replace('"', "\\\""))
}
