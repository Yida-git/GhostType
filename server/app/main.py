import asyncio
import json
import os
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, Optional

from fastapi import FastAPI, WebSocket, WebSocketDisconnect

from app.logging_config import get_logger, setup_logging, with_trace

setup_logging()
log_server = get_logger("server")
log_ws = get_logger("ws")
log_audio = get_logger("audio")
log_asr = get_logger("asr")

from app.core.asr import AsrEngine, SenseVoiceEngine, StubAsrEngine
from app.utils.audio import decode_opus_packets_to_pcm_s16le, write_wav_s16le

app = FastAPI()

MODEL_PATH = Path(__file__).parent.parent / "models" / "sensevoice-small.onnx"
asr_engine: AsrEngine = StubAsrEngine()


@app.on_event("startup")
async def startup() -> None:
    global asr_engine

    t_start = time.perf_counter()
    log_server.info("服务器启动 | Server starting")

    if MODEL_PATH.exists():
        size_mb = MODEL_PATH.stat().st_size / (1024 * 1024)
        log_asr.info(
            "ASR模型加载中 | Loading ASR model | model={model} size_mb={size_mb:.1f}",
            model=str(MODEL_PATH),
            size_mb=size_mb,
        )
        try:
            t0 = time.perf_counter()
            asr_engine = SenseVoiceEngine(MODEL_PATH)
            load_ms = (time.perf_counter() - t0) * 1000.0
            providers = getattr(asr_engine, "providers", None)
            if providers:
                server_root = Path(__file__).parent.parent
                try:
                    model_display = MODEL_PATH.relative_to(server_root)
                except ValueError:
                    model_display = MODEL_PATH
                log_asr.info(
                    "ASR模型已加载 | ASR model loaded | model={model} providers={providers} load_time_ms={load_ms:.0f}",
                    model=str(model_display),
                    providers=providers,
                    load_ms=load_ms,
                )
            else:
                log_asr.info(
                    "ASR模型已加载 | ASR model loaded | model={model} load_time_ms={load_ms:.0f}",
                    model=str(MODEL_PATH),
                    load_ms=load_ms,
                )
        except Exception as exc:
            asr_engine = StubAsrEngine()
            log_asr.exception(
                "ASR模型加载失败，使用Stub | Failed to load ASR model; using stub | model={model} error={error}",
                model=str(MODEL_PATH),
                error=str(exc),
            )
    else:
        log_asr.warning(
            "ASR模型不存在，使用Stub | ASR model not found; using stub | model={model}",
            model=str(MODEL_PATH),
        )

    startup_ms = (time.perf_counter() - t_start) * 1000.0
    log_server.info("服务器就绪 | Server ready | startup_time_ms={ms:.0f}", ms=startup_ms)


@dataclass
class SessionState:
    trace_id: Optional[str] = None
    sample_rate: Optional[int] = None
    context: Dict[str, Any] = field(default_factory=dict)
    use_cloud_api: bool = False
    opus_packets: list[bytes] = field(default_factory=list)
    packet_count: int = 0
    total_bytes: int = 0

    def reset_audio(self) -> None:
        self.opus_packets.clear()
        self.packet_count = 0
        self.total_bytes = 0


def _json_dumps(obj: Any) -> str:
    return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))


def _generate_trace_id() -> str:
    alphabet = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
    micros = int(time.time_ns() // 1000)
    n = micros & 0xFFFFFFFF
    out = []
    for _ in range(6):
        out.append(alphabet[n % 62])
        n //= 62
    return "".join(reversed(out))


async def _send_error(ws: WebSocket, message: str, *, trace_id: Optional[str] = None) -> None:
    payload: Dict[str, Any] = {"type": "error", "message": message}
    if trace_id:
        payload["trace_id"] = trace_id
    await ws.send_text(_json_dumps(payload))


@app.get("/")
def health() -> Dict[str, str]:
    return {"status": "ok"}


@app.websocket("/ws")
async def websocket_endpoint(ws: WebSocket) -> None:
    await ws.accept()

    client_ip = getattr(getattr(ws, "client", None), "host", None) or ""
    log_ws.info("客户端已连接 | Client connected | client_ip={client_ip}", client_ip=client_ip)

    state = SessionState()
    stop_lock = asyncio.Lock()

    async def handle_stop() -> None:
        async with stop_lock:
            if state.sample_rate is None:
                log_ws.warning("收到stop但未start | Stop before start")
                await _send_error(ws, "stop before start", trace_id=state.trace_id)
                state.reset_audio()
                return

            trace_id = state.trace_id or ""
            tlog_audio = with_trace(log_audio, trace_id)
            tlog_asr = with_trace(log_asr, trace_id)

            try:
                t0 = time.perf_counter()
                packets = state.packet_count
                total_bytes = state.total_bytes
                pcm = await asyncio.to_thread(
                    decode_opus_packets_to_pcm_s16le,
                    state.opus_packets,
                    input_sample_rate=state.sample_rate,
                )
                t1 = time.perf_counter()
                if os.environ.get("GHOSTTYPE_DUMP_WAV"):
                    dump_dir = Path(
                        os.environ.get("GHOSTTYPE_DUMP_WAV_DIR") or tempfile.gettempdir()
                    )
                    dump_name = datetime.now().strftime("ghosttype_%Y%m%d_%H%M%S_%f.wav")
                    dump_path = dump_dir / dump_name
                    await asyncio.to_thread(
                        write_wav_s16le,
                        dump_path,
                        pcm_s16le=pcm.pcm_s16le,
                        sample_rate=pcm.sample_rate,
                        channels=pcm.channels,
                    )
                    tlog_audio.info(
                        "已保存WAV | WAV dumped | path={path}", path=str(dump_path)
                    )

                try:
                    t_asr0 = time.perf_counter()
                    pcm_samples = len(pcm.pcm_s16le) // 2
                    pcm_duration_ms = (
                        (pcm_samples / max(pcm.sample_rate, 1)) * 1000.0
                        if pcm_samples > 0
                        else 0.0
                    )
                    decode_ms = (t1 - t0) * 1000.0
                    tlog_audio.info(
                        "音频解码完成 | Audio decoded | pcm_samples={samples} decode_time_ms={ms:.0f} packets={packets} total_bytes={bytes} sample_rate={sr}",
                        samples=pcm_samples,
                        ms=decode_ms,
                        packets=packets,
                        bytes=total_bytes,
                        sr=pcm.sample_rate,
                    )

                    tlog_asr.info(
                        "ASR推理开始 | ASR inference started | pcm_duration_ms={dur:.0f}",
                        dur=pcm_duration_ms,
                    )
                    text = await asr_engine.transcribe(pcm.pcm_s16le, pcm.sample_rate)
                    t_asr1 = time.perf_counter()
                except Exception as exc:
                    await _send_error(ws, f"asr failed: {exc}", trace_id=state.trace_id)
                    text = f"[asr_error: {exc}]"

                asr_ms = (t_asr1 - t_asr0) * 1000.0
                total_ms = (t_asr1 - t0) * 1000.0
                preview = text.replace("\n", " ")[:80]
                tlog_asr.info(
                    "ASR推理完成 | ASR inference completed | text={text} inference_time_ms={ms:.0f} total_ms={total:.0f}",
                    text=preview,
                    ms=asr_ms,
                    total=total_ms,
                )
                if os.environ.get("GHOSTTYPE_LOG_TIMINGS"):
                    with_trace(log_asr, trace_id).info(
                        "timings: decode={decode_ms:.0f}ms asr={asr_ms:.0f}ms total={total_ms:.0f}ms pcm_bytes={pcm_bytes} sr={sr}",
                        decode_ms=decode_ms,
                        asr_ms=asr_ms,
                        total_ms=total_ms,
                        pcm_bytes=len(pcm.pcm_s16le),
                        sr=pcm.sample_rate,
                    )

                await ws.send_text(
                    _json_dumps(
                        {
                            "type": "fast_text",
                            "trace_id": state.trace_id,
                            "content": text,
                            "is_final": True,
                        }
                    )
                )
                with_trace(log_ws, trace_id).debug(
                    "发送识别结果 | Sending recognition result | text_len={len}",
                    len=len(text),
                )
            except Exception as exc:
                with_trace(log_audio, state.trace_id or "").exception(
                    "音频解码失败 | Audio decode failed | error={error}", error=str(exc)
                )
                await _send_error(ws, f"audio decode failed: {exc}", trace_id=state.trace_id)
            finally:
                state.reset_audio()

    try:
        while True:
            msg = await ws.receive()

            text = msg.get("text")
            if text is not None:
                try:
                    payload = json.loads(text)
                except json.JSONDecodeError:
                    await _send_error(ws, "invalid json", trace_id=state.trace_id)
                    continue

                msg_type = payload.get("type")
                if msg_type == "ping":
                    await ws.send_text(_json_dumps({"type": "pong"}))
                    continue

                if msg_type == "start":
                    trace_id = payload.get("trace_id")
                    if isinstance(trace_id, str) and trace_id.strip():
                        state.trace_id = trace_id.strip()
                    else:
                        state.trace_id = _generate_trace_id()
                    state.sample_rate = int(payload.get("sample_rate", 48000))
                    state.context = dict(payload.get("context") or {})
                    state.use_cloud_api = bool(payload.get("use_cloud_api", False))
                    state.reset_audio()
                    with_trace(log_ws, state.trace_id).debug(
                        "收到控制消息 | Control message received | type=start sample_rate={sr}",
                        sr=state.sample_rate,
                    )
                    continue

                if msg_type == "stop":
                    with_trace(log_ws, state.trace_id or "").debug(
                        "收到控制消息 | Control message received | type=stop"
                    )
                    await handle_stop()
                    continue

                await _send_error(ws, f"unknown type: {msg_type}", trace_id=state.trace_id)
                continue

            audio = msg.get("bytes")
            if audio is not None:
                state.opus_packets.append(audio)
                state.packet_count += 1
                state.total_bytes += len(audio)
                with_trace(log_audio, state.trace_id or "").debug(
                    "音频包已接收 | Audio packet received | bytes={bytes} total_packets={packets}",
                    bytes=len(audio),
                    packets=state.packet_count,
                )
                continue

    except WebSocketDisconnect:
        log_ws.info("客户端断开 | Client disconnected | client_ip={client_ip}", client_ip=client_ip)
        return
    except RuntimeError as exc:
        # Starlette raises a RuntimeError when `receive()` is called after a disconnect
        # message has already been processed.
        if 'disconnect message has been received' in str(exc):
            return
        raise
