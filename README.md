# GhostType

MVP: hold `CapsLock` to talk, release to type; server returns fast ASR text then LLM correction.

## Repo Layout

- `client/`: Tauri (Rust) client
- `server/`: FastAPI server
- `docs/`: architecture + MVP docs + `docs/protocol.md`

## Quick Start (Windows)

### 1) Server

```powershell
cd server
python -m venv venv
.\venv\Scripts\Activate.ps1
pip install -r requirements.txt
.\run.bat
```

Server listens on `ws://0.0.0.0:8000/ws`.

Audio verification (Phase 1): dump decoded WAV to temp

```powershell
$env:GHOSTTYPE_DUMP_WAV="1"
# optional:
# $env:GHOSTTYPE_DUMP_WAV_DIR="C:\\Temp"
.\run.bat
```

### 2) Client

Edit `client/config.json` and set your LAN/Tailscale endpoints:

```json
{ "server_endpoints": ["ws://127.0.0.1:8000/ws"], "use_cloud_api": false, "hotkey": "capslock" }
```

Run:

```powershell
cd client
npm install
npm run tauri dev
```

The window is hidden by default; use the tray menu `Show/Hide/Quit`.

## Status

- Audio path is complete: server decodes Ogg/Opus in-memory via PyAV and produces 16kHz PCM.
- ASR is enabled when `server/models/sensevoice-small.onnx` exists; otherwise server falls back to stub text.
- Protocol spec: `docs/protocol.md`.

## Dependencies

- Server audio decode uses PyAV (`av`) + `numpy` (no external/system `ffmpeg` required for end users).
