# GhostType

**Voice-to-text input tool with real-time ASR and LLM correction.**

Hold a hotkey, speak, release — your words appear as text in any application. GhostType captures audio, streams it to a local server for fast speech recognition, and injects the transcribed text directly into the active window.

## What It Does

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                              GhostType Flow                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   [Hold F8]  →  [Speak]  →  [Release F8]  →  [Text appears in any app]     │
│                                                                             │
│   ┌──────────────────┐         WebSocket          ┌───────────────────┐    │
│   │  Client (Tauri)  │ ──── Opus Audio Stream ──→ │  Server (Python)  │    │
│   │                  │                            │                   │    │
│   │  • Hotkey listen │                            │  • Opus decode    │    │
│   │  • Audio capture │ ←───── ASR Text ────────── │  • SenseVoice ASR │    │
│   │  • Text injection│                            │  • (LLM correct)  │    │
│   └──────────────────┘                            └───────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Features

- **Push-to-talk**: Hold hotkey to record, release to transcribe
- **Fast local ASR**: SenseVoice ONNX model for low-latency speech recognition
- **Universal text injection**: Works in any application (editors, browsers, chat apps)
- **Cross-platform client**: macOS and Windows support
- **Privacy-first**: All processing happens locally, no cloud dependency
- **Opus compression**: Efficient audio streaming over network

## Current Status

> **Stage: MVP (Minimum Viable Product) — macOS Adaptation Complete**

| Component | Status | Notes |
|-----------|--------|-------|
| Audio capture | ✅ Done | cpal + Opus encoding |
| Hotkey detection | ✅ Done | F8 (macOS), CapsLock (Windows) |
| WebSocket streaming | ✅ Done | Auto-reconnect with backoff |
| Opus decode (server) | ✅ Done | PyAV, no system ffmpeg needed |
| ASR inference | ✅ Done | SenseVoice ONNX (model required) |
| Text injection | ✅ Done | enigo library |
| macOS permissions | ✅ Done | Accessibility + Microphone |
| LLM correction | 🚧 Planned | Track B: Ollama Qwen2.5 |
| Linux client | 🚧 Planned | Platform abstraction ready |

### What Works Now

1. Hold F8 (macOS) or CapsLock (Windows) to start recording
2. Speak into microphone
3. Release key to trigger transcription
4. Text appears at cursor position in active application

### What's Missing

- LLM-based text correction (planned "Track B")
- Tray icon status colors (recording/processing/ready)
- Linux client build

## Quick Start

### Prerequisites

- **Client**: Node.js 18+, Rust toolchain
- **Server**: Python 3.10+
- **ASR Model**: `sensevoice-small.onnx` (place in `server/models/`)

### 1. Server Setup

#### macOS

```bash
cd server
python3 -m venv venv
source venv/bin/activate
pip install -r requirements-macos.txt

# Download model to server/models/sensevoice-small.onnx
# Then start server:
uvicorn app.main:app --host 0.0.0.0 --port 8000
```

#### Windows

```powershell
cd server
python -m venv venv
.\venv\Scripts\Activate.ps1
pip install -r requirements.txt

# Download model to server\models\sensevoice-small.onnx
.\run.bat
```

Server listens on `ws://0.0.0.0:8000/ws`.

### 2. Client Setup

Edit `client/config.json`:

```json
{
  "server_endpoints": ["ws://127.0.0.1:8000/ws"],
  "use_cloud_api": false,
  "hotkey": "f8"
}
```

Build and run:

```bash
cd client
npm install
npm run tauri dev
```

The window is hidden by default. Use the system tray menu: **Show / Hide / Quit**.

### 3. Permissions (macOS)

On first launch, grant these permissions in **System Settings → Privacy & Security**:

- **Accessibility**: Required for global hotkey and text injection
- **Microphone**: Required for audio capture

## Repository Structure

```text
GhostType/
├── client/                     # Tauri + Rust desktop client
│   ├── src-tauri/
│   │   ├── src/
│   │   │   ├── main.rs         # Entry point, hotkey handling
│   │   │   ├── audio.rs        # Microphone capture + Opus encoding
│   │   │   ├── network.rs      # WebSocket client
│   │   │   ├── input.rs        # Keyboard injection (enigo)
│   │   │   ├── config.rs       # Configuration loading
│   │   │   ├── platform/       # macOS/Windows/Linux abstraction
│   │   │   └── ...
│   │   ├── vendor/opus-sys/    # Static libopus (no brew required)
│   │   └── Cargo.toml
│   ├── config.json             # Runtime configuration
│   └── package.json
│
├── server/                     # FastAPI + Python server
│   ├── app/
│   │   ├── main.py             # WebSocket endpoint
│   │   ├── core/
│   │   │   ├── asr.py          # SenseVoice ONNX engine
│   │   │   └── fbank.py        # Mel spectrogram features
│   │   └── utils/
│   │       └── audio.py        # Opus decode, Ogg container
│   ├── models/                 # Place .onnx model here
│   ├── requirements.txt        # Windows (DirectML)
│   └── requirements-macos.txt  # macOS (CPU/CoreML)
│
└── docs/
    ├── protocol.md             # WebSocket protocol spec
    └── TODO.md                 # Development task tracking
```

## Configuration

### Client (`client/config.json`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `server_endpoints` | string[] | `["ws://127.0.0.1:8000/ws"]` | Server WebSocket URLs (tries in order) |
| `use_cloud_api` | bool | `false` | Reserved for future cloud ASR |
| `hotkey` | string | `"f8"` (macOS) / `"capslock"` (Windows) | Push-to-talk key |

### Server Environment Variables

| Variable | Description |
|----------|-------------|
| `GHOSTTYPE_DUMP_WAV=1` | Save decoded audio to temp directory |
| `GHOSTTYPE_DUMP_WAV_DIR=/path` | Custom WAV dump directory |
| `GHOSTTYPE_LOG_TIMINGS=1` | Print decode/ASR timing info |
| `GHOSTTYPE_LOG=debug` | Log level (error/warn/info/debug/trace) |
| `GHOSTTYPE_LOG_FILE=1` | Enable server log file output (`logs/`) |

## Protocol

See [docs/protocol.md](docs/protocol.md) for WebSocket message format.

**Client → Server:**

- `{"type": "start", "sample_rate": 48000, ...}` — Begin recording session
- `[binary]` — Opus audio frames
- `{"type": "stop"}` — End recording, trigger ASR

**Server → Client:**

- `{"type": "fast_text", "content": "...", "is_final": true}` — ASR result
- `{"type": "correction", "delete_count": 5, "replaced_text": "..."}` — LLM fix (planned)
- `{"type": "error", "message": "..."}` — Error

## Development

### Build Client

```bash
cd client
npm run tauri build
```

Output: `client/src-tauri/target/release/bundle/`

### Run Tests

```bash
# Server environment check
cd server
python tests/test_layer0_env.py

# Server API test (requires running server)
python tests/test_layer1_server.py
```

### Debug Logging

```bash
# Client (macOS/Linux)
GHOSTTYPE_LOG=debug ./path/to/ghosttype-client

# Client (Windows)
$env:GHOSTTYPE_LOG="debug"
.\ghosttype-client.exe
```

## Tech Stack

**Client:**

- [Tauri](https://tauri.app/) v2 — Desktop framework
- [cpal](https://crates.io/crates/cpal) — Cross-platform audio
- [opus-sys](https://crates.io/crates/opus-sys) — Opus encoding (vendored)
- [rdev](https://crates.io/crates/rdev) — Global hotkey
- [enigo](https://crates.io/crates/enigo) — Keyboard simulation
- [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite) — WebSocket

**Server:**

- [FastAPI](https://fastapi.tiangolo.com/) — Web framework
- [ONNX Runtime](https://onnxruntime.ai/) — Model inference
- [PyAV](https://pyav.org/) — Opus decoding
- [SenseVoice](https://github.com/FunAudioLLM/SenseVoice) — ASR model

## License

MIT

## Contributing

This is an early-stage MVP. Issues and PRs welcome.

---

*Hold to talk. Release to type. That's it.*
