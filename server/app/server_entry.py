from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Optional

from app.server_config import load_config


def _is_frozen() -> bool:
    """是否处于 PyInstaller 打包后的 frozen 模式。"""

    return bool(getattr(sys, "frozen", False))


def _resolve_base_path() -> Path:
    """
    解析运行时资源根目录。

    优先级：
    1) 显式环境变量 GHOSTTYPE_BASE_PATH（方便调试/自定义安装目录）
    2) 打包模式：exe 所在目录（便于与 models/ 同级分发）
    3) 源码模式：server/ 目录（当前文件位于 server/app/ 下）
    """

    env = (os.environ.get("GHOSTTYPE_BASE_PATH") or "").strip()
    if env:
        return Path(env).expanduser().resolve()

    if _is_frozen():
        return Path(sys.executable).resolve().parent

    return Path(__file__).resolve().parents[1]


def _show_error_dialog(title: str, message: str) -> None:
    """尽量用弹窗提示错误；失败则回退到 stderr。"""

    try:
        import tkinter as tk
        from tkinter import messagebox

        root = tk.Tk()
        root.withdraw()
        try:
            messagebox.showerror(title, message)
        finally:
            try:
                root.destroy()
            except Exception:
                pass
    except Exception:
        sys.stderr.write(f"{title}\n{message}\n")


def _model_path(base_path: Path) -> Path:
    return base_path / "models" / "sensevoice-small.onnx"


def _ensure_base_env(base_path: Path) -> None:
    os.environ["GHOSTTYPE_BASE_PATH"] = str(base_path)


def _maybe_chdir_base(base_path: Path) -> None:
    """
    进程工作目录统一到 base_path，避免：
    - Windows 双击启动 cwd 落到 System32 导致 logs/ 写错位置
    - 相对路径资源查找不稳定
    """

    try:
        os.chdir(str(base_path))
    except OSError:
        pass


def _run_uvicorn(*, host: str, port: int) -> int:
    import uvicorn

    uvicorn.run(
        "app.main:app",
        host=host,
        port=port,
        reload=False,
        access_log=False,
    )
    return 0


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="GhostType Server 入口（支持打包/源码两种模式）")
    parser.add_argument("--host", default=None, help="绑定地址（默认读取 config.json / 回退 0.0.0.0）")
    parser.add_argument("--port", default=None, type=int, help="绑定端口（默认读取 config.json / 回退 8000）")
    parser.add_argument("--no-gui", action="store_true", help="不启动托盘 GUI，仅运行服务端")
    # 内部参数：用于 GUI 子进程启动服务端（打包后用 exe 自己拉起自己）
    parser.add_argument("--run-server", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)

    base_path = _resolve_base_path()
    _ensure_base_env(base_path)
    _maybe_chdir_base(base_path)

    config = load_config(base_path)
    host = (args.host or "").strip() or config.host
    port = int(args.port) if args.port is not None else int(config.port)

    raw_level = (os.environ.get("GHOSTTYPE_LOG") or "").strip()
    if not raw_level:
        os.environ["GHOSTTYPE_LOG"] = str(config.log_level)

    model_path = _model_path(base_path)
    if not model_path.exists():
        msg = (
            "未找到 ASR 模型文件（sensevoice-small.onnx）。\n\n"
            f"期望路径：\n{model_path}\n\n"
            "请将模型放到上述位置的 models/ 目录后再启动。"
        )
        if args.no_gui or args.run_server:
            sys.stderr.write(msg + "\n")
        else:
            _show_error_dialog("GhostType Server - 缺少模型文件", msg)
        return 2

    if args.no_gui or args.run_server:
        return _run_uvicorn(host=host, port=port)

    from app.gui import main as gui_main

    return int(gui_main(["--host", host, "--port", str(port)]))


if __name__ == "__main__":
    raise SystemExit(main())
