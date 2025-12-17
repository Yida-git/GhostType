# GhostType 项目审计 TODO（Mac 适配收尾）

> 目的：把“审计报告与任务规划”落成一份可执行的待办清单（含优先级/负责人/验收口径/依赖关系）。
>
> 建议使用方式：每完成一项就在对应项打勾，并在 PR / 提交说明里附上验证结果。

## 0. 总体验收标准（Definition of Done）

### Mac 客户端验收
- `npm run tauri build` 无错误完成
- 编译无警告（或明确抑制且可解释）
- 无需 `brew install opus` 即可构建
- 首次运行：辅助功能权限请求/引导生效
- 首次录音：麦克风权限弹窗生效（Info.plist 生效）
- `F8` 热键可正常触发录音流程
- 可连接服务端并收发消息
- 识别结果可注入到目标应用

### 测试文件验收
- Layer 0-4 测试均可在 Mac 上执行
- Checklist 包含 Mac 特定步骤
- 无 Windows-only 的硬编码假设

## 1. 已完成（来自当前审计结论）
- [x] 依赖替换：`active-win` → `active-win-pos-rs`，`enigo 0.1` → `enigo 0.6`
- [x] 音频模块重构：解决 `cpal::Stream` 非 `Send` 问题（独立线程持有）
- [x] 权限检测：macOS 辅助功能检测已收敛到 `client/src-tauri/src/platform/macos.rs`
- [x] 热键适配：macOS 默认 `F8`，并添加 CapsLock 警告
- [x] 配置更新：`config.json` 默认热键改为 `F8`，bundle id 修复
- [x] `Info.plist`：新增麦克风权限声明（`client/src-tauri/Info.plist`）
- [x] 构建验证：`npm run tauri build` 可成功打包

## 2. TODO（按优先级）

### P0（阻塞验收 / 关键路径）
- [x] A2（开发）：在 `client/src-tauri/tauri.conf.json` 配置 `bundle.macOS.infoPlist`
  - 验收口径：打包后应用首次录音会弹出麦克风权限请求；系统设置可看到对应权限描述文案
  - 验证建议：安装打包产物后触发录音；或查看 `.app/Contents/Info.plist` 是否包含 `NSMicrophoneUsageDescription`
- [x] A3（开发）：确保 Opus 无外部依赖且静态链接
  - 实施：非 Windows 使用 vendored `opus-sys`（`client/src-tauri/vendor/opus-sys`）构建静态 `libopus`，Windows 继续使用 `audiopus`
  - 验收口径：在“全新/无 brew opus”的 macOS 环境中 `cargo build` 成功；运行时不依赖系统 `libopus`
- [x] B1（开发）：更新 `server/tests/test_layer0_env.py`，补全 macOS CoreML provider 检查
  - 验收口径：在 Mac 上运行 Layer0 能正确判断 `onnxruntime` provider（至少 CPU；如支持 CoreML 则输出提示）
- [x] B2（开发）：更新 `docs/test_layer2_checklist.md`，移除 Windows-only 假设并补齐 macOS 步骤
  - 验收口径：Layer2 清单中明确：热键（F8）、辅助功能/麦克风权限、注入/回退按键行为的 macOS 操作步骤
- [ ] D1（测试）：Mac 上运行 Layer 0 环境验证
- [ ] D2（测试）：Mac 上运行 Layer 1 服务端测试

### P0（本轮新增：Server/Client GUI + Windows）
- [x] E1（测试）：新增 Layer 2 打包单测：`server/tests/test_layer2_packaging.py`
  - 验收口径：无需启动服务端即可运行；覆盖 `server_entry/gui/main` 的路径解析与打包逻辑
- [x] E2（开发）：服务端 GUI 设置界面（host/port/log_level）
  - 验收口径：GUI 可编辑配置并保存到 `config.json`；点击“保存并重启服务”后服务端重启生效
- [x] E3（开发）：服务端 GUI 状态显示（运行状态/连接数/模型/Provider/ASR 耗时）
  - 验收口径：状态可随日志实时更新；重启后状态能重置并恢复
- [x] E4（开发）：客户端权限引导（macOS 辅助功能 + 麦克风；Windows 麦克风/设备引导）
  - 验收口径：权限缺失时窗口自动弹出引导；支持一键打开系统设置并可刷新状态
- [x] E5（开发）：客户端设置界面增强（连接测试 + 音频输入设备选择 + 状态卡片）
  - 验收口径：可测试服务器连接；可选择音频输入设备并写入配置（重启生效）
- [x] E6（开发）：客户端日志文件输出（`GHOSTTYPE_LOG_FILE=1`）
  - 验收口径：写入 `<exe_dir>/logs/ghosttype_client.log`，并在文件过大时自动轮转
- [ ] E7（测试）：Windows 环境端到端测试
  - 验收口径：按 `docs/test_windows_e2e_checklist.md` 执行并记录结果

### P1（体验/质量提升）
- [x] A1（开发/核对）：完善打包场景下 `config.json` 搜索（`client/src-tauri/src/config.rs`）
  - 说明：已补充 macOS `.app/Contents/Resources/config.json` 候选路径
  - 验收口径：打包后无需手动设置 `GHOSTTYPE_CONFIG` 也能读取到预期配置（或给出明确放置/读取约定）
- [x] A4（开发）：消除编译警告（当前审计：4 个 `dead_code`）
  - 方案 A：删除无用代码/字段（推荐，长期可维护）
  - 方案 B：局部 `#[allow(dead_code)]`（短期止血，需写清原因）
  - 验收口径：`cargo check`/`tauri build` 无警告（或可解释抑制）
- [x] A5（开发）：补齐 Retina 托盘图标（`64x64` / `@2x`）
  - 验收口径：macOS Retina 屏托盘图标清晰，无明显模糊
- [x] B3（开发）：更新 `docs/test_layer4_checklist.md`：多平台依赖检查与 GPU/CoreML 说明
- [x] C1（开发）：创建 `server/requirements-macos.txt`（替换 DirectML 依赖）
  - 验收口径：Mac 可按该文件安装并运行 Layer0/Layer1；依赖描述清晰（CPU/可选 CoreML）
- [ ] D3（测试）：Mac 上执行 Layer 2 人工测试
- [ ] D4（测试）：Mac 上执行 Layer 3 端到端测试
- [ ] D5（测试）：Mac 上执行 Layer 4 打包前检查

### P2（可维护性 / 可扩展）
- [x] A6（开发，可选）：创建平台抽象层 `client/src-tauri/src/platform/mod.rs`（为 Linux 预留）
  - 验收口径：平台相关实现收敛到 `platform/*`；业务层只依赖统一接口
- [x] A7（开发，可选）：统一日志输出
  - 实施：客户端引入 `log`，并提供 `client/src-tauri/src/logging.rs` 的零依赖 logger（支持 `GHOSTTYPE_LOG`/`RUST_LOG`）
  - 验收口径：不再混用 `println!/eprintln!`；日志级别可配置；关键路径带上下文信息
- [x] C2（开发，可选）：服务端 ASR 动态选择 provider（优先 CoreML，回退 CPU）
  - 说明：`server/app/core/asr.py` 默认 provider 顺序已包含 `CoreMLExecutionProvider`
  - 验收口径：Mac 上输出实际启用的 providers，并在不可用时自动回退 CPU

## 3. 依赖关系与关键路径

关键路径（建议以此为先）：`A2 + A3 → B1/B2 → D1 → D2 → D3 → D4 → D5`

依赖关系（来自审计规划，便于排程）：
- `A2 → D5`
- `A3 → D1`
- `B1 → D1`
- `B2 → D3`
- `B3 → D5`
- `C1 → D1`

## 4. 推荐执行顺序（面向交付）
1) 先做 `A2`（麦克风权限是否生效属于“打包即爆雷”的问题）  
2) 再做 `A3`（避免“换机器/换环境就无法构建”的阻塞）  
3) 并行推进 `B1` + `B2`（为测试阶段扫清脚本与清单障碍）  
4) 进入 `D1/D2`（尽快把关键路径跑通）  
5) 收尾 `A4/A5/B3/C1`，最后做 `D3/D4/D5`  
6) 若计划扩展平台与可维护性，再做 `A6/A7/C2`
