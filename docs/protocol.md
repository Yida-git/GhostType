# GhostType Protocol (MVP)

目标：客户端按住/松开 `CapsLock` 推流音频；服务端先返回 ASR 文本（快速上屏），再返回 LLM 修正指令（回退替换）。

## Transport

- Client <-> Server: WebSocket
- 路径：`/ws`

## Message Types

### Client -> Server (Text JSON)

#### `ping`

```json
{ "type": "ping" }
```

#### `start`

```json
{
  "type": "start",
  "sample_rate": 48000,
  "context": {
    "app_name": "Visual Studio Code",
    "window_title": "ghosttype\\main.rs"
  },
  "use_cloud_api": false
}
```

#### `stop`

```json
{ "type": "stop" }
```

### Client -> Server (Binary)

- Opus 数据包（二进制帧），连续发送直到 `stop`。

### Server -> Client (Text JSON)

#### `pong`

```json
{ "type": "pong" }
```

#### `fast_text`

```json
{
  "type": "fast_text",
  "content": "测试文本",
  "is_final": true
}
```

#### `correction`

```json
{
  "type": "correction",
  "original_text": "测试文本",
  "replaced_text": "测试文本。",
  "delete_count": 4
}
```

客户端执行：`Backspace * delete_count`，再输入 `replaced_text`。

#### `error`

```json
{ "type": "error", "message": "reason" }
```

