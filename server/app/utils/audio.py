from __future__ import annotations

import io
import secrets
import struct
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, List

try:
    import av  # type: ignore[import-not-found]
    import numpy as np  # type: ignore[import-not-found]
except Exception:  # noqa: BLE001 - optional at import time; raised on decode if missing
    av = None  # type: ignore[assignment]
    np = None  # type: ignore[assignment]

_OGG_CRC_POLY = 0x04C11DB7
_OGG_OPUS_GRANULE_RATE = 48000
_SUPPORTED_OPUS_RATES = (8000, 12000, 16000, 24000, 48000)


def _make_ogg_crc_table() -> List[int]:
    table: List[int] = []
    for i in range(256):
        r = i << 24
        for _ in range(8):
            if r & 0x8000_0000:
                r = ((r << 1) & 0xFFFF_FFFF) ^ _OGG_CRC_POLY
            else:
                r = (r << 1) & 0xFFFF_FFFF
        table.append(r)
    return table


_OGG_CRC_TABLE = _make_ogg_crc_table()


def _ogg_crc(page: bytes) -> int:
    crc = 0
    for b in page:
        crc = ((crc << 8) & 0xFFFF_FFFF) ^ _OGG_CRC_TABLE[((crc >> 24) & 0xFF) ^ b]
    return crc & 0xFFFF_FFFF


def _opus_head(*, input_sample_rate: int, channels: int = 1, pre_skip: int = 312) -> bytes:
    return (
        b"OpusHead"
        + bytes([1, channels])
        + struct.pack("<H", pre_skip)
        + struct.pack("<I", input_sample_rate)
        + struct.pack("<h", 0)
        + bytes([0])
    )


def _opus_tags(vendor: str = "GhostType") -> bytes:
    vendor_bytes = vendor.encode("utf-8", errors="strict")
    return (
        b"OpusTags"
        + struct.pack("<I", len(vendor_bytes))
        + vendor_bytes
        + struct.pack("<I", 0)
    )


def _segment_table_for_packet(packet_len: int) -> bytes:
    if packet_len < 0:
        raise ValueError("packet_len must be >= 0")

    lacing: List[int] = []
    remaining = packet_len
    while remaining >= 255:
        lacing.append(255)
        remaining -= 255
    if remaining > 0:
        lacing.append(remaining)
    else:
        lacing.append(0)
    return bytes(lacing)


def _build_ogg_page(
    *,
    header_type: int,
    granule_position: int,
    serial: int,
    sequence: int,
    packet: bytes,
) -> bytes:
    if granule_position < 0:
        raise ValueError("granule_position must be >= 0")

    lacing = _segment_table_for_packet(len(packet))
    if len(lacing) > 255:
        raise ValueError("packet too large for single Ogg page in this MVP writer")

    header = (
        b"OggS"
        + bytes([0, header_type & 0xFF])
        + struct.pack("<QII", granule_position, serial & 0xFFFF_FFFF, sequence & 0xFFFF_FFFF)
        + struct.pack("<I", 0)
        + bytes([len(lacing)])
        + lacing
    )

    page = header + packet
    crc = _ogg_crc(page)
    return page[:22] + struct.pack("<I", crc) + page[26:]


def packets_to_ogg_opus_bytes(opus_packets: Iterable[bytes], input_sample_rate: int) -> bytes:
    if input_sample_rate not in _SUPPORTED_OPUS_RATES:
        raise ValueError(
            f"不支持的采样率 | Unsupported sample rate: {input_sample_rate}. "
            f"支持 | Supported: {_SUPPORTED_OPUS_RATES}"
        )

    packets = list(opus_packets)
    serial = secrets.randbits(32)

    pre_skip = 312
    frame_samples = input_sample_rate // 50
    if frame_samples * 50 != input_sample_rate:
        raise ValueError(f"unsupported input_sample_rate for 20ms frames: {input_sample_rate}")
    if _OGG_OPUS_GRANULE_RATE % input_sample_rate != 0:
        raise ValueError(f"unsupported input_sample_rate for Ogg/Opus granules: {input_sample_rate}")
    granule_step = frame_samples * (_OGG_OPUS_GRANULE_RATE // input_sample_rate)
    out = bytearray()

    out += _build_ogg_page(
        header_type=0x02,
        granule_position=0,
        serial=serial,
        sequence=0,
        packet=_opus_head(input_sample_rate=input_sample_rate, channels=1, pre_skip=pre_skip),
    )
    out += _build_ogg_page(
        header_type=0x00,
        granule_position=0,
        serial=serial,
        sequence=1,
        packet=_opus_tags(),
    )

    granule = 0
    seq = 2
    for i, pkt in enumerate(packets):
        is_last = i == (len(packets) - 1)
        granule += granule_step
        effective_granule = max(0, granule - pre_skip)
        out += _build_ogg_page(
            header_type=0x04 if is_last else 0x00,
            granule_position=effective_granule,
            serial=serial,
            sequence=seq,
            packet=pkt,
        )
        seq += 1

    return bytes(out)


@dataclass(frozen=True)
class PcmAudio:
    pcm_s16le: bytes
    sample_rate: int
    channels: int = 1


def write_wav_s16le(path: Path, *, pcm_s16le: bytes, sample_rate: int, channels: int = 1) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate)
        wav.writeframes(pcm_s16le)


def decode_opus_packets_to_pcm_s16le(
    opus_packets: Iterable[bytes],
    *,
    input_sample_rate: int,
    output_sample_rate: int = 16000,
) -> PcmAudio:
    packets = list(opus_packets)
    if not packets:
        return PcmAudio(pcm_s16le=b"", sample_rate=output_sample_rate, channels=1)

    ogg_bytes = packets_to_ogg_opus_bytes(packets, input_sample_rate=input_sample_rate)
    if av is None or np is None:
        raise RuntimeError("PyAV not installed; add `av` + `numpy` to your environment")

    pcm_chunks: List[bytes] = []
    with av.open(io.BytesIO(ogg_bytes), mode="r", format="ogg") as container:
        resampler = av.AudioResampler(format="s16", layout="mono", rate=output_sample_rate)
        for frame in container.decode(audio=0):
            frame.pts = None
            resampled_frames = resampler.resample(frame)
            if resampled_frames is None:
                continue
            if not isinstance(resampled_frames, (list, tuple)):
                resampled_frames = [resampled_frames]
            for r in resampled_frames:
                pcm_chunks.append(r.to_ndarray().tobytes())

        flushed = resampler.resample(None)
        if flushed is not None:
            if not isinstance(flushed, (list, tuple)):
                flushed = [flushed]
            for r in flushed:
                pcm_chunks.append(r.to_ndarray().tobytes())

    return PcmAudio(
        pcm_s16le=b"".join(pcm_chunks),
        sample_rate=output_sample_rate,
        channels=1,
    )
