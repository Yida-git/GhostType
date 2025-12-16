#!/usr/bin/env python3
"""
Layer 0: 环境验证

运行:
  cd server
  python tests/test_layer0_env.py
"""

from __future__ import annotations

import sys
from pathlib import Path

REQUIRED_PYTHON = (3, 10)


def _configure_stdout() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
        except Exception:
            pass


def test_python_version() -> bool:
    """Python 版本必须 >= 3.10"""
    print(f"Python: {sys.version}")
    print(f"Executable: {sys.executable}")
    if sys.version_info < REQUIRED_PYTHON:
        print(f"  ❌ 需要 Python {REQUIRED_PYTHON[0]}.{REQUIRED_PYTHON[1]}+")
        return False
    print("  ✅ Python 版本检查通过")
    return True


def test_imports() -> bool:
    """所有核心依赖必须能导入"""
    modules = [
        ("fastapi", "FastAPI"),
        ("uvicorn", "uvicorn"),
        ("websockets", "websockets"),
        ("loguru", "loguru"),
        ("av", "PyAV"),
        ("numpy", "numpy"),
        ("onnxruntime", "onnxruntime"),
    ]

    all_ok = True
    for module_name, display_name in modules:
        try:
            __import__(module_name)
            print(f"  ✅ {display_name}")
        except ImportError as exc:
            print(f"  ❌ {display_name}: {exc}")
            all_ok = False

    return all_ok


def test_onnxruntime_providers() -> bool:
    """检查 ONNX Runtime 可用的执行提供者"""
    try:
        import onnxruntime as ort
    except ImportError:
        print("  ❌ onnxruntime 未安装")
        return False

    providers = ort.get_available_providers()
    version = getattr(ort, "__version__", None)
    if version:
        print(f"  onnxruntime: {version}")
    print(f"  可用 Providers: {providers}")

    has_dml = "DmlExecutionProvider" in providers
    has_cuda = "CUDAExecutionProvider" in providers
    has_coreml = "CoreMLExecutionProvider" in providers
    has_cpu = "CPUExecutionProvider" in providers

    if has_dml:
        print("  ✅ DirectML 可用 (Windows GPU)")
    if has_cuda:
        print("  ✅ CUDA 可用 (NVIDIA GPU)")
    if has_coreml:
        print("  ✅ CoreML 可用 (macOS)")
    if not has_dml and not has_cuda and not has_coreml:
        if sys.platform == "darwin":
            print("  ⚠️  警告: 未检测到 CoreML Provider，将使用 CPU，ASR 性能可能不足")
        else:
            print("  ⚠️  警告: 只有 CPU，ASR 性能可能不足")

    if not has_cpu:
        print("  ❌ 连 CPU Provider 都没有，onnxruntime 安装异常")
        return False

    return True


def test_model_exists() -> bool:
    """检查模型文件是否存在"""
    server_dir = Path(__file__).parent.parent
    model_path = server_dir / "models" / "sensevoice-small.onnx"

    print(f"  模型路径: {model_path}")

    if model_path.exists():
        size_mb = model_path.stat().st_size / (1024 * 1024)
        print(f"  ✅ 模型存在 ({size_mb:.1f} MB)")
        if size_mb < 10:
            print("  ⚠️  警告: 模型文件过小，可能不完整")
        return True

    print("  ❌ 模型不存在!")
    downloads = model_path.parent / "_downloads"
    if downloads.exists():
        files = list(downloads.glob("*"))
        print("   _downloads 目录内容:")
        for f in files[:10]:
            print(f"      - {f.name}")
        print("   提示: 请解压模型并移动到 models/ 目录")

    return False


def test_tokens_file() -> bool:
    """检查 tokens 文件 (非必须)"""
    server_dir = Path(__file__).parent.parent
    candidates = [
        server_dir / "models" / "tokens.txt",
        server_dir / "models" / "sensevoice-small.tokens.txt",
        server_dir / "models" / "vocab.txt",
    ]

    for path in candidates:
        if path.exists():
            lines = len(path.read_text(encoding="utf-8").splitlines())
            print(f"  ✅ tokens 文件: {path.name} ({lines} lines)")
            return True

    print("  ⚠️  tokens 文件不存在 (如果模型直接输出文本则无需)")
    return True


def test_no_ghost_files() -> bool:
    """检查是否有幽灵文件 (警告但不阻塞)"""
    server_dir = Path(__file__).parent.parent
    project_root = server_dir.parent

    ghost_files = ["nul", "con", "prn", "aux"]
    found_ghosts = []

    def dir_has_entry(dir_path: Path, name: str) -> bool:
        # NOTE: On Windows, device names like "CON"/"NUL" can make Path.exists()
        # return True even if no actual filesystem entry exists. Use directory
        # listing to detect real ghost files.
        try:
            return any(child.name.lower() == name.lower() for child in dir_path.iterdir())
        except OSError:
            return False

    for ghost in ghost_files:
        if dir_has_entry(project_root, ghost):
            found_ghosts.append(str(project_root / ghost))
        if dir_has_entry(server_dir, ghost):
            found_ghosts.append(str(server_dir / ghost))

    if found_ghosts:
        print(f"  ⚠️  发现幽灵文件: {found_ghosts}")
        print(r"   提示: 使用 del /f \\?\<path> 删除")
        return True

    print("  ✅ 无幽灵文件")
    return True


def main() -> None:
    _configure_stdout()

    print("=" * 60)
    print("Layer 0: 环境验证")
    print("=" * 60)

    tests = [
        ("Python 版本", test_python_version),
        ("依赖导入", test_imports),
        ("ONNX Providers", test_onnxruntime_providers),
        ("模型文件", test_model_exists),
        ("Tokens 文件", test_tokens_file),
        ("幽灵文件检查", test_no_ghost_files),
    ]

    passed = 0
    failed = 0

    for name, test_fn in tests:
        print(f"\n{name}")
        try:
            if test_fn():
                passed += 1
            else:
                failed += 1
        except Exception as exc:
            print(f"  ❌ 异常: {exc}")
            failed += 1

    print("\n" + "=" * 60)
    print(f"结果: {passed} 通过, {failed} 失败")
    print("=" * 60)

    if failed > 0:
        print("\n❌ Layer 0 未通过，请先修复上述问题")
        raise SystemExit(1)

    print("\n✅ Layer 0 通过，可以进入 Layer 1")
    raise SystemExit(0)


if __name__ == "__main__":
    main()
