# Layer 4: 打包前最终检查清单

## 文件检查

### Server 目录
- [ ] `models/sensevoice-small.onnx` 存在
- [ ] `models/sensevoice-small.onnx` 大小 > 50MB
- [ ] `models/tokens.txt` 存在 (如需要)
- [ ] `models/_downloads/` 已删除或清空 (节省空间)
- [ ] `bin/` 目录不存在或为空 (不需要 ffmpeg.exe)
- [ ] 无 `__pycache__/` 残留 (可选清理)
- [ ] 无 `.pyc` 文件残留 (可选清理)

### Client 目录
- [ ] `src-tauri/icons/` 下有图标文件
- [ ] `config.json` 配置正确

### 根目录
- [ ] 无 `nul` / `con` / `prn` 等幽灵文件
- [ ] 无临时测试文件残留

---

## 依赖检查

### Server `requirements.txt`
- [ ] 包含 `onnxruntime-directml` (不是 `onnxruntime-gpu`)
- [ ] 包含 `av` (PyAV)
- [ ] 包含 `numpy`
- [ ] 包含 `fastapi`
- [ ] 包含 `uvicorn[standard]`
- [ ] 无多余未使用依赖 (如 `httpx`/`pyyaml` 未使用可后续清理)

### Client `Cargo.toml`
- [ ] 所有依赖版本锁定
- [ ] 无未使用的依赖

---

## 代码检查

- [ ] 无调试用 `print` 语句残留 (或已改为 logging)
- [ ] 无硬编码绝对路径 (如 `C:\\Users\\xxx\\...`)
- [ ] 无 `TODO` / `FIXME` 遗留 (或已知并记录)
- [ ] 托盘图标代码已添加

---

## 功能验证 (快速复核)

- [ ] Server 启动显示 “ASR engine loaded”
- [ ] Server 日志确认使用 GPU (DirectML/CUDA) 或明确 CPU 降级原因
- [ ] Client 托盘图标正常显示
- [ ] 按住说话能识别
- [ ] 文字能正确输入到焦点窗口
- [ ] 断线能自动重连

---

## 性能确认

- [ ] 端到端延迟 < 500ms
- [ ] ASR 单次推理 < 300ms (理想值)

---

## 签字确认

程序员签名: _________________  日期: ___________

架构师签名: _________________  日期: ___________

