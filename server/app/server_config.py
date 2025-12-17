from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class ServerConfig:
    host: str = "0.0.0.0"
    port: int = 8000
    log_level: str = "INFO"


_VALID_LOG_LEVELS = {"DEBUG", "INFO", "WARNING", "ERROR"}


def config_path(base_path: Path) -> Path:
    return Path(base_path) / "config.json"


def load_config(base_path: Path) -> ServerConfig:
    path = config_path(base_path)
    if not path.exists():
        return ServerConfig()

    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return ServerConfig()

    if not isinstance(data, dict):
        return ServerConfig()

    def get_str(key: str, default: str) -> str:
        raw = data.get(key)
        if raw is None:
            return default
        value = str(raw).strip()
        return value or default

    def get_int(key: str, default: int) -> int:
        raw = data.get(key)
        if raw is None:
            return default
        try:
            value = int(raw)
        except Exception:
            return default
        if not (1 <= value <= 65535):
            return default
        return value

    host = get_str("host", ServerConfig.host)
    port = get_int("port", ServerConfig.port)
    log_level = get_str("log_level", ServerConfig.log_level).upper()
    if log_level not in _VALID_LOG_LEVELS:
        log_level = ServerConfig.log_level

    return ServerConfig(host=host, port=port, log_level=log_level)


def save_config(base_path: Path, config: ServerConfig) -> None:
    path = config_path(base_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    data: dict[str, Any] = asdict(config)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

