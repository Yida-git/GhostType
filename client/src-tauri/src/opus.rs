use anyhow::Context as _;

pub struct OpusEncoder {
    inner: imp::OpusEncoder,
}

// Opus encoder 只要不并发使用（我们保证单线程/单任务顺序调用），跨线程移动是安全的。
unsafe impl Send for OpusEncoder {}

impl OpusEncoder {
    pub fn new(sample_rate: u32) -> anyhow::Result<Self> {
        Ok(Self {
            inner: imp::OpusEncoder::new(sample_rate).context("init opus encoder")?,
        })
    }

    pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> anyhow::Result<usize> {
        self.inner.encode(pcm, out)
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use anyhow::anyhow;
    use audiopus::{coder::Encoder, Application, Channels, SampleRate};

    pub struct OpusEncoder {
        encoder: Encoder,
    }

    impl OpusEncoder {
        pub fn new(sample_rate: u32) -> anyhow::Result<Self> {
            let sr = match sample_rate {
                8000 => SampleRate::Hz8000,
                12000 => SampleRate::Hz12000,
                16000 => SampleRate::Hz16000,
                24000 => SampleRate::Hz24000,
                48000 => SampleRate::Hz48000,
                _ => return Err(anyhow!("unsupported sample rate for opus: {sample_rate}")),
            };

            Ok(Self {
                encoder: Encoder::new(sr, Channels::Mono, Application::Voip)?,
            })
        }

        pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> anyhow::Result<usize> {
            Ok(self.encoder.encode(pcm, out)?)
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use std::ffi::CStr;
    use std::ptr::NonNull;

    use anyhow::{anyhow, Context as _};
    use opus_sys as opus;

    const OPUS_APPLICATION_VOIP: i32 = 2048;

    pub struct OpusEncoder {
        encoder: NonNull<opus::OpusEncoder>,
        channels: i32,
    }

    impl OpusEncoder {
        pub fn new(sample_rate: u32) -> anyhow::Result<Self> {
            if !matches!(sample_rate, 8000 | 12000 | 16000 | 24000 | 48000) {
                return Err(anyhow!(
                    "不支持的 Opus 采样率 | Unsupported Opus sample rate: {} (支持 8000/12000/16000/24000/48000)",
                    sample_rate
                ));
            }

            let channels = 1i32;
            let mut err = 0i32;
            let encoder = unsafe {
                opus::opus_encoder_create(
                    sample_rate as i32,
                    channels,
                    OPUS_APPLICATION_VOIP,
                    &mut err as *mut i32,
                )
            };

            let encoder = NonNull::new(encoder).ok_or_else(|| anyhow!("opus encoder create returned null"))?;
            if err != opus::OPUS_OK {
                return Err(anyhow!("opus encoder create failed: {}", opus_error(err)));
            }

            Ok(Self { encoder, channels })
        }

        pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> anyhow::Result<usize> {
            if out.is_empty() {
                return Err(anyhow!("opus output buffer is empty"));
            }

            let channels = self.channels as usize;
            if channels == 0 {
                return Err(anyhow!("opus channels invalid"));
            }
            if pcm.len() % channels != 0 {
                return Err(anyhow!("opus pcm length {} not divisible by channels {}", pcm.len(), channels));
            }

            let frame_size = (pcm.len() / channels) as i32;
            if frame_size <= 0 {
                return Ok(0);
            }

            let encoded = unsafe {
                opus::opus_encode(
                    self.encoder.as_ptr(),
                    pcm.as_ptr(),
                    frame_size,
                    out.as_mut_ptr(),
                    out.len() as i32,
                )
            };

            if encoded < 0 {
                return Err(anyhow!("opus encode failed: {}", opus_error(encoded as i32)))
                    .context("opus_encode");
            }

            Ok(encoded as usize)
        }
    }

    impl Drop for OpusEncoder {
        fn drop(&mut self) {
            unsafe {
                opus::opus_encoder_destroy(self.encoder.as_ptr());
            }
        }
    }

    fn opus_error(code: i32) -> String {
        unsafe {
            let ptr = opus::opus_strerror(code);
            if ptr.is_null() {
                return format!("code={code}");
            }
            CStr::from_ptr(ptr).to_string_lossy().to_string()
        }
    }
}
