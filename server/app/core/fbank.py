from __future__ import annotations

from functools import lru_cache
from typing import Tuple

import numpy as np


@lru_cache(maxsize=32)
def _mel_filterbank(
    *,
    sample_rate: int,
    n_fft: int,
    n_mels: int,
    f_min: float,
    f_max: float,
) -> np.ndarray:
    n_freq = n_fft // 2 + 1

    def hz_to_mel(hz: np.ndarray) -> np.ndarray:
        return 1127.0 * np.log1p(hz / 700.0)

    def mel_to_hz(mel: np.ndarray) -> np.ndarray:
        return 700.0 * (np.expm1(mel / 1127.0))

    mel_min = hz_to_mel(np.array([f_min], dtype=np.float32))[0]
    mel_max = hz_to_mel(np.array([f_max], dtype=np.float32))[0]

    mels = np.linspace(mel_min, mel_max, n_mels + 2, dtype=np.float32)
    hz = mel_to_hz(mels)
    bins = np.floor((n_fft + 1) * hz / sample_rate).astype(np.int32)
    bins = np.clip(bins, 0, n_freq - 1)

    fb = np.zeros((n_mels, n_freq), dtype=np.float32)
    for m in range(1, n_mels + 1):
        left = int(bins[m - 1])
        center = int(bins[m])
        right = int(bins[m + 1])
        if center == left or right == center:
            continue
        fb[m - 1, left:center] = (np.arange(left, center) - left) / float(center - left)
        fb[m - 1, center:right] = (right - np.arange(center, right)) / float(right - center)
    return fb


@lru_cache(maxsize=32)
def _hamming_window(frame_length: int) -> np.ndarray:
    return np.hamming(frame_length).astype(np.float32)


def _frame_waveform(
    waveform: np.ndarray, *, frame_length: int, frame_shift: int
) -> np.ndarray:
    if waveform.ndim != 1:
        raise ValueError("waveform must be 1-D")
    if frame_length <= 0 or frame_shift <= 0:
        raise ValueError("frame_length/frame_shift must be > 0")

    if waveform.size < frame_length:
        waveform = np.pad(waveform, (0, frame_length - waveform.size), mode="constant")

    num_frames = 1 + (waveform.size - frame_length) // frame_shift
    total_len = (num_frames - 1) * frame_shift + frame_length
    if waveform.size < total_len:
        waveform = np.pad(waveform, (0, total_len - waveform.size), mode="constant")

    item_stride = waveform.strides[0]
    frames = np.lib.stride_tricks.as_strided(
        waveform,
        shape=(num_frames, frame_length),
        strides=(frame_shift * item_stride, item_stride),
    )
    return np.array(frames, dtype=np.float32, copy=True)


def log_mel_fbank(
    waveform: np.ndarray,
    *,
    sample_rate: int,
    n_mels: int = 80,
    frame_length_ms: float = 25.0,
    frame_shift_ms: float = 10.0,
    n_fft: int = 512,
    f_min: float = 0.0,
    f_max: float | None = None,
) -> np.ndarray:
    if f_max is None:
        f_max = sample_rate / 2.0

    frame_length = int(round(sample_rate * frame_length_ms / 1000.0))
    frame_shift = int(round(sample_rate * frame_shift_ms / 1000.0))
    if frame_length <= 0 or frame_shift <= 0:
        raise ValueError("invalid frame_length_ms/frame_shift_ms")

    frames = _frame_waveform(waveform, frame_length=frame_length, frame_shift=frame_shift)
    frames *= _hamming_window(frame_length)[None, :]

    spec = np.fft.rfft(frames, n=n_fft, axis=1)
    power = (spec.real**2 + spec.imag**2).astype(np.float32)

    fb = _mel_filterbank(
        sample_rate=sample_rate, n_fft=n_fft, n_mels=n_mels, f_min=float(f_min), f_max=float(f_max)
    )
    mel = power @ fb.T
    mel = np.maximum(mel, 1e-10).astype(np.float32)
    return np.log(mel).astype(np.float32)


def apply_lfr(features: np.ndarray, *, lfr_m: int, lfr_n: int) -> np.ndarray:
    if features.ndim != 2:
        raise ValueError("features must be 2-D [T, C]")
    if lfr_m <= 0 or lfr_n <= 0:
        raise ValueError("lfr_m/lfr_n must be > 0")

    t, c = features.shape
    if t == 0:
        return np.zeros((0, c * lfr_m), dtype=np.float32)

    out = []
    idx = 0
    while idx < t:
        chunk = features[idx : idx + lfr_m]
        if chunk.shape[0] < lfr_m:
            pad = np.repeat(chunk[-1:, :], lfr_m - chunk.shape[0], axis=0)
            chunk = np.concatenate([chunk, pad], axis=0)
        out.append(chunk.reshape(-1))
        idx += lfr_n

    return np.stack(out, axis=0).astype(np.float32)


def apply_cmvn(
    features: np.ndarray, *, neg_mean: np.ndarray, inv_stddev: np.ndarray
) -> np.ndarray:
    if features.ndim != 2:
        raise ValueError("features must be 2-D [T, C]")
    if neg_mean.ndim != 1 or inv_stddev.ndim != 1:
        raise ValueError("cmvn vectors must be 1-D")
    if features.shape[1] != neg_mean.shape[0] or features.shape[1] != inv_stddev.shape[0]:
        raise ValueError("cmvn vectors must match feature dimension")

    return ((features + neg_mean[None, :]) * inv_stddev[None, :]).astype(np.float32)


def sensevoice_ctc_features(
    pcm_s16le: bytes,
    *,
    sample_rate: int,
    n_mels: int,
    lfr_m: int,
    lfr_n: int,
    cmvn_neg_mean: np.ndarray,
    cmvn_inv_stddev: np.ndarray,
) -> Tuple[np.ndarray, int]:
    waveform = np.frombuffer(pcm_s16le, dtype=np.int16).astype(np.float32)

    fbanks = log_mel_fbank(waveform, sample_rate=sample_rate, n_mels=n_mels)
    lfr = apply_lfr(fbanks, lfr_m=lfr_m, lfr_n=lfr_n)
    feats = apply_cmvn(lfr, neg_mean=cmvn_neg_mean, inv_stddev=cmvn_inv_stddev)
    return feats, int(feats.shape[0])
