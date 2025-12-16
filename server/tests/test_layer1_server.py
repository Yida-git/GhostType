#!/usr/bin/env python3
"""
Layer 1: 服务端单独测试

运行前准备（推荐使用虚拟环境）:
  cd server
  python -m venv .venv

  # macOS/Linux:
  source .venv/bin/activate
  pip install -r requirements-macos.txt   # macOS
  # 或 pip install -r requirements.txt     # Windows/DirectML

  # Windows PowerShell:
  .venv\\Scripts\\Activate.ps1
  pip install -r requirements.txt

运行前先启动服务端:
  cd server
  uvicorn app.main:app --host 0.0.0.0 --port 8000

然后运行:
  python tests/test_layer1_server.py
"""

from __future__ import annotations

import asyncio
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

SERVER_HTTP = "http://localhost:8000"
WS_URL = "ws://localhost:8000/ws"

FIRST_INFERENCE_TIMEOUT = 15.0
NORMAL_TIMEOUT = 5.0


def _configure_stdout() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
        except Exception:
            pass


class TestResult:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.warnings = 0
        self.results: list[tuple[str, str, str]] = []

    def ok(self, name: str, msg: str = "") -> None:
        self.results.append(("✅", name, msg))
        self.passed += 1

    def fail(self, name: str, msg: str = "") -> None:
        self.results.append(("❌", name, msg))
        self.failed += 1

    def warn(self, name: str, msg: str = "") -> None:
        self.results.append(("⚠️", name, msg))
        self.warnings += 1


async def test_health_check(result: TestResult) -> None:
    """T1.1: HTTP 健康检查"""
    print("\nT1.1: HTTP 健康检查")
    try:
        import httpx

        async with httpx.AsyncClient() as client:
            resp = await client.get(f"{SERVER_HTTP}/", timeout=5.0)

        if resp.status_code == 200:
            print(f"  响应: {resp.json()}")
            result.ok("T1.1 健康检查")
        else:
            result.fail("T1.1 健康检查", f"status={resp.status_code}")
    except Exception as exc:
        result.fail("T1.1 健康检查", str(exc))


async def test_websocket_connect(result: TestResult) -> None:
    """T1.2: WebSocket 连接"""
    print("\nT1.2: WebSocket 连接")
    try:
        import websockets

        async with websockets.connect(WS_URL) as ws:
            _ = ws
        result.ok("T1.2 WebSocket连接")
    except Exception as exc:
        result.fail("T1.2 WebSocket连接", str(exc))


async def test_ping_pong(result: TestResult) -> None:
    """T1.3: Ping/Pong 协议"""
    print("\nT1.3: Ping/Pong 协议")
    try:
        import websockets

        async with websockets.connect(WS_URL) as ws:
            await ws.send(json.dumps({"type": "ping"}))
            response = await asyncio.wait_for(ws.recv(), timeout=NORMAL_TIMEOUT)
            data = json.loads(response)
            if data.get("type") == "pong":
                result.ok("T1.3 Ping/Pong")
            else:
                result.fail("T1.3 Ping/Pong", f"unexpected: {data}")
    except Exception as exc:
        result.fail("T1.3 Ping/Pong", str(exc))


async def test_start_stop_flow(result: TestResult) -> None:
    """T1.4: Start/Stop 流程 (无音频)"""
    print("\nT1.4: Start/Stop 流程 (无音频)")
    try:
        import websockets

        async with websockets.connect(WS_URL) as ws:
            await ws.send(
                json.dumps(
                    {
                        "type": "start",
                        "sample_rate": 48000,
                        "context": {"app_name": "TestScript", "window_title": "test"},
                        "use_cloud_api": False,
                    },
                    ensure_ascii=False,
                )
            )
            await ws.send(json.dumps({"type": "stop"}))

            try:
                response = await asyncio.wait_for(ws.recv(), timeout=NORMAL_TIMEOUT)
                data = json.loads(response)
                if data.get("type") in ("fast_text", "error"):
                    result.ok("T1.4 Start/Stop", f"type={data.get('type')}")
                else:
                    result.warn("T1.4 Start/Stop", f"unexpected: {data.get('type')}")
            except asyncio.TimeoutError:
                result.ok("T1.4 Start/Stop", "empty audio - no response")
    except Exception as exc:
        result.fail("T1.4 Start/Stop", str(exc))


async def test_audio_decode_pipeline(result: TestResult) -> None:
    """T1.5: 音频解码管道测试"""
    print("\nT1.5: 音频解码管道")
    try:
        from app.utils.audio import PcmAudio, decode_opus_packets_to_pcm_s16le

        pcm = decode_opus_packets_to_pcm_s16le([], input_sample_rate=48000, output_sample_rate=16000)
        assert isinstance(pcm, PcmAudio)
        assert pcm.sample_rate == 16000
        assert pcm.channels == 1
        result.ok("T1.5 音频解码", f"pcm_bytes={len(pcm.pcm_s16le)}")
    except Exception as exc:
        result.fail("T1.5 音频解码", str(exc))


async def test_asr_engine_load(result: TestResult) -> object | None:
    """T1.6: ASR 引擎加载 + Warmup"""
    print("\nT1.6: ASR 引擎加载")

    model_path = Path(__file__).parent.parent / "models" / "sensevoice-small.onnx"
    if not model_path.exists():
        result.warn("T1.6 ASR加载", "model missing")
        return None

    try:
        import numpy as np

        from app.core.asr import SenseVoiceEngine

        t0 = time.time()
        engine = SenseVoiceEngine(model_path)
        load_time = time.time() - t0

        providers = getattr(engine, "providers", None) or engine.session.get_providers()
        mode = getattr(engine, "_mode", "unknown")
        print(f"  model: {model_path.name}")
        print(f"  load: {load_time:.2f}s")
        print(f"  providers: {providers}")
        print(f"  mode: {mode}")

        print("  warmup...")
        silence = np.zeros(16000, dtype=np.int16).tobytes()
        t0 = time.time()
        _ = await asyncio.wait_for(engine.transcribe(silence, 16000), timeout=FIRST_INFERENCE_TIMEOUT)
        warmup_time = time.time() - t0
        result.ok("T1.6 ASR加载", f"warmup={warmup_time:.2f}s")
        return engine
    except asyncio.TimeoutError:
        result.fail("T1.6 ASR加载", f"warmup timeout >{FIRST_INFERENCE_TIMEOUT}s")
        return None
    except Exception as exc:
        result.fail("T1.6 ASR加载", str(exc))
        return None


async def test_asr_inference(result: TestResult, engine: object | None) -> None:
    """T1.7: ASR 推理测试"""
    print("\nT1.7: ASR 推理测试")
    if engine is None:
        result.warn("T1.7 ASR推理", "engine not loaded")
        return

    try:
        import numpy as np

        sample_rate = 16000
        noise = np.random.randint(-100, 100, sample_rate, dtype=np.int16).tobytes()

        t0 = time.time()
        text = await asyncio.wait_for(engine.transcribe(noise, sample_rate), timeout=NORMAL_TIMEOUT)  # type: ignore[attr-defined]
        dt = time.time() - t0
        preview = text[:50].replace("\n", " ")
        print(f"  text: '{preview}{'...' if len(text) > 50 else ''}'")
        print(f"  time: {dt:.3f}s")
        result.ok("T1.7 ASR推理", f"{dt:.3f}s")
    except asyncio.TimeoutError:
        result.fail("T1.7 ASR推理", f"timeout >{NORMAL_TIMEOUT}s")
    except Exception as exc:
        result.fail("T1.7 ASR推理", str(exc))


async def main() -> None:
    _configure_stdout()

    print("=" * 60)
    print("Layer 1: 服务端单独测试")
    print("=" * 60)
    print("⚠️  请确保服务端已启动:")
    print("    cd server")
    print("    uvicorn app.main:app --host 0.0.0.0 --port 8000")
    print("\n提示: 建议先激活虚拟环境并安装依赖:")
    print("    python -m venv .venv")
    print("    source .venv/bin/activate          # macOS/Linux")
    print("    pip install -r requirements-macos.txt  # macOS")
    print("    # Windows: pip install -r requirements.txt")

    if sys.stdin.isatty():
        try:
            input("\n按 Enter 开始测试...")
        except EOFError:
            pass

    result = TestResult()

    await test_health_check(result)
    await test_websocket_connect(result)
    await test_ping_pong(result)
    await test_start_stop_flow(result)
    await test_audio_decode_pipeline(result)

    engine = await test_asr_engine_load(result)
    await test_asr_inference(result, engine)

    print("\n" + "=" * 60)
    print("测试结果汇总")
    print("=" * 60)
    for status, name, msg in result.results:
        suffix = f" ({msg})" if msg else ""
        print(f"  {status} {name}{suffix}")
    print()
    print(f"通过: {result.passed}  失败: {result.failed}  警告: {result.warnings}")
    print("=" * 60)

    if result.failed > 0:
        raise SystemExit(1)
    raise SystemExit(0)


if __name__ == "__main__":
    asyncio.run(main())
