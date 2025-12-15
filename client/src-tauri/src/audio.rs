use anyhow::{anyhow, Context as _};
use audiopus::{coder::Encoder, Application, Channels, SampleRate};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Sample, SampleFormat, Stream, StreamConfig};
use std::sync::Arc;

use crate::network::NetworkCommand;

pub struct AudioRecorder {
    stream: Option<Stream>,
    join: Option<std::thread::JoinHandle<()>>,
    pub sample_rate: u32,
}

impl AudioRecorder {
    pub fn stop(mut self) {
        self.stream.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start_audio(network_tx: tokio::sync::mpsc::Sender<NetworkCommand>) -> anyhow::Result<AudioRecorder> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no input device"))?;

    let (config, sample_format, sample_rate) = pick_stream_config(&device)?;
    let channels = config.channels as usize;

    let (raw_tx, raw_rx) = crossbeam_channel::bounded::<Vec<f32>>(16);
    let raw_tx = Arc::new(raw_tx);

    let stream = build_input_stream(&device, &config, sample_format, channels, raw_tx.clone())?;
    stream.play().context("start input stream")?;

    let join = std::thread::spawn(move || {
        let mut encoder = match opus_encoder(sample_rate) {
            Ok(enc) => enc,
            Err(err) => {
                eprintln!("opus encoder init failed: {err:#}");
                return;
            }
        };

        let frame_size = (sample_rate / 50) as usize;
        let mut pcm_buf: Vec<i16> = Vec::with_capacity(frame_size * 4);
        let mut out_buf = vec![0u8; 4096];

        while let Ok(chunk) = raw_rx.recv() {
            pcm_buf.extend(chunk.into_iter().map(f32_to_i16));

            while pcm_buf.len() >= frame_size {
                let frame: Vec<i16> = pcm_buf.drain(..frame_size).collect();
                let Ok(len) = encoder.encode(&frame, &mut out_buf) else {
                    continue;
                };
                let packet = out_buf[..len].to_vec();
                let _ = network_tx.try_send(NetworkCommand::SendAudio(packet));
            }
        }
    });

    Ok(AudioRecorder {
        stream: Some(stream),
        join: Some(join),
        sample_rate,
    })
}

fn pick_stream_config(device: &Device) -> anyhow::Result<(StreamConfig, SampleFormat, u32)> {
    let target_rates: [u32; 5] = [48000, 16000, 24000, 12000, 8000];

    if let Ok(mut ranges) = device.supported_input_configs() {
        for range in ranges.by_ref() {
            let min = range.min_sample_rate().0;
            let max = range.max_sample_rate().0;
            for rate in target_rates {
                if rate < min || rate > max {
                    continue;
                }
                let config = range.with_sample_rate(cpal::SampleRate(rate));
                return Ok((config.clone().into(), config.sample_format(), rate));
            }
        }
    }

    let default_config = device.default_input_config().context("default_input_config")?;
    let sample_rate = default_config.sample_rate().0;
    let sample_format = default_config.sample_format();

    if matches!(sample_rate, 8000 | 12000 | 16000 | 24000 | 48000) {
        return Ok((default_config.into(), sample_format, sample_rate));
    }

    Err(anyhow!(
        "unsupported input sample rate for opus: {sample_rate} (try setting device to 48kHz)"
    ))
}

fn build_input_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    raw_tx: Arc<crossbeam_channel::Sender<Vec<f32>>>,
) -> anyhow::Result<Stream> {
    let err_fn = move |err| eprintln!("cpal stream error: {err}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| push_mono(data, channels, &raw_tx),
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| push_mono(data, channels, &raw_tx),
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| push_mono(data, channels, &raw_tx),
            err_fn,
            None,
        )?,
        _ => return Err(anyhow!("unsupported sample format")),
    };

    Ok(stream)
}

fn push_mono<T: Sample>(data: &[T], channels: usize, raw_tx: &crossbeam_channel::Sender<Vec<f32>>) {
    if channels == 0 {
        return;
    }

    let mut mono = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks(channels) {
        mono.push(frame[0].to_f32());
    }

    let _ = raw_tx.try_send(mono);
}

fn opus_encoder(sample_rate: u32) -> anyhow::Result<Encoder> {
    let sr = match sample_rate {
        8000 => SampleRate::Hz8000,
        12000 => SampleRate::Hz12000,
        16000 => SampleRate::Hz16000,
        24000 => SampleRate::Hz24000,
        48000 => SampleRate::Hz48000,
        _ => return Err(anyhow!("unsupported sample rate for opus: {sample_rate}")),
    };

    Ok(Encoder::new(sr, Channels::Mono, Application::Voip)?)
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    (sample * i16::MAX as f32) as i16
}
