# Whisper

**Whisper** 是面向 Oopz 的 Windows 原生轻量级游戏内战术文字浮层客户端（v2.0.0），采用 **纯 Rust + Win32 DirectComposition / Direct2D / DirectWrite** 从零重写，单文件免安装、零外部运行库依赖。

## 核心特性

- **极致轻快**：单文件 `Whisper.exe`（< 3.5 MB），零临时解压毫秒级启动，平时空闲 **0% 重绘**（0% CPU / 0% GPU），常驻内存极低。
- **双态战术 HUD**：
  - **平时穿透态**：零矩形边框、无遮挡 100% 鼠标穿透（`WS_EX_TRANSPARENT`），按停靠角落平滑绘制径向暗晕，发言人按 UID 稳定着色，纯白正文配紧致深色描边，亮沙地与暗夜场景皆清晰锐利。
  - **呼出交互态（默认 `F8`）**：瞬时呼出深色半透明面板，支持 `Tab` 或点击顶部 `# 频道名 ▾` 快切当前服务器文字频道、滚轮翻阅历史与 `↓ N 条新消息` 回底药丸、`Ctrl+滚轮` 调字号、点击自动识别的链接/房间码微胶囊一键复制，以及原生 Win32 `IMM32` 输入法精准跟随与防误发；发送或按 `Esc` 后自动归还前台焦点给游戏窗口。
- **原生 Direct2D 设置窗口**：内置 **1:1 实时预览舞台**（支持亮色沙地 / 暗夜室内双检验背景与屏幕实景拖拽定位）、服务器与文字频道双下拉选择、点击录制的全局热键、四象限停靠角落点选、Windows 系统提示音选择与 `▶ 试听`、开机静默自启及安全自更新。
- **1.x 无损升级**：支持线上 1.0.4 老用户平滑升级，首次启动自动解密并无损迁移 `%LOCALAPPDATA%\RinChan\OopzTacticalOverlay\state.bin` 下的全部配置与提示音选择。

## 构建说明

```bash
# 运行核心协议签名与状态机单元测试
cargo test --lib

# 交叉编译 Windows 发布产物（x86_64 与 aarch64）
cargo xwin build --target x86_64-pc-windows-msvc --release
cargo xwin build --target aarch64-pc-windows-msvc --release
```
