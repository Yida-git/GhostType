@echo off
setlocal enabledelayedexpansion

REM GhostType Server (Windows) 一键打包脚本
REM 运行方式：双击此文件，或在 cmd/powershell 中执行

chcp 65001 >nul

set "SCRIPT_DIR=%~dp0"
set "REPO_ROOT=%SCRIPT_DIR%..\.."

cd /d "%REPO_ROOT%" || (
  echo [ERROR] 无法进入仓库根目录: %REPO_ROOT%
  exit /b 1
)

echo ============================================================
echo GhostType Server Windows 打包
echo   Repo: %CD%
echo ============================================================

where python >nul 2>nul || (
  echo [ERROR] 未找到 python，请先安装 Python 3.10+ 并加入 PATH
  exit /b 1
)

python -c "import sys; raise SystemExit(0 if sys.version_info >= (3,10) else 1)" || (
  echo [ERROR] 需要 Python 3.10+ 才能打包
  exit /b 1
)

set "VENV_DIR=%SCRIPT_DIR%venv"
if not exist "%VENV_DIR%\\Scripts\\python.exe" (
  echo [INFO] 创建虚拟环境: %VENV_DIR%
  python -m venv "%VENV_DIR%" || (
    echo [ERROR] 创建虚拟环境失败
    exit /b 1
  )
)

call "%VENV_DIR%\\Scripts\\activate.bat" || (
  echo [ERROR] 激活虚拟环境失败
  exit /b 1
)

echo [INFO] 升级 pip...
python -m pip install -U pip >nul || (
  echo [ERROR] pip 升级失败
  exit /b 1
)

echo [INFO] 安装依赖（server/requirements.txt）...
python -m pip install -r server\\requirements.txt || (
  echo [ERROR] 依赖安装失败
  exit /b 1
)

echo [INFO] 安装 PyInstaller...
python -m pip install pyinstaller || (
  echo [ERROR] PyInstaller 安装失败
  exit /b 1
)

echo [INFO] 开始打包（build/windows/server.spec）...
python -m PyInstaller --noconfirm --clean ^
  --distpath build\\windows\\dist ^
  --workpath build\\windows\\build ^
  build\\windows\\server.spec || (
  echo [ERROR] PyInstaller 打包失败
  exit /b 1
)

set "DIST_DIR=%SCRIPT_DIR%dist\\GhostTypeServer"
if not exist "%DIST_DIR%\\GhostTypeServer.exe" (
  echo [ERROR] 未找到输出文件: %DIST_DIR%\\GhostTypeServer.exe
  exit /b 1
)

set "RELEASE_DIR=%REPO_ROOT%\\releases\\windows\\GhostType-Server"
echo [INFO] 准备发布目录: %RELEASE_DIR%
if exist "%RELEASE_DIR%" (
  rmdir /s /q "%RELEASE_DIR%"
)
mkdir "%RELEASE_DIR%" || (
  echo [ERROR] 创建发布目录失败
  exit /b 1
)

echo [INFO] 复制打包产物...
xcopy /e /i /y "%DIST_DIR%\\*" "%RELEASE_DIR%\\" >nul || (
  echo [ERROR] 复制打包产物失败
  exit /b 1
)

REM 复制模型目录（如果存在模型文件，会一并复制；否则只复制 README 提示）
if exist "server\\models\\README.md" (
  if not exist "%RELEASE_DIR%\\models" mkdir "%RELEASE_DIR%\\models" >nul
  copy /y "server\\models\\README.md" "%RELEASE_DIR%\\models\\README.md" >nul
)
if exist "server\\models\\sensevoice-small.onnx" (
  if not exist "%RELEASE_DIR%\\models" mkdir "%RELEASE_DIR%\\models" >nul
  copy /y "server\\models\\sensevoice-small.onnx" "%RELEASE_DIR%\\models\\sensevoice-small.onnx" >nul
) else (
  echo [WARN] 未检测到 server\\models\\sensevoice-small.onnx（发布包将无法启动 ASR）
)

echo [INFO] 复制发行说明...
if exist "build\\windows\\README.txt" (
  copy /y "build\\windows\\README.txt" "%RELEASE_DIR%\\README.txt" >nul
)

echo ============================================================
echo [OK] 打包完成
echo   发布目录: %RELEASE_DIR%
echo   启动文件: %RELEASE_DIR%\\GhostTypeServer.exe
echo ============================================================

exit /b 0
