# 发行与自动更新验收（2.x）

完成本地构建不代表用户能够自动更新。每次发行必须完成线上发布和旧版升级验证，再报告交付完成。

## 发布内容

- 保持检查入口：`https://acv.k-neco.com/tools/oopz-tactical-overlay/latest.json`。
- 新版程序先发布到版本目录，例如 `2.0.0/OopzTacticalOverlay.exe`，清单的 `url` 指向该文件。版本目录中的程序发布后不再修改。
- 清单字段为 `version`、`url`、`size`、`sha256`、`publishedAt`，与 1.x 相同，1.x 用户的旧版 helper 才能读取。`url` 必须以 `https://acv.k-neco.com/` 开头。
- 清单中的版本、文件大小和 SHA-256 必须对应已验证的 Windows 发行包；`publishedAt` 使用实际发布时间。
- 保持原下载入口 `OopzTacticalOverlay.exe` 可用，并同步为新版。
- 更换线上文件前，在公开目录之外备份旧程序和旧清单。临时文件写完并校验后，再原子替换入口文件。

静态站点沿用现有 `archive` 服务；发布程序不需要重启服务或改动其他应用。

## 构建

```sh
python tools/build/build_windows.py     # Windows 上执行，产物 OopzTacticalOverlay.exe（Nuitka onefile）
python -m pytest tests                  # 纯逻辑测试，发行前必须全绿
```

版本号在 `pyproject.toml` 与 `src/typeslip/__init__.py` 中保持一致；CHANGELOG 写用户能看懂的一段。

## 必须完成的验证

1. 从公共 HTTPS 地址重新读取清单，确认版本、下载地址、大小和 SHA-256；不能用本地清单代替。
2. 在隔离的 Windows 用户状态目录启动上一发行版，让其使用真实公共更新入口。
3. 验证旧版自动下载完成，待更新记录和下载文件校验正确，运行中的程序尚未被替换。
4. 正常退出并重新启动，确认 helper 替换原文件、启动新版本、浮层显示「纸条已更新到 x.y.z」。
5. 核对原设置保留、新版浮层与设置窗正常、退出后热键释放。
6. 记录测试环境、前后版本、发行包 SHA-256 和实际结果；清理本轮测试任务、进程和临时目录。

还应验证实际安装入口，尤其是程序与更新暂存目录位于不同磁盘的情况。helper 先在程序所在盘复制并重新校验更新文件，再替换入口文件；不能直接对跨盘路径调用 `os.replace`。

## 用户看到的流程

程序启动约 3 秒后检查更新，之后每 6 小时检查一次。下载完成后不打扰聊天，下次启动时由 helper 替换并启动新版。已经打开的程序不会在使用中强制重启。

因此，发布时已打开的旧版需要先重启一次触发检查，下载完成后再退出并启动完成替换。

## 2.0.0 记录

2.0 是白纸重做，代码在 `src/typeslip/`，1.x 旧代码保留在 `legacy/`，不参与构建。

- 设置文件为 `%LOCALAPPDATA%\RinChan\OopzTacticalOverlay\settings.json`（明文，不含凭据）。首次启动时若存在 1.x 的加密 `state.bin`，迁移一次：频道、热键、字号、透明度、显示器和提示音开关保留；1.x 的坐标与「始终可见」不再生效。
- 登录态仍只从本机 Oopz 读取，且要求 Oopz.exe 正在运行。
- 实机核对表见 `docs/verify-2.0.md`，未通过的步骤不发布。
