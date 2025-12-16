from __future__ import annotations

import asyncio
import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Protocol

from app.logging_config import get_logger, setup_logging

setup_logging()
log = get_logger("asr")

try:
    import numpy as np  # type: ignore[import-not-found]
    import onnxruntime as ort  # type: ignore[import-not-found]
except Exception:  # noqa: BLE001 - optional at import time; raised on engine init
    np = None  # type: ignore[assignment]
    ort = None  # type: ignore[assignment]

try:
    from app.core.fbank import sensevoice_ctc_features
except Exception:  # noqa: BLE001 - optional dependency at import time
    sensevoice_ctc_features = None  # type: ignore[assignment]


class AsrEngine(Protocol):
    async def transcribe(self, audio_pcm: bytes, sample_rate: int) -> str: ...


class StubAsrEngine:
    async def transcribe(self, audio_pcm: bytes, sample_rate: int) -> str:
        return f"[pcm_bytes={len(audio_pcm)} sr={sample_rate}]"


@dataclass(frozen=True)
class SenseVoiceConfig:
    sample_rate: int = 16000
    language: str = "auto"
    text_norm: str = "with_itn"
    dml_device_id: Optional[int] = None


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
                "onnxruntime/numpy not installed; add `onnxruntime-directml` (Windows) or `onnxruntime-gpu` (CUDA) or `onnxruntime` + `numpy` (macOS/Linux CPU, 可选 CoreML)"
            )

        self.model_path = Path(model_path)
        if not self.model_path.exists():
            raise FileNotFoundError(str(self.model_path))

        self.config = config or SenseVoiceConfig()

        sess_options = ort.SessionOptions()
        sess_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL

        # Create a CPU session first for model inspection. Some provider/model combinations
        # can hard-crash the process (e.g. DML + ORT-quantized MatMul); avoid them by
        # detecting quantization from metadata before selecting providers.
        cpu_session = ort.InferenceSession(
            str(self.model_path),
            sess_options=sess_options,
            providers=["CPUExecutionProvider"],
        )
        self._meta = self._read_custom_metadata(cpu_session)
        self._token_list = self._load_token_list()
        self._init_model_io(cpu_session)

        available = ort.get_available_providers()
        available_set = set(available)
        preferred = preferred_providers or [
            "CUDAExecutionProvider",
            "CoreMLExecutionProvider",
            "DmlExecutionProvider",
            "CPUExecutionProvider",
        ]

        wants = [p for p in preferred if p in available_set]
        if not wants:
            wants = available or ["CPUExecutionProvider"]

        is_ort_quant = (self._meta.get("onnx.infer") or "").strip() == "onnxruntime.quant"
        wants = [p for p in wants if not (p == "DmlExecutionProvider" and is_ort_quant)]
        if not wants:
            wants = ["CPUExecutionProvider"]

        self.session = self._create_session_with_providers(
            sess_options=sess_options,
            requested=wants,
            cpu_session=cpu_session,
        )
        self.providers = self.session.get_providers()

    async def transcribe(self, audio_pcm: bytes, sample_rate: int) -> str:
        return await asyncio.to_thread(self._transcribe_sync, audio_pcm, sample_rate)

    def _read_custom_metadata(self, session: Any) -> Dict[str, str]:
        try:
            meta = session.get_modelmeta()
            custom = getattr(meta, "custom_metadata_map", None) or {}
        except Exception:
            return {}
        return {str(k): str(v) for k, v in custom.items()}

    def _init_model_io(self, session: Any) -> None:
        input_names = [inp.name for inp in session.get_inputs()]
        if {"x", "x_length", "language", "text_norm"}.issubset(input_names):
            self._mode = "sense_voice_ctc"
            self._ctc_drop_frames = 4
            self._init_sense_voice_ctc_inputs(session)
            return

        self._mode = "waveform"
        self._ctc_drop_frames = 0
        self._input_waveform_name, self._input_waveform_type = self._pick_waveform_input(session)
        self._input_length = self._pick_length_input(session)

    def _pick_waveform_input(self, session: Any) -> tuple[str, str]:
        inputs = session.get_inputs()
        for inp in inputs:
            if "tensor(float" in inp.type:
                return inp.name, inp.type
        if inputs:
            return inputs[0].name, inputs[0].type
        raise RuntimeError("onnx model has no inputs")

    def _pick_length_input(self, session: Any) -> Optional[tuple[str, str]]:
        for inp in session.get_inputs():
            if inp.name == self._input_waveform_name:
                continue
            if "tensor(int" in inp.type:
                return inp.name, inp.type
        return None

    def _init_sense_voice_ctc_inputs(self, session: Any) -> None:
        if np is None:
            raise RuntimeError("numpy not installed")

        x_inp = next((i for i in session.get_inputs() if i.name == "x"), None)
        if x_inp is None:
            raise RuntimeError("sense_voice_ctc model missing input: x")

        self._input_x_type = x_inp.type
        try:
            feature_dim = int(x_inp.shape[-1])
        except Exception:
            raise RuntimeError(f"unsupported x shape: {x_inp.shape}")

        lfr_m = int(self._meta.get("lfr_window_size") or 7)
        lfr_n = int(self._meta.get("lfr_window_shift") or 6)
        if feature_dim % lfr_m != 0:
            raise RuntimeError(f"feature_dim {feature_dim} not divisible by lfr_window_size {lfr_m}")
        n_mels = feature_dim // lfr_m

        cmvn_neg = self._parse_csv_vector(self._meta.get("neg_mean"), expected_dim=feature_dim)
        cmvn_inv = self._parse_csv_vector(self._meta.get("inv_stddev"), expected_dim=feature_dim)

        self._sensevoice_n_mels = n_mels
        self._sensevoice_lfr_m = lfr_m
        self._sensevoice_lfr_n = lfr_n
        self._sensevoice_cmvn_neg_mean = cmvn_neg
        self._sensevoice_cmvn_inv_stddev = cmvn_inv
        self._sensevoice_language_id = self._resolve_language_id()
        self._sensevoice_text_norm_id = self._resolve_text_norm_id()

    def _parse_csv_vector(self, value: Optional[str], *, expected_dim: int) -> Any:
        if np is None:
            raise RuntimeError("numpy not installed")
        if not value:
            raise RuntimeError("missing cmvn vector in model metadata")
        parts = [p for p in value.split(",") if p.strip() != ""]
        if len(parts) != expected_dim:
            raise RuntimeError(f"cmvn vector dim mismatch: got={len(parts)} expected={expected_dim}")
        arr = np.array([float(p) for p in parts], dtype=np.float32)
        return np.ascontiguousarray(arr)

    def _resolve_language_id(self) -> int:
        lang = (self.config.language or "auto").strip().lower()
        key = f"lang_{lang}"
        raw = self._meta.get(key) or self._meta.get("lang_auto") or "0"
        try:
            return int(raw)
        except ValueError:
            return 0

    def _resolve_text_norm_id(self) -> int:
        mode = (self.config.text_norm or "with_itn").strip().lower()
        key = "with_itn" if mode in {"with_itn", "withitn", "itn"} else "without_itn"
        raw = self._meta.get(key) or self._meta.get("with_itn") or "0"
        try:
            return int(raw)
        except ValueError:
            return 0

    def _create_session_with_providers(
        self,
        *,
        sess_options: Any,
        requested: List[str],
        cpu_session: Any,
    ) -> Any:
        if ort is None:
            raise RuntimeError("onnxruntime not installed")

        if requested == ["CPUExecutionProvider"]:
            return cpu_session

        if "DmlExecutionProvider" not in requested:
            return ort.InferenceSession(str(self.model_path), sess_options=sess_options, providers=requested)

        dml_ids = self._candidate_dml_device_ids()
        last_exc: Optional[Exception] = None
        for device_id in dml_ids:
            providers: List[Any] = []
            for p in requested:
                if p == "DmlExecutionProvider":
                    providers.append(("DmlExecutionProvider", {"device_id": device_id}))
                else:
                    providers.append(p)
            try:
                return ort.InferenceSession(str(self.model_path), sess_options=sess_options, providers=providers)
            except Exception as exc:
                last_exc = exc
                continue

        if last_exc is not None:
            log.warning(
                "DirectML 初始化失败，回退 CPU | DML init failed, falling back to CPU | "
                "model={model} error={error}",
                model=str(self.model_path),
                error=str(last_exc),
            )
        return cpu_session

    def _candidate_dml_device_ids(self) -> List[int]:
        if self.config.dml_device_id is not None:
            return [int(self.config.dml_device_id)]

        raw = os.environ.get("GHOSTTYPE_DML_DEVICE_ID") or os.environ.get("ORT_DML_DEVICE_ID")
        if raw is not None:
            try:
                return [int(raw)]
            except ValueError:
                return [0]

        # Heuristic for laptops: 0 often = iGPU, 1 often = dGPU.
        return [1, 0]

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
            tokens = self._parse_token_file(content)
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

    def _parse_token_file(self, content: str) -> Optional[List[str]]:
        lines = [line.strip() for line in content.splitlines() if line.strip() != ""]
        if not lines:
            return None

        pairs: List[tuple[int, str]] = []
        for line in lines:
            parts = line.rsplit(maxsplit=1)
            if len(parts) != 2:
                pairs = []
                break
            tok, idx = parts
            try:
                pairs.append((int(idx), tok))
            except ValueError:
                pairs = []
                break

        if not pairs:
            return lines

        max_id = max(i for i, _ in pairs)
        out = [""] * (max_id + 1)
        for i, tok in pairs:
            if 0 <= i < len(out):
                out[i] = tok
        return out

    def _transcribe_sync(self, audio_pcm: bytes, sample_rate: int) -> str:
        if np is None:
            raise RuntimeError("numpy not installed")

        if sample_rate != self.config.sample_rate:
            return f"[unsupported sample_rate={sample_rate}; expected {self.config.sample_rate}]"

        if self._mode == "sense_voice_ctc":
            if sensevoice_ctc_features is None:
                raise RuntimeError("sensevoice frontend not available; install numpy")
            x, x_len = sensevoice_ctc_features(
                audio_pcm,
                sample_rate=sample_rate,
                n_mels=self._sensevoice_n_mels,
                lfr_m=self._sensevoice_lfr_m,
                lfr_n=self._sensevoice_lfr_n,
                cmvn_neg_mean=self._sensevoice_cmvn_neg_mean,
                cmvn_inv_stddev=self._sensevoice_cmvn_inv_stddev,
            )
            if x_len == 0:
                return ""

            input_x = x[None, :, :]
            if "float16" in getattr(self, "_input_x_type", ""):
                input_x = input_x.astype(np.float16)

            inputs: Dict[str, Any] = {
                "x": input_x,
                "x_length": np.array([x_len], dtype=np.int32),
                "language": np.array([self._sensevoice_language_id], dtype=np.int32),
                "text_norm": np.array([self._sensevoice_text_norm_id], dtype=np.int32),
            }
            outputs = self.session.run(None, inputs)
            return self._decode_outputs(outputs)

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
            inputs = dict(base_inputs)
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

        token_ids = self._extract_token_ids(outputs, drop_first=self._ctc_drop_frames)
        if token_ids is None:
            return "[asr_output_unhandled]"

        if not self._token_list:
            return f"[token_ids={token_ids[:64]}{'...' if len(token_ids) > 64 else ''}]"

        return self._decode_token_ids(token_ids, self._token_list)

    def _extract_token_ids(self, outputs: List[Any], *, drop_first: int = 0) -> Optional[List[int]]:
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
                ids = [int(x) for x in arr[0].tolist()]
                return ids[drop_first:] if drop_first > 0 else ids
            if arr.ndim == 1:
                ids = [int(x) for x in arr.tolist()]
                return ids[drop_first:] if drop_first > 0 else ids

        for arr in float_arrays:
            if arr.ndim == 3 and arr.shape[0] >= 1:
                ids = arr[0].argmax(axis=-1)
                ids_list = [int(x) for x in ids.tolist()]
                return ids_list[drop_first:] if drop_first > 0 else ids_list

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
