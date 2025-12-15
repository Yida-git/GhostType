from typing import Any, Dict, Protocol


class LlmEngine(Protocol):
    async def correct(self, text: str, context: Dict[str, Any]) -> str: ...


class StubLlmEngine:
    async def correct(self, text: str, context: Dict[str, Any]) -> str:
        return text

