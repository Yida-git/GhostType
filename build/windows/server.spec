# -*- mode: python ; coding: utf-8 -*-
#
# GhostType Server (Windows) - PyInstaller 打包配置
#
# 设计原则：
# - 入口统一使用 app.server_entry（自动处理 base_path / 模型检查 / GUI 启动）
# - dist/work 输出固定到 build/windows/ 下，避免污染仓库根目录
# - 尽量通过 collect_* 自动收集 DLL（DirectML / FFmpeg 等），降低手工维护成本
#

from __future__ import annotations

import sys
from pathlib import Path

from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs, collect_submodules

build_dir = Path(__file__).resolve().parent  # build/windows
repo_root = build_dir.parents[1]  # repo/
server_root = repo_root / "server"

entry_script = server_root / "app" / "server_entry.py"

sys.path.insert(0, str(server_root))

hiddenimports: list[str] = []
hiddenimports += collect_submodules("app")
hiddenimports += collect_submodules("uvicorn")
hiddenimports += collect_submodules("fastapi")
hiddenimports += collect_submodules("starlette")
hiddenimports += collect_submodules("pydantic")
hiddenimports += collect_submodules("anyio")
hiddenimports += collect_submodules("sniffio")
hiddenimports += collect_submodules("websockets")
hiddenimports += collect_submodules("wsproto")
hiddenimports += collect_submodules("h11")

# Windows 托盘后端
hiddenimports += ["pystray._win32"]
hiddenimports += collect_submodules("pystray")
hiddenimports += collect_submodules("PIL")

# ASR / 音频依赖（含大量动态库）
hiddenimports += collect_submodules("onnxruntime")
hiddenimports += collect_submodules("av")

datas: list[tuple[str, str]] = []

# 分发包中附带 models/README.md（模型文件通常不入库，需用户自行放置或由构建脚本复制）
model_readme = server_root / "models" / "README.md"
if model_readme.exists():
    datas += [(str(model_readme), "models")]

# 某些包可能需要额外数据文件（如 av 的运行时资源）
datas += collect_data_files("av", include_py_files=False)

binaries: list[tuple[str, str]] = []

# onnxruntime-directml: DirectML.dll / onnxruntime.dll 等
binaries += collect_dynamic_libs("onnxruntime")

# PyAV: ffmpeg*.dll 等（通常在 av.libs/ 下）
binaries += collect_dynamic_libs("av")

block_cipher = None

a = Analysis(
    [str(entry_script)],
    pathex=[str(server_root)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "matplotlib",
        "scipy",
        "pandas",
        "pytest",
        "setuptools",
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="GhostTypeServer",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=False,  # 托盘 GUI 模式，默认不弹控制台
    disable_windowed_traceback=False,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name="GhostTypeServer",
)
