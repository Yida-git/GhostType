from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Protocol

try:
    import numpy as np  # type: ignore[import-not-found]
    import onnxruntime as ort  # type: ignore[import-not-found]
except Exception:  # noqa: BLE001 - optional at import time; raised on engine init
    np = None  # type: ignore[assignment]
    ort = None  # type: ignore[assignment]


class AsrEngine(Protocol):
    async def transcribe(self, audio_pcm: bytes, sample_rate: int) -> str: ...


class StubAsrEngine:
    async def transcribe(self, audio_pcm: bytes, sample_rate: int) -> str:
        return f"[pcm_bytes={len(audio_pcm)} sr={sample_rate}]"


@dataclass(frozen=True)
class SenseVoiceConfig:
    sample_rate: int = 16000


class SenseVoiceEngine:
    def __init__(
        self,
        model_path: Path,
        *,
        config: Optional[SenseVoiceConfig] = None,
        preferred_providers: Optional[List[str]] = None,
    ) -> None:
        if ort is None or np is None:
            raise RuntimeError(
                "onnxruntime/numpy not installed; add `onnxruntime-directml` (Windows) or `onnxruntime-gpu` (CUDA) or `onnxruntime` + `numpy`"
            )

        self.model_path = Path(model_path)
        if not self.model_path.exists():
            raise FileNotFoundError(str(self.model_path))

        self.config = config or SenseVoiceConfig()

        available = ort.get_available_providers()
        available_set = set(available)
        preferred = preferred_providers or [
            "CUDAExecutionProvider",
            "DmlExecutionProvider",
            "CPUExecutionProvider",
        ]
        providers = [p for p in preferred if p in available_set]
        if not providers:
            providers = available

        sess_options = ort.SessionOptions()
        sess_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL

        self.session = ort.InferenceSession(str(self.model_path), sess_options=sess_options, providers=providers)
        self._input_waveform_name, self._input_waveform_type = self._pick_waveform_input()
        self._input_length = self._pick_length_input()
        self._token_list = self._load_token_list()

    async def transcribe(self, audio_pcm: bytes, sample_rate: int) -> str:
        return await asyncio.to_thread(self._transcribe_sync, audio_pcm, sample_rate)

    def _pick_waveform_input(self) -> tuple[str, str]:
        inputs = self.session.get_inputs()
        for inp in inputs:
            if "tensor(float" in inp.type:
                return inp.name, inp.type
        if inputs:
            return inputs[0].name, inputs[0].type
        raise RuntimeError("onnx model has no inputs")

    def _pick_length_input(self) -> Optional[tuple[str, str]]:
        for inp in self.session.get_inputs():
            if inp.name == self._input_waveform_name:
                continue
            if "tensor(int" in inp.type:
                return inp.name, inp.type
        return None

    def _load_token_list(self) -> Optional[List[str]]:
        token_list = self._load_token_list_from_model_metadata()
        if token_list:
            return token_list

        candidates = [
            self.model_path.with_suffix(".tokens.txt"),
            self.model_path.with_suffix(".txt"),
            self.model_path.parent / "tokens.txt",
            self.model_path.parent / "token_list.txt",
            self.model_path.parent / "vocab.txt",
        ]
        for path in candidates:
            try:
                content = path.read_text(encoding="utf-8")
            except OSError:
                continue
            tokens = [line.rstrip("\n") for line in content.splitlines() if line.strip() != ""]
            if tokens:
                return tokens
        return None

    def _load_token_list_from_model_metadata(self) -> Optional[List[str]]:
        try:
            meta = self.session.get_modelmeta()
            custom = getattr(meta, "custom_metadata_map", None) or {}
        except Exception:
            return None

        for key in ("token_list", "tokens", "vocab", "char_list"):
            raw = custom.get(key)
            if not raw:
                continue
            raw = raw.strip()
            if raw.startswith("["):
                try:
                    tokens = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                if isinstance(tokens, list) and all(isinstance(t, str) for t in tokens):
                    return tokens
            tokens = [line.rstrip("\n") for line in raw.splitlines() if line.strip() != ""]
            if tokens:
                return tokens
        return None

    def _transcribe_sync(self, audio_pcm: bytes, sample_rate: int) -> str:
        if np is None:
            raise RuntimeError("numpy not installed")

        if sample_rate != self.config.sample_rate:
            return f"[unsupported sample_rate={sample_rate}; expected {self.config.sample_rate}]"

        waveform = np.frombuffer(audio_pcm, dtype=np.int16).astype(np.float32) / 32768.0
        waveform = np.ascontiguousarray(waveform)

        input_wave = waveform
        if "float16" in self._input_waveform_type:
            input_wave = input_wave.astype(np.float16)

        base_inputs: Dict[str, Any] = {}
        if self._input_length is not None:
            name, typ = self._input_length
            if "int64" in typ:
                base_inputs[name] = np.array([waveform.shape[0]], dtype=np.int64)
            else:
                base_inputs[name] = np.array([waveform.shape[0]], dtype=np.int32)

        last_exc: Optional[Exception] = None
        for wave_in in (input_wave[None, :], input_wave, input_wave[None, None, :]):
            inputs: Dict[str, Any] = dict(base_inputs)
            inputs[self._input_waveform_name] = wave_in
            try:
                outputs = self.session.run(None, inputs)
                return self._decode_outputs(outputs)
            except Exception as exc:
                last_exc = exc

        raise RuntimeError(f"onnx inference failed: {last_exc}")

    def _decode_outputs(self, outputs: List[Any]) -> str:
        if np is None:
            raise RuntimeError("numpy not installed")

        for out in outputs:
            if isinstance(out, np.ndarray) and out.dtype.kind in {"O", "S", "U"}:
                val = out.flatten()[0]
                if isinstance(val, (bytes, bytearray)):
                    try:
                        return val.decode("utf-8", errors="ignore")
                    except Exception:
                        return str(val)
                return str(val)

        token_ids = self._extract_token_ids(outputs)
        if token_ids is None:
            return "[asr_output_unhandled]"

        if not self._token_list:
            return f"[token_ids={token_ids[:64]}{'...' if len(token_ids) > 64 else ''}]"

        return self._decode_token_ids(token_ids, self._token_list)

    def _extract_token_ids(self, outputs: List[Any]) -> Optional[List[int]]:
        if np is None:
            return None

        int_arrays: List[np.ndarray] = []
        float_arrays: List[np.ndarray] = []
        for out in outputs:
            if not isinstance(out, np.ndarray):
                continue
            if out.dtype.kind in {"i", "u"}:
                int_arrays.append(out)
            elif out.dtype.kind == "f":
                float_arrays.append(out)

        for arr in int_arrays:
            if arr.ndim == 2 and arr.shape[0] >= 1:
                return [int(x) for x in arr[0].tolist()]
            if arr.ndim == 1:
                return [int(x) for x in arr.tolist()]

        for arr in float_arrays:
            if arr.ndim == 3 and arr.shape[0] >= 1:
                ids = arr[0].argmax(axis=-1)
                return [int(x) for x in ids.tolist()]

        return None

    def _decode_token_ids(self, token_ids: List[int], token_list: List[str]) -> str:
        blank_id = 0
        out_tokens: List[str] = []
        prev: Optional[int] = None

        for tid in token_ids:
            if tid == blank_id:
                prev = tid
                continue
            if prev is not None and tid == prev:
                continue
            prev = tid
            if tid < 0 or tid >= len(token_list):
                continue
            tok = token_list[tid]
            if tok in {"<blank>", "<pad>", "<s>", "</s>", "<eos>", "<bos>"}:
                continue
            out_tokens.append(tok)

        text = "".join(out_tokens)
        text = text.replace("\u2581", " ").replace("<space>", " ")
        return " ".join(text.split()).strip()
