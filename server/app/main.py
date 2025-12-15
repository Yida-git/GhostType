import asyncio
import json
import os
import tempfile
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, Optional

from fastapi import FastAPI, WebSocket, WebSocketDisconnect

from app.core.asr import AsrEngine, SenseVoiceEngine, StubAsrEngine
from app.utils.audio import decode_opus_packets_to_pcm_s16le, write_wav_s16le

app = FastAPI()

MODEL_PATH = Path(__file__).parent.parent / "models" / "sensevoice-small.onnx"
asr_engine: AsrEngine = StubAsrEngine()


@app.on_event("startup")
async def startup() -> None:
    global asr_engine

    if MODEL_PATH.exists():
        try:
            asr_engine = SenseVoiceEngine(MODEL_PATH)
            print(f"ASR engine loaded: {MODEL_PATH}")
        except Exception as exc:
            asr_engine = StubAsrEngine()
            print(f"WARNING: failed to load ASR model ({exc}); using stub")
    else:
        print(f"WARNING: ASR model not found at {MODEL_PATH}, using stub")


@dataclass
class SessionState:
    sample_rate: Optional[int] = None
    context: Dict[str, Any] = field(default_factory=dict)
    use_cloud_api: bool = False
    opus_packets: list[bytes] = field(default_factory=list)

    def reset_audio(self) -> None:
        self.opus_packets.clear()


def _json_dumps(obj: Any) -> str:
    return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))


async def _send_error(ws: WebSocket, message: str) -> None:
    await ws.send_text(_json_dumps({"type": "error", "message": message}))


@app.get("/")
def health() -> Dict[str, str]:
    return {"status": "GhostType Server Running"}


@app.websocket("/ws")
async def websocket_endpoint(ws: WebSocket) -> None:
    await ws.accept()

    state = SessionState()
    stop_lock = asyncio.Lock()

    async def handle_stop() -> None:
        async with stop_lock:
            if state.sample_rate is None:
                await _send_error(ws, "stop before start")
                state.reset_audio()
                return

            try:
                pcm = await asyncio.to_thread(
                    decode_opus_packets_to_pcm_s16le,
                    state.opus_packets,
                    input_sample_rate=state.sample_rate,
                )
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
                    print(f"dumped wav: {dump_path}")

                try:
                    text = await asr_engine.transcribe(pcm.pcm_s16le, pcm.sample_rate)
                except Exception as exc:
                    await _send_error(ws, f"asr failed: {exc}")
                    text = f"[asr_error: {exc}]"

                await ws.send_text(
                    _json_dumps({"type": "fast_text", "content": text, "is_final": True})
                )
            except Exception as exc:
                await _send_error(ws, f"audio decode failed: {exc}")
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
                    await _send_error(ws, "invalid json")
                    continue

                msg_type = payload.get("type")
                if msg_type == "ping":
                    await ws.send_text(_json_dumps({"type": "pong"}))
                    continue

                if msg_type == "start":
                    state.sample_rate = int(payload.get("sample_rate", 48000))
                    state.context = dict(payload.get("context") or {})
                    state.use_cloud_api = bool(payload.get("use_cloud_api", False))
                    state.reset_audio()
                    continue

                if msg_type == "stop":
                    await handle_stop()
                    continue

                await _send_error(ws, f"unknown type: {msg_type}")
                continue

            audio = msg.get("bytes")
            if audio is not None:
                state.opus_packets.append(audio)
                continue

    except WebSocketDisconnect:
        return
