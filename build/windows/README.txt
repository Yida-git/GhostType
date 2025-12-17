GhostType Server（Windows）发布说明
================================

一、系统要求
- Windows 10 / 11（64 位）
- Python：不需要（已打包成可执行文件）
- GPU（可选但推荐）：支持 DirectML 的显卡驱动环境（仅影响 ASR 性能）
- 网络/端口：默认使用 8000 端口（TCP），首次启动可能需要在防火墙放行

二、目录结构（解压后）
GhostType-Server/
  GhostTypeServer.exe
  *.dll / *.pyd ...（运行时依赖）
  models/
    sensevoice-small.onnx        （必须：ASR 模型）
    README.md                    （模型说明）

三、如何运行
1) 准备模型：
   将 ASR 模型文件放到：models\\sensevoice-small.onnx
2) 启动：
   双击运行 GhostTypeServer.exe
3) 托盘菜单：
   - Show logs：打开日志窗口
   - Quit：退出服务

四、常见问题（Troubleshooting）
1) 提示 “缺少模型文件”
   - 说明 models\\sensevoice-small.onnx 不存在或文件名不匹配
2) 启动后无法连接
   - 检查防火墙是否允许 GhostTypeServer.exe 访问网络
   - 检查 8000 端口是否被占用（可用命令：netstat -ano | findstr :8000）
3) 杀毒软件误报
   - 这是 PyInstaller 类可执行文件的常见情况，请将目录加入白名单后再运行


Quick Guide (English)
=====================

Requirements
- Windows 10/11 (x64)
- No Python required (bundled)
- Model file required: models\\sensevoice-small.onnx

How to Run
1) Put the ASR model at: models\\sensevoice-small.onnx
2) Double-click: GhostTypeServer.exe
3) Tray menu:
   - Show logs / Quit

