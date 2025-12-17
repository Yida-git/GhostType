use anyhow::{anyhow, Context as _};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, Stream, StreamConfig};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[derive(Debug, Clone, serde::Serialize)]
pub struct InputDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub struct AudioRecorder {
    stop_tx: crossbeam_channel::Sender<()>,
    join: Option<std::thread::JoinHandle<()>>,
    pub trace_id: String,
    pub sample_rate: u32,
}

impl AudioRecorder {
    pub fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start_audio(
    trace_id: String,
    device_name: Option<String>,
) -> anyhow::Result<(AudioRecorder, mpsc::Receiver<Vec<i16>>)> {
    let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<anyhow::Result<u32>>(1);
    let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<i16>>(64);

    let trace_id_for_thread = trace_id.clone();
    let requested_device = device_name.clone();
    let join = std::thread::spawn(move || {
        let start_result =
            (|| -> anyhow::Result<(Stream, crossbeam_channel::Receiver<Vec<f32>>, u32, String)> {
            let host = cpal::default_host();
            let device = select_input_device(&host, requested_device.as_deref())?;

            let device_name = device.name().unwrap_or_else(|_| "default".to_string());
            let (config, sample_format, sample_rate) = pick_stream_config(&device)?;
            let channels = config.channels as usize;

            let (raw_tx, raw_rx) = crossbeam_channel::bounded::<Vec<f32>>(16);
            let raw_tx = Arc::new(raw_tx);

            let stream = build_input_stream(&device, &config, sample_format, channels, raw_tx)?;
            stream.play().context("start input stream")?;

            Ok((stream, raw_rx, sample_rate, device_name))
        })();

        let (stream, raw_rx, sample_rate, device_name) = match start_result {
            Ok(parts) => {
                let _ = ready_tx.send(Ok(parts.2));
                parts
            }
            Err(err) => {
                let _ = ready_tx.send(Err(err));
                return;
            }
        };

        info!(
            target: "audio",
            trace_id = %trace_id_for_thread,
            sample_rate = sample_rate,
            device = device_name.as_str(),
            "录音开始 | Recording started"
        );

        let frame_size = (sample_rate / 50) as usize;
        let mut pcm_buf: Vec<i16> = Vec::with_capacity(frame_size * 4);
        let started_at = Instant::now();
        let mut packets: u64 = 0;
        let mut total_samples: u64 = 0;

        loop {
            crossbeam_channel::select! {
                recv(stop_rx) -> _ => break,
                recv(raw_rx) -> msg => {
                    let Ok(chunk) = msg else { break };
                    pcm_buf.extend(chunk.into_iter().map(f32_to_i16));

                    while pcm_buf.len() >= frame_size {
                        let frame: Vec<i16> = pcm_buf.drain(..frame_size).collect();
                        packets = packets.wrapping_add(1);
                        total_samples = total_samples.wrapping_add(frame.len() as u64);
                        debug!(
                            target: "audio",
                            trace_id = %trace_id_for_thread,
                            samples = frame.len(),
                            packets = packets,
                            "音频帧已采集 | Audio frame captured"
                        );
                        if pcm_tx.blocking_send(frame).is_err() {
                            break;
                        }
                    }
                }
            }
        }

        info!(
            target: "audio",
            trace_id = %trace_id_for_thread,
            duration_ms = started_at.elapsed().as_millis(),
            packets = packets,
            total_samples = total_samples,
            "录音结束 | Recording stopped"
        );

        drop(stream);
    });

    let sample_rate = ready_rx
        .recv()
        .context("audio thread start failed")??;

    Ok((
        AudioRecorder {
            stop_tx,
            join: Some(join),
            trace_id,
            sample_rate,
        },
        pcm_rx,
    ))
}

pub fn list_input_devices() -> anyhow::Result<Vec<InputDeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let mut out = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            let Ok(name) = device.name() else {
                continue;
            };
            out.push(InputDeviceInfo {
                is_default: !default_name.is_empty() && name == default_name,
                name,
            });
        }
    }

    if out.is_empty() && !default_name.is_empty() {
        out.push(InputDeviceInfo {
            name: default_name,
            is_default: true,
        });
    }

    Ok(out)
}

pub fn check_microphone_access(requested: Option<&str>) -> bool {
    let host = cpal::default_host();
    let Ok(device) = select_input_device(&host, requested) else {
        return false;
    };
    device.default_input_config().is_ok()
}

fn select_input_device(host: &cpal::Host, requested: Option<&str>) -> anyhow::Result<Device> {
    let requested = requested.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    if let Some(want) = requested {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                let Ok(name) = device.name() else {
                    continue;
                };
                if name == want {
                    info!(
                        target: "audio",
                        device = name.as_str(),
                        "选择音频输入设备 | Audio input device selected"
                    );
                    return Ok(device);
                }
            }
        }

        tracing::warn!(
            target: "audio",
            device = want,
            "未找到指定音频设备，回退默认设备 | Requested device not found, falling back to default"
        );
    }

    host.default_input_device()
        .ok_or_else(|| anyhow!("no input device"))
}

fn pick_stream_config(device: &Device) -> anyhow::Result<(StreamConfig, SampleFormat, u32)> {
    let target_rates: [u32; 5] = [48000, 16000, 24000, 12000, 8000];

    let mut ranges = Vec::new();
    if let Ok(configs) = device.supported_input_configs() {
        for cfg in configs {
            debug!(
                target: "audio",
                format = ?cfg.sample_format(),
                channels = cfg.channels(),
                min_rate = cfg.min_sample_rate().0,
                max_rate = cfg.max_sample_rate().0,
                "设备支持的配置 | Supported config"
            );
            ranges.push(cfg);
        }
    }

    for rate in target_rates {
        for range in &ranges {
            let min = range.min_sample_rate().0;
            let max = range.max_sample_rate().0;
            if rate < min || rate > max {
                continue;
            }
            let config = range.with_sample_rate(cpal::SampleRate(rate));
            let sample_format = config.sample_format();
            info!(
                target: "audio",
                format = ?sample_format,
                sample_rate = rate,
                channels = config.channels(),
                "选择音频配置 | Audio config selected"
            );
            return Ok((config.clone().into(), sample_format, rate));
        }
    }

    let default_config = device.default_input_config().context("default_input_config")?;
    let sample_rate = default_config.sample_rate().0;
    let sample_format = default_config.sample_format();

    info!(
        target: "audio",
        format = ?sample_format,
        sample_rate = sample_rate,
        channels = default_config.channels(),
        "使用默认配置 | Using default config"
    );

    if matches!(sample_rate, 8000 | 12000 | 16000 | 24000 | 48000) {
        return Ok((default_config.into(), sample_format, sample_rate));
    }

    Err(anyhow!(
        "不支持的采样率 | Unsupported sample rate: {sample_rate} (需要 8000/12000/16000/24000/48000)"
    ))
}

fn build_input_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    raw_tx: Arc<crossbeam_channel::Sender<Vec<f32>>>,
) -> anyhow::Result<Stream> {
    let err_fn = move |err| {
        error!(
            target: "audio",
            error = %err,
            "音频流错误 | Audio stream error"
        );
    };

    macro_rules! build_stream {
        ($sample_type:ty) => {
            device.build_input_stream(
                config,
                {
                    let raw_tx = raw_tx.clone();
                    move |data: &[$sample_type], _| push_mono(data, channels, &raw_tx)
                },
                err_fn,
                None,
            )?
        };
    }

    let stream = match sample_format {
        SampleFormat::I8 => build_stream!(i8),
        SampleFormat::I16 => build_stream!(i16),
        SampleFormat::I32 => build_stream!(i32),
        SampleFormat::I64 => build_stream!(i64),
        SampleFormat::U8 => build_stream!(u8),
        SampleFormat::U16 => build_stream!(u16),
        SampleFormat::U32 => build_stream!(u32),
        SampleFormat::U64 => build_stream!(u64),
        SampleFormat::F32 => build_stream!(f32),
        SampleFormat::F64 => build_stream!(f64),
        _ => {
            return Err(anyhow!(
                "未知采样格式 | Unknown sample format: {sample_format:?}"
            ))
        }
    };

    Ok(stream)
}

fn push_mono<T>(data: &[T], channels: usize, raw_tx: &crossbeam_channel::Sender<Vec<f32>>)
where
    T: Sample,
    f32: FromSample<T>,
{
    if channels == 0 {
        return;
    }

    let mut mono = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks(channels) {
        mono.push(f32::from_sample(frame[0]));
    }

    let _ = raw_tx.try_send(mono);
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    (sample * i16::MAX as f32) as i16
}
