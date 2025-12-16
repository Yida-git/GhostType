from __future__ import annotations

import argparse
import queue
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Callable, Optional


class TrayStatus(str, Enum):
    IDLE = "idle"
    CONNECTED = "connected"
    PROCESSING = "processing"
    ERROR = "error"


@dataclass
class RuntimeState:
    connected_clients: int = 0
    processing: bool = False
    error: bool = False

    def effective_status(self) -> TrayStatus:
        if self.error:
            return TrayStatus.ERROR
        if self.processing:
            return TrayStatus.PROCESSING
        if self.connected_clients > 0:
            return TrayStatus.CONNECTED
        return TrayStatus.IDLE


def _build_icon(color: tuple[int, int, int]) -> "Image.Image":
    from PIL import Image, ImageDraw

    size = 64
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    pad = 6
    draw.ellipse((pad, pad, size - pad, size - pad), fill=(*color, 255))
    draw.ellipse((pad + 2, pad + 2, size - pad - 2, size - pad - 2), outline=(255, 255, 255, 200), width=2)
    return img


def _default_icons() -> dict[TrayStatus, "Image.Image"]:
    return {
        TrayStatus.IDLE: _build_icon((140, 140, 140)),
        TrayStatus.CONNECTED: _build_icon((0, 170, 0)),
        TrayStatus.PROCESSING: _build_icon((0, 120, 255)),
        TrayStatus.ERROR: _build_icon((210, 0, 0)),
    }


def _server_command(*, host: str, port: int) -> list[str]:
    return [
        sys.executable,
        "-m",
        "uvicorn",
        "app.main:app",
        "--host",
        host,
        "--port",
        str(port),
    ]


class GhostTypeServerGui:
    def __init__(self, *, host: str, port: int) -> None:
        self._host = host
        self._port = port
        self._state = RuntimeState()
        self._icons = _default_icons()
        self._log_queue: "queue.Queue[str]" = queue.Queue()
        self._stop_event = threading.Event()
        self._process: Optional[subprocess.Popen[str]] = None
        self._icon = None
        self._root = None
        self._text = None

    def run(self) -> None:
        self._init_tk()
        self._start_server()
        self._init_tray()
        self._root.mainloop()

    def _init_tk(self) -> None:
        import tkinter as tk
        from tkinter.scrolledtext import ScrolledText

        root = tk.Tk()
        root.title("GhostType Server Logs")
        root.geometry("860x520")

        text = ScrolledText(root, wrap="word")
        text.pack(fill="both", expand=True)

        def hide_window() -> None:
            root.withdraw()

        root.protocol("WM_DELETE_WINDOW", hide_window)
        root.withdraw()

        self._root = root
        self._text = text

        def poll_logs() -> None:
            if self._stop_event.is_set():
                return
            flushed = 0
            while flushed < 200:
                try:
                    line = self._log_queue.get_nowait()
                except queue.Empty:
                    break
                self._append_log(line)
                flushed += 1
            root.after(80, poll_logs)

        root.after(80, poll_logs)

    def _append_log(self, line: str) -> None:
        if not self._text:
            return
        self._text.insert("end", line)
        if not line.endswith("\n"):
            self._text.insert("end", "\n")
        self._text.see("end")

    def _init_tray(self) -> None:
        import pystray

        def on_show(_icon, _item) -> None:
            if not self._root:
                return

            def show() -> None:
                self._root.deiconify()
                self._root.lift()
                try:
                    self._root.focus_force()
                except Exception:
                    pass

            self._root.after(0, show)

        def on_quit(icon, _item) -> None:
            self.stop()
            try:
                icon.stop()
            finally:
                pass

        menu = pystray.Menu(
            pystray.MenuItem("Show logs", on_show, default=True),
            pystray.MenuItem("Quit", on_quit),
        )

        title = f"GhostType Server ({self._host}:{self._port})"
        icon = pystray.Icon("GhostType", self._icons[self._state.effective_status()], title, menu)
        self._icon = icon

        # Keep Tkinter in the main thread; run tray icon in the background.
        icon.run_detached()

    def _set_status(self, status: TrayStatus) -> None:
        if not self._icon:
            return
        self._icon.icon = self._icons[status]
        try:
            self._icon.update_icon()
        except Exception:
            pass

    def _apply_state_change(self, mutate: Callable[[RuntimeState], None]) -> None:
        mutate(self._state)
        self._set_status(self._state.effective_status())

    def _start_server(self) -> None:
        server_root = Path(__file__).resolve().parents[1]
        cmd = _server_command(host=self._host, port=self._port)
        self._log_queue.put(f"[gui] starting server: {' '.join(cmd)}\n")

        proc = subprocess.Popen(
            cmd,
            cwd=str(server_root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        self._process = proc

        def pump(stream, prefix: str) -> None:
            try:
                for line in iter(stream.readline, ""):
                    if self._stop_event.is_set():
                        break
                    self._handle_log_line(f"{prefix}{line.rstrip()}\n")
            finally:
                try:
                    stream.close()
                except Exception:
                    pass

        if proc.stdout is not None:
            threading.Thread(target=pump, args=(proc.stdout, ""), daemon=True).start()
        if proc.stderr is not None:
            threading.Thread(target=pump, args=(proc.stderr, ""), daemon=True).start()

        threading.Thread(target=self._watch_process, daemon=True).start()

    def _watch_process(self) -> None:
        proc = self._process
        if not proc:
            return

        while not self._stop_event.is_set():
            code = proc.poll()
            if code is None:
                time.sleep(0.2)
                continue

            self._log_queue.put(f"[gui] server exited with code={code}\n")
            self._apply_state_change(lambda st: setattr(st, "error", True))
            return

    def _handle_log_line(self, line: str) -> None:
        self._log_queue.put(line)

        # State machine driven by stable English markers (log format is bilingual).
        if "Client connected" in line:
            self._apply_state_change(
                lambda st: (
                    setattr(st, "connected_clients", st.connected_clients + 1),
                    setattr(st, "error", False),
                )
            )
        elif "Client disconnected" in line:
            self._apply_state_change(
                lambda st: setattr(st, "connected_clients", max(0, st.connected_clients - 1))
            )
        elif "ASR inference started" in line:
            self._apply_state_change(lambda st: (setattr(st, "processing", True), setattr(st, "error", False)))
        elif "ASR inference completed" in line:
            self._apply_state_change(lambda st: setattr(st, "processing", False))
        elif "Audio decode failed" in line or "asr failed" in line:
            self._apply_state_change(lambda st: setattr(st, "error", True))

    def stop(self) -> None:
        self._stop_event.set()

        if self._process and self._process.poll() is None:
            try:
                self._process.terminate()
            except Exception:
                pass
            try:
                self._process.wait(timeout=5)
            except Exception:
                try:
                    self._process.kill()
                except Exception:
                    pass

        if self._root:
            try:
                self._root.after(0, self._root.quit)
            except Exception:
                pass


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="GhostType server tray GUI (pystray + Tkinter)")
    parser.add_argument("--host", default="0.0.0.0", help="Bind host (default: 0.0.0.0)")
    parser.add_argument("--port", default=8000, type=int, help="Bind port (default: 8000)")
    args = parser.parse_args(argv)

    gui = GhostTypeServerGui(host=args.host, port=args.port)
    gui.run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

