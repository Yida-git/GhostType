from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any

from loguru import logger

_CONFIGURED = False


def setup_logging() -> Any:
    """Configure GhostType logging (loguru)."""

    global _CONFIGURED
    if _CONFIGURED:
        return logger

    logger.remove()

    level = os.getenv("GHOSTTYPE_LOG", "WARNING").upper()

    fmt = (
        "<green>[{time:YYYY-MM-DD HH:mm:ss.SSS}]</green> "
        "<level>[{level: <5}]</level> "
        "<cyan>[{extra[module]: <8}]</cyan> "
        "{extra[trace_id]}{message}"
    )

    logger.add(
        sys.stderr,
        format=fmt,
        level=level,
        colorize=True,
        backtrace=True,
        diagnose=True,
        enqueue=True,
    )

    if os.getenv("GHOSTTYPE_LOG_FILE"):
        Path("logs").mkdir(parents=True, exist_ok=True)

        plain_fmt = fmt
        for tag in ("<green>", "</green>", "<level>", "</level>", "<cyan>", "</cyan>"):
            plain_fmt = plain_fmt.replace(tag, "")

        logger.add(
            "logs/ghosttype_{time:YYYY-MM-DD}.log",
            format=plain_fmt,
            level=level,
            rotation="00:00",
            retention="7 days",
            compression="gz",
            enqueue=True,
        )

    _CONFIGURED = True
    return logger


def get_logger(module: str):
    """Get a module-scoped logger with a fixed module id (<=8 chars)."""
    module = (module or "server")[:8]
    return logger.bind(module=module, trace_id="")


def with_trace(log, trace_id: str):
    """Bind a 6-char trace id to a logger."""
    trace_id = (trace_id or "").strip()
    if trace_id:
        return log.bind(trace_id=f"[t:{trace_id}] ")
    return log.bind(trace_id="")
