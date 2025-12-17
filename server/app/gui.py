from __future__ import annotations

import argparse
import os
import queue
import re
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Callable, Optional

from app.server_config import ServerConfig, load_config, save_config


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
    # PyInstaller 打包后，sys.executable 指向 exe，本身并不支持 `-m uvicorn`。
    # 采用 “exe 拉起 exe” 的方式，并通过 server_entry 的内部参数启动服务端。
    if bool(getattr(sys, "frozen", False)):
        return [
            sys.executable,
            "--run-server",
            "--host",
            host,
            "--port",
            str(port),
        ]

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


def _runtime_base_path() -> Path:
    base = (os.environ.get("GHOSTTYPE_BASE_PATH") or "").strip()
    if base:
        return Path(base).expanduser()
    if bool(getattr(sys, "frozen", False)):
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parents[1]


class GhostTypeServerGui:
    def __init__(self, *, host: str, port: int) -> None:
        self._host = host
        self._port = port
        self._base_path = _runtime_base_path().resolve()
        self._config = load_config(self._base_path)
        raw_level = (os.environ.get("GHOSTTYPE_LOG") or "").strip()
        self._log_level = raw_level.upper() if raw_level else self._config.log_level
        self._server_ready = False
        self._state = RuntimeState()
        self._icons = _default_icons()
        self._log_queue: "queue.Queue[str]" = queue.Queue()
        self._stop_event = threading.Event()
        self._process: Optional[subprocess.Popen[str]] = None
        self._icon = None
        self._root = None
        self._text = None
        self._host_var = None
        self._port_var = None
        self._log_level_var = None
        self._service_status_var = None
        self._clients_var = None
        self._model_var = None
        self._provider_var = None
        self._last_asr_var = None

    def run(self) -> None:
        self._init_tk()
        self._start_server()
        self._init_tray()
        self._root.mainloop()

    def _init_tk(self) -> None:
        import tkinter as tk
        from tkinter import messagebox, ttk
        from tkinter.scrolledtext import ScrolledText

        root = tk.Tk()
        root.title("GhostType Server")
        root.geometry("900x720")
        root.minsize(860, 620)

        container = ttk.Frame(root, padding=12)
        container.pack(fill="both", expand=True)

        # === 状态面板 ===
        status_frame = ttk.LabelFrame(container, text="📊 状态", padding=10)
        status_frame.pack(fill="x", pady=(0, 10))

        self._service_status_var = tk.StringVar(value="启动中")
        self._clients_var = tk.StringVar(value="0")
        self._model_var = tk.StringVar(value="-")
        self._provider_var = tk.StringVar(value="-")
        self._last_asr_var = tk.StringVar(value="-")

        def row(label: str, var: tk.StringVar, r: int) -> None:
            ttk.Label(status_frame, text=label).grid(row=r, column=0, sticky="w", padx=(0, 10), pady=2)
            ttk.Label(status_frame, textvariable=var).grid(row=r, column=1, sticky="w", pady=2)

        row("服务状态:", self._service_status_var, 0)
        row("连接数:", self._clients_var, 1)
        row("模型:", self._model_var, 2)
        row("Provider:", self._provider_var, 3)
        row("最近一次 ASR 耗时:", self._last_asr_var, 4)
        status_frame.columnconfigure(1, weight=1)

        # === 设置面板 ===
        settings_frame = ttk.LabelFrame(container, text="⚙️ 设置", padding=10)
        settings_frame.pack(fill="x", pady=(0, 10))

        self._host_var = tk.StringVar(value=self._host)
        self._port_var = tk.StringVar(value=str(self._port))
        self._log_level_var = tk.StringVar(value=self._log_level)

        ttk.Label(settings_frame, text="绑定地址").grid(row=0, column=0, sticky="w", padx=(0, 10), pady=4)
        host_combo = ttk.Combobox(
            settings_frame,
            textvariable=self._host_var,
            state="readonly",
            values=["0.0.0.0", "127.0.0.1"],
            width=20,
        )
        host_combo.grid(row=0, column=1, sticky="w", pady=4)

        ttk.Label(settings_frame, text="绑定端口").grid(row=1, column=0, sticky="w", padx=(0, 10), pady=4)
        port_entry = ttk.Entry(settings_frame, textvariable=self._port_var, width=12)
        port_entry.grid(row=1, column=1, sticky="w", pady=4)

        ttk.Label(settings_frame, text="日志级别").grid(row=2, column=0, sticky="w", padx=(0, 10), pady=4)
        level_combo = ttk.Combobox(
            settings_frame,
            textvariable=self._log_level_var,
            state="readonly",
            values=["DEBUG", "INFO", "WARNING", "ERROR"],
            width=12,
        )
        level_combo.grid(row=2, column=1, sticky="w", pady=4)

        cfg_path = self._base_path / "config.json"
        ttk.Label(settings_frame, text=f"配置文件: {cfg_path}").grid(
            row=3, column=0, columnspan=2, sticky="w", pady=(6, 2)
        )

        def on_save_and_restart() -> None:
            host = (self._host_var.get() if self._host_var else "").strip()
            if not host:
                messagebox.showerror("配置错误", "绑定地址不能为空")
                return

            try:
                port = int((self._port_var.get() if self._port_var else "").strip())
            except Exception:
                messagebox.showerror("配置错误", "绑定端口必须是整数")
                return
            if not (1 <= port <= 65535):
                messagebox.showerror("配置错误", "绑定端口必须在 1~65535 之间")
                return

            level = (self._log_level_var.get() if self._log_level_var else "").strip().upper()
            if level not in {"DEBUG", "INFO", "WARNING", "ERROR"}:
                messagebox.showerror("配置错误", "日志级别必须是 DEBUG/INFO/WARNING/ERROR")
                return

            config = ServerConfig(host=host, port=port, log_level=level)
            save_config(self._base_path, config)
            self._config = config
            self._host = host
            self._port = port
            self._log_level = level

            self._log_queue.put(f"[gui] 配置已保存: host={host} port={port} log_level={level}\n")
            self._restart_server()

        ttk.Button(settings_frame, text="保存并重启服务", command=on_save_and_restart).grid(
            row=4, column=0, columnspan=2, sticky="w", pady=(8, 0)
        )
        settings_frame.columnconfigure(1, weight=1)

        # === 日志面板 ===
        logs_frame = ttk.LabelFrame(container, text="📝 日志", padding=10)
        logs_frame.pack(fill="both", expand=True)

        text = ScrolledText(logs_frame, wrap="word", height=16)
        text.pack(fill="both", expand=True)

        def hide_window() -> None:
            root.withdraw()

        root.protocol("WM_DELETE_WINDOW", hide_window)

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
            pystray.MenuItem("Show window", on_show, default=True),
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

    def _set_tray_title(self) -> None:
        if not self._icon:
            return
        self._icon.title = f"GhostType Server ({self._host}:{self._port})"
        try:
            self._icon.update_menu()
        except Exception:
            pass

    def _update_status_panel(self) -> None:
        if self._clients_var is not None:
            self._clients_var.set(str(self._state.connected_clients))

        if self._service_status_var is not None:
            if self._process is None or self._process.poll() is not None:
                text = "已停止"
            elif self._state.error:
                text = "错误"
            elif not self._server_ready:
                text = "启动中"
            else:
                text = "运行中"
            self._service_status_var.set(f"{text} ({self._host}:{self._port})")

    def _apply_state_change(self, mutate: Callable[[RuntimeState], None]) -> None:
        mutate(self._state)
        self._set_status(self._state.effective_status())
        self._update_status_panel()

    def _mark_error(self) -> None:
        def mutate(st: RuntimeState) -> None:
            st.error = True

        self._apply_state_change(mutate)

    def _mark_client_connected(self) -> None:
        def mutate(st: RuntimeState) -> None:
            st.connected_clients += 1
            st.error = False

        self._apply_state_change(mutate)

    def _mark_client_disconnected(self) -> None:
        def mutate(st: RuntimeState) -> None:
            st.connected_clients = max(0, st.connected_clients - 1)

        self._apply_state_change(mutate)

    def _mark_processing_started(self) -> None:
        def mutate(st: RuntimeState) -> None:
            st.processing = True
            st.error = False

        self._apply_state_change(mutate)

    def _mark_processing_completed(self) -> None:
        def mutate(st: RuntimeState) -> None:
            st.processing = False

        self._apply_state_change(mutate)

    def _start_server(self) -> None:
        self._server_ready = False
        self._update_status_panel()

        server_root = _runtime_base_path()
        cmd = _server_command(host=self._host, port=self._port)
        self._log_queue.put(f"[gui] starting server: {' '.join(cmd)}\n")

        env = os.environ.copy()
        env["GHOSTTYPE_LOG"] = self._log_level or "INFO"

        proc = subprocess.Popen(
            cmd,
            cwd=str(server_root),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        self._process = proc
        self._set_tray_title()

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
            self._mark_error()
            self._update_status_panel()
            return

    def _handle_log_line(self, line: str) -> None:
        self._log_queue.put(line)

        # State machine driven by stable English markers (log format is bilingual).
        if "Server ready" in line:
            self._server_ready = True
            self._update_status_panel()

        if "Client connected" in line:
            self._mark_client_connected()
        elif "Client disconnected" in line:
            self._mark_client_disconnected()
        elif "ASR inference started" in line:
            self._mark_processing_started()
        elif "ASR inference completed" in line:
            self._mark_processing_completed()
        elif "Audio decode failed" in line or "asr failed" in line:
            self._mark_error()

        self._maybe_update_model_info(line)
        self._maybe_update_last_asr_time(line)

    def _maybe_update_model_info(self, line: str) -> None:
        if self._model_var is None and self._provider_var is None:
            return
        if "ASR model loaded" not in line and "Loading ASR model" not in line:
            return

        model = None
        providers = None
        size_mb = None

        m = re.search(r"\bmodel=([^\s|]+)", line)
        if m:
            model = m.group(1)

        m = re.search(r"\bproviders=([^|]+?)\s*(\bload_time_ms=|$)", line)
        if m:
            providers = m.group(1).strip()

        m = re.search(r"\bsize_mb=([0-9.]+)", line)
        if m:
            size_mb = m.group(1)

        if model and self._model_var is not None:
            display = model
            if size_mb:
                display = f"{model} ({size_mb} MB)"
            self._model_var.set(display)
        if providers and self._provider_var is not None:
            self._provider_var.set(providers)

    def _maybe_update_last_asr_time(self, line: str) -> None:
        if self._last_asr_var is None:
            return
        if "ASR inference completed" not in line:
            return
        m = re.search(r"\binference_time_ms=([0-9.]+)", line)
        if not m:
            return
        ms = m.group(1)
        self._last_asr_var.set(f"{ms} ms")

    def _restart_server(self) -> None:
        proc = self._process
        if proc and proc.poll() is None:
            self._log_queue.put("[gui] stopping server...\n")
            try:
                proc.terminate()
            except Exception:
                pass
            try:
                proc.wait(timeout=5)
            except Exception:
                try:
                    proc.kill()
                except Exception:
                    pass

        self._process = None
        self._server_ready = False
        self._state = RuntimeState()
        if self._model_var is not None:
            self._model_var.set("-")
        if self._provider_var is not None:
            self._provider_var.set("-")
        if self._last_asr_var is not None:
            self._last_asr_var.set("-")
        self._set_status(self._state.effective_status())
        self._update_status_panel()
        self._start_server()

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
