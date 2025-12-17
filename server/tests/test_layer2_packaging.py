#!/usr/bin/env python3
"""
Layer 2: 打包相关功能测试

运行:
  cd server
  python tests/test_layer2_packaging.py

无需启动服务端，纯单元测试。
"""

from __future__ import annotations

import os
import sys
import tempfile
from contextlib import contextmanager
from pathlib import Path
from typing import Iterator


def _configure_stdout() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
        except Exception:
            pass


@contextmanager
def _temp_env(**updates: str | None) -> Iterator[None]:
    backup: dict[str, str | None] = {}
    for key, value in updates.items():
        backup[key] = os.environ.get(key)
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value
    try:
        yield
    finally:
        for key, old in backup.items():
            if old is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old


@contextmanager
def _restore_cwd() -> Iterator[None]:
    cwd = os.getcwd()
    try:
        yield
    finally:
        try:
            os.chdir(cwd)
        except OSError:
            pass


def _import_modules():
    server_dir = Path(__file__).resolve().parent.parent
    sys.path.insert(0, str(server_dir))
    from app import gui as gui_mod
    from app import main as main_mod
    from app import server_entry as server_entry_mod

    return server_dir, server_entry_mod, gui_mod, main_mod


def test_t2_1_is_frozen_source_mode(server_entry) -> bool:
    print("\nT2.1: _is_frozen() 源码模式返回 False")
    ok = server_entry._is_frozen() is False
    print(f"  {'✅' if ok else '❌'} got={server_entry._is_frozen()}")
    return ok


def test_t2_2_resolve_base_path_source_mode(server_dir: Path, server_entry) -> bool:
    print("\nT2.2: _resolve_base_path() 源码模式返回 server/ 目录")
    with _temp_env(GHOSTTYPE_BASE_PATH=None):
        got = server_entry._resolve_base_path()
    expected = server_dir
    ok = got.resolve() == expected.resolve()
    print(f"  expected: {expected}")
    print(f"  got:      {got}")
    print(f"  {'✅' if ok else '❌'}")
    return ok


def test_t2_3_resolve_base_path_env_override(server_entry) -> bool:
    print("\nT2.3: _resolve_base_path() ENV 覆盖生效")
    with tempfile.TemporaryDirectory() as tmp:
        with _temp_env(GHOSTTYPE_BASE_PATH=tmp):
            got = server_entry._resolve_base_path()
        expected = Path(tmp).expanduser().resolve()
        ok = got.resolve() == expected
        print(f"  expected: {expected}")
        print(f"  got:      {got}")
        print(f"  {'✅' if ok else '❌'}")
        return ok


def test_t2_4_model_path(server_dir: Path, server_entry) -> bool:
    print("\nT2.4: _model_path() 返回正确路径")
    got = server_entry._model_path(server_dir)
    expected = server_dir / "models" / "sensevoice-small.onnx"
    ok = got.resolve() == expected.resolve()
    print(f"  expected: {expected}")
    print(f"  got:      {got}")
    print(f"  {'✅' if ok else '❌'}")
    return ok


def test_t2_5_server_entry_main_exit_code_2_when_model_missing(server_entry) -> bool:
    print("\nT2.5: server_entry.main() 无模型时 exit code = 2")
    with tempfile.TemporaryDirectory() as tmp:
        with _restore_cwd(), _temp_env(GHOSTTYPE_BASE_PATH=tmp):
            code = int(server_entry.main(["--no-gui"]))
    ok = code == 2
    print(f"  expected: 2")
    print(f"  got:      {code}")
    print(f"  {'✅' if ok else '❌'}")
    return ok


def test_t2_6_gui_server_command_source_mode(gui_mod) -> bool:
    print("\nT2.6: gui._server_command() 源码模式返回 python -m uvicorn")
    cmd = gui_mod._server_command(host="0.0.0.0", port=8000)
    ok = (
        len(cmd) >= 8
        and cmd[0] == sys.executable
        and cmd[1:4] == ["-m", "uvicorn", "app.main:app"]
        and "--host" in cmd
        and "--port" in cmd
    )
    print(f"  cmd: {' '.join(cmd)}")
    print(f"  {'✅' if ok else '❌'}")
    return ok


def test_t2_7_gui_runtime_base_path_consistent(server_entry, gui_mod) -> bool:
    print("\nT2.7: gui._runtime_base_path() 与 server_entry 一致")
    with _temp_env(GHOSTTYPE_BASE_PATH=None):
        a = server_entry._resolve_base_path()
        b = gui_mod._runtime_base_path()
    ok = a.resolve() == b.resolve()
    print(f"  server_entry: {a}")
    print(f"  gui:         {b}")
    print(f"  {'✅' if ok else '❌'}")
    return ok


def test_t2_8_main_resolve_model_path_source_mode(server_dir: Path, main_mod) -> bool:
    print("\nT2.8: main.py _resolve_model_path() 源码模式")
    with _temp_env(GHOSTTYPE_BASE_PATH=None):
        got = main_mod._resolve_model_path()
    expected = server_dir / "models" / "sensevoice-small.onnx"
    ok = got.resolve() == expected.resolve()
    print(f"  expected: {expected}")
    print(f"  got:      {got}")
    print(f"  {'✅' if ok else '❌'}")
    return ok


def test_t2_9_main_resolve_model_path_env_override(main_mod) -> bool:
    print("\nT2.9: main.py _resolve_model_path() ENV 覆盖")
    with tempfile.TemporaryDirectory() as tmp:
        with _temp_env(GHOSTTYPE_BASE_PATH=tmp):
            got = main_mod._resolve_model_path()
        expected = Path(tmp).expanduser() / "models" / "sensevoice-small.onnx"
        ok = got.resolve() == expected.resolve()
        print(f"  expected: {expected}")
        print(f"  got:      {got}")
        print(f"  {'✅' if ok else '❌'}")
        return ok


def main() -> None:
    _configure_stdout()

    print("=" * 60)
    print("Layer 2: 打包相关功能测试")
    print("=" * 60)

    server_dir, server_entry, gui_mod, main_mod = _import_modules()

    tests = [
        ("T2.1", lambda: test_t2_1_is_frozen_source_mode(server_entry)),
        ("T2.2", lambda: test_t2_2_resolve_base_path_source_mode(server_dir, server_entry)),
        ("T2.3", lambda: test_t2_3_resolve_base_path_env_override(server_entry)),
        ("T2.4", lambda: test_t2_4_model_path(server_dir, server_entry)),
        ("T2.5", lambda: test_t2_5_server_entry_main_exit_code_2_when_model_missing(server_entry)),
        ("T2.6", lambda: test_t2_6_gui_server_command_source_mode(gui_mod)),
        ("T2.7", lambda: test_t2_7_gui_runtime_base_path_consistent(server_entry, gui_mod)),
        ("T2.8", lambda: test_t2_8_main_resolve_model_path_source_mode(server_dir, main_mod)),
        ("T2.9", lambda: test_t2_9_main_resolve_model_path_env_override(main_mod)),
    ]

    passed = 0
    failed = 0
    for name, fn in tests:
        try:
            if fn():
                passed += 1
            else:
                failed += 1
        except Exception as exc:
            failed += 1
            print(f"\n{name}: ❌ 异常: {exc}")

    print("\n" + "=" * 60)
    print(f"结果: {passed} 通过, {failed} 失败")
    print("=" * 60)
    if failed:
        raise SystemExit(1)
    raise SystemExit(0)


if __name__ == "__main__":
    main()

