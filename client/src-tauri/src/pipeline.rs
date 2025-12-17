use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};

use crate::asr::{self, AsrContext, AsrEngine};
use crate::input::{InjectCommand, Injector};
use crate::llm::{self, LlmEngine};

pub struct Pipeline {
    asr: Box<dyn AsrEngine>,
    llm: Arc<dyn LlmEngine>,
    injector: Injector,
    generation: Arc<AtomicU64>,
    cancel_tx: watch::Sender<u64>,
    _cancel_rx: watch::Receiver<u64>,
    trace_id: Option<String>,
    injected_len: usize,
}

impl Pipeline {
    pub fn new(asr_config: &asr::AsrConfig, llm_config: &llm::LlmConfig, injector: Injector) -> anyhow::Result<Self> {
        let asr = asr::create_engine(asr_config)?;
        let llm_engine = llm::create_engine(llm_config)?;
        let llm: Arc<dyn LlmEngine> = Arc::from(llm_engine);
        let (cancel_tx, cancel_rx) = watch::channel::<u64>(0);

        Ok(Self {
            asr,
            llm,
            injector,
            generation: Arc::new(AtomicU64::new(0)),
            cancel_tx,
            _cancel_rx: cancel_rx,
            trace_id: None,
            injected_len: 0,
        })
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }

    pub fn events(&mut self) -> &mut mpsc::Receiver<asr::AsrEvent> {
        self.asr.events()
    }

    pub async fn start(&mut self, trace_id: String, sample_rate: u32, context: AsrContext) -> anyhow::Result<u64> {
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.cancel_tx.send(gen);
        self.trace_id = Some(trace_id.clone());
        self.injected_len = 0;

        info!(
            target: "pipeline",
            trace_id = trace_id.as_str(),
            sample_rate = sample_rate,
            gen = gen,
            "ASR 会话开始 | ASR session started"
        );

        self.asr.start(trace_id, sample_rate, context).await?;
        Ok(gen)
    }

    pub async fn feed_audio(&mut self, pcm: &[i16]) -> anyhow::Result<()> {
        self.asr.feed_audio(pcm).await
    }

    pub async fn stop(&mut self, session_gen: u64) -> anyhow::Result<()> {
        let trace_id = self.trace_id.clone();
        let started = Instant::now();
        let cancel_rx = self.cancel_tx.subscribe();

        let asr_text = self.asr.stop().await?;
        let asr_text = asr_text.trim().to_string();

        debug!(
            target: "pipeline",
            trace_id = trace_id.as_deref().unwrap_or(""),
            gen = session_gen,
            text_len = asr_text.chars().count(),
            "ASR 完成 | ASR completed"
        );

        if asr_text.is_empty() {
            self.trace_id = None;
            self.injected_len = 0;
            return Ok(());
        }

        let injected_at = Instant::now();
        let injected_len = asr_text.chars().count();
        self.injected_len = injected_len;

        let _ = self
            .injector
            .tx
            .send(InjectCommand::TypeText {
                trace_id: trace_id.clone(),
                text: asr_text.clone(),
            })
            .await
            .map_err(|err| {
                error!(
                    target: "pipeline",
                    trace_id = trace_id.as_deref().unwrap_or(""),
                    gen = session_gen,
                    error = %err,
                    "文字注入失败：注入通道已关闭 | Injection channel closed"
                );
            })
            .ok();

        info!(
            target: "pipeline",
            trace_id = trace_id.as_deref().unwrap_or(""),
            gen = session_gen,
            len = injected_len,
            asr_ms = started.elapsed().as_millis() as u64,
            "ASR 已输出 | ASR injected"
        );

        let generation = self.generation.clone();
        let llm = self.llm.clone();
        let injector = self.injector.clone();
        let original = asr_text;
        let trace_id_for_task = trace_id.clone();
        let injected_at_for_task = injected_at;
        let mut cancel_rx = cancel_rx;

        tauri::async_runtime::spawn(async move {
            let llm_started = Instant::now();
            let correction = tokio::select! {
                _ = cancel_rx.changed() => {
                    warn!(
                        target: "pipeline",
                        trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                        gen = session_gen,
                        "LLM 校正已取消：检测到新会话 | LLM cancelled: new session started"
                    );
                    return;
                }
                res = llm.correct(&original) => res,
            };

            let min_delay = Duration::from_millis(500);
            let since_injected = injected_at_for_task.elapsed();
            if since_injected < min_delay {
                let remaining = min_delay - since_injected;
                tokio::select! {
                    _ = cancel_rx.changed() => {
                        warn!(
                            target: "pipeline",
                            trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                            gen = session_gen,
                            "LLM 校正已取消：检测到新会话 | LLM cancelled: new session started"
                        );
                        return;
                    }
                    _ = tokio::time::sleep(remaining) => {}
                }
            }

            if generation.load(Ordering::SeqCst) != session_gen {
                warn!(
                    target: "pipeline",
                    trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                    gen = session_gen,
                    "跳过校正：已有新会话 | Skip correction: new session started"
                );
                return;
            }

            let Ok(correction) = correction else {
                warn!(
                    target: "pipeline",
                    trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                    gen = session_gen,
                    error = %format!("{correction:?}"),
                    latency_ms = llm_started.elapsed().as_millis() as u64,
                    "LLM 校正失败 | LLM correction failed"
                );
                return;
            };

            if !correction.changed {
                debug!(
                    target: "pipeline",
                    trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                    gen = session_gen,
                    latency_ms = correction.latency_ms,
                    "LLM 无需校正 | LLM no change"
                );
                return;
            }

            let corrected = correction.corrected.trim().to_string();
            if corrected.is_empty() {
                return;
            }

            info!(
                target: "pipeline",
                trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                gen = session_gen,
                latency_ms = correction.latency_ms,
                "LLM 校正就绪，开始替换 | LLM correction ready, replacing"
            );

            if injector
                .tx
                .send(InjectCommand::Backspace {
                    trace_id: trace_id_for_task.clone(),
                    count: injected_len,
                })
                .await
                .is_err()
            {
                warn!(
                    target: "pipeline",
                    trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                    gen = session_gen,
                    "退格注入失败：注入通道已关闭 | Backspace injection failed (channel closed)"
                );
                return;
            }

            if injector
                .tx
                .send(InjectCommand::TypeText {
                    trace_id: trace_id_for_task.clone(),
                    text: corrected,
                })
                .await
                .is_err()
            {
                warn!(
                    target: "pipeline",
                    trace_id = trace_id_for_task.as_deref().unwrap_or(""),
                    gen = session_gen,
                    "文字注入失败：注入通道已关闭 | Injection failed (channel closed)"
                );
            }
        });

        self.trace_id = None;
        self.injected_len = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockAsrEngine {
        final_text: String,
        rx: mpsc::Receiver<asr::AsrEvent>,
    }

    impl MockAsrEngine {
        fn new(final_text: impl Into<String>) -> Self {
            let (_tx, rx) = mpsc::channel(8);
            Self {
                final_text: final_text.into(),
                rx,
            }
        }
    }

    #[async_trait]
    impl AsrEngine for MockAsrEngine {
        async fn start(&mut self, _trace_id: String, _sample_rate: u32, _context: AsrContext) -> anyhow::Result<()> {
            Ok(())
        }

        async fn feed_audio(&mut self, _pcm: &[i16]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<String> {
            Ok(self.final_text.clone())
        }

        fn events(&mut self) -> &mut mpsc::Receiver<asr::AsrEvent> {
            &mut self.rx
        }
    }

    struct MockLlmEngine {
        corrected: String,
        changed: bool,
    }

    impl MockLlmEngine {
        fn new(corrected: impl Into<String>, changed: bool) -> Self {
            Self {
                corrected: corrected.into(),
                changed,
            }
        }
    }

    #[async_trait]
    impl LlmEngine for MockLlmEngine {
        async fn correct(&self, text: &str) -> anyhow::Result<llm::CorrectionResult> {
            Ok(llm::CorrectionResult {
                original: text.to_string(),
                corrected: self.corrected.clone(),
                changed: self.changed,
                latency_ms: 0,
            })
        }

        async fn health_check(&self) -> bool {
            true
        }
    }

    fn test_pipeline(asr_text: &str, corrected: &str, changed: bool) -> (Pipeline, mpsc::Receiver<InjectCommand>) {
        let (tx, rx) = mpsc::channel(16);
        let injector = Injector { tx };

        let asr: Box<dyn AsrEngine> = Box::new(MockAsrEngine::new(asr_text));
        let llm: Arc<dyn LlmEngine> = Arc::new(MockLlmEngine::new(corrected, changed));
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel::<u64>(0);

        (
            Pipeline {
                asr,
                llm,
                injector,
                generation: Arc::new(AtomicU64::new(0)),
                cancel_tx,
                _cancel_rx: cancel_rx,
                trace_id: None,
                injected_len: 0,
            },
            rx,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn pipeline_replaces_after_500ms_delay() {
        let (mut pipeline, mut rx) = test_pipeline("你好", "您好", true);

        let gen = pipeline
            .start("t1".to_string(), 16000, AsrContext::default())
            .await
            .expect("start");
        pipeline.stop(gen).await.expect("stop");

        let cmd1 = rx.recv().await.expect("cmd1");
        match cmd1 {
            InjectCommand::TypeText { text, .. } => assert_eq!(text, "你好"),
            other => panic!("unexpected cmd1: {other:?}"),
        }

        assert!(rx.try_recv().is_err(), "不应在 500ms 内替换");

        tokio::time::advance(Duration::from_millis(500)).await;
        tokio::task::yield_now().await;

        let cmd2 = rx.recv().await.expect("cmd2");
        match cmd2 {
            InjectCommand::Backspace { count, .. } => assert_eq!(count, 2),
            other => panic!("unexpected cmd2: {other:?}"),
        }

        let cmd3 = rx.recv().await.expect("cmd3");
        match cmd3 {
            InjectCommand::TypeText { text, .. } => assert_eq!(text, "您好"),
            other => panic!("unexpected cmd3: {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn pipeline_skips_replace_when_new_session_started() {
        let (mut pipeline, mut rx) = test_pipeline("hello", "fixed", true);

        let gen1 = pipeline
            .start("t1".to_string(), 16000, AsrContext::default())
            .await
            .expect("start 1");
        pipeline.stop(gen1).await.expect("stop 1");

        let _ = rx.recv().await.expect("cmd1");

        let _gen2 = pipeline
            .start("t2".to_string(), 16000, AsrContext::default())
            .await
            .expect("start 2");

        tokio::time::advance(Duration::from_millis(500)).await;
        tokio::task::yield_now().await;

        assert!(rx.try_recv().is_err(), "新会话开始后不应替换旧结果");
    }

    #[tokio::test(start_paused = true)]
    async fn pipeline_no_replace_when_llm_unchanged() {
        let (mut pipeline, mut rx) = test_pipeline("hello", "hello", false);

        let gen = pipeline
            .start("t1".to_string(), 16000, AsrContext::default())
            .await
            .expect("start");
        pipeline.stop(gen).await.expect("stop");

        let _ = rx.recv().await.expect("cmd1");

        tokio::time::advance(Duration::from_millis(500)).await;
        tokio::task::yield_now().await;

        assert!(rx.try_recv().is_err(), "LLM 无变化不应替换");
    }
}
