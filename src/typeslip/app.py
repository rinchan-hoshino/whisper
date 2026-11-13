"""纸条 2.0 入口：组装 Oopz 连接、HUD、热键、提示音、托盘、设置与自动更新。"""

from __future__ import annotations

import json
import logging
import os
import sys
import threading
import time
from dataclasses import replace
from pathlib import Path

from PySide6.QtCore import QAbstractNativeEventFilter, QObject, QTimer
from PySide6.QtGui import QAction, QIcon
from PySide6.QtNetwork import QLocalServer, QLocalSocket
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon

from . import PRODUCT, __version__
from .adapters import dpapi, session, updater, win32
from .adapters.oopz import OopzLink, Target
from .core import settings as settings_store
from .core.hotkeys import HotkeyError, parse_hotkey
from .core.sound import scale_wav
from .core.timeline import ChatMessage, Timeline
from .core.updates import is_newer
from .ui.hud import Hud
from .ui.settings_dialog import SettingsDialog

LOGGER = logging.getLogger("typeslip")
ASSETS = Path(__file__).resolve().parent / "assets"
SINGLE_NAME = "TypeSlip-2.0-single"
UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000
OPEN_ID, TOGGLE_ID = 1, 2


def state_dir() -> Path:
    root = Path(os.environ.get("LOCALAPPDATA", Path.home()))
    return root / "RinChan" / PRODUCT


def migrate_if_needed(directory: Path) -> None:
    """1.x 的 state.bin 是 DPAPI 加密的 JSON；迁移一次后写 settings.json。"""
    target = directory / "settings.json"
    legacy = directory / "state.bin"
    if target.exists() or not legacy.is_file() or os.name != "nt":
        return
    try:
        raw = json.loads(dpapi.unprotect(legacy.read_bytes()).decode("utf-8"))
        if isinstance(raw, dict):
            settings_store.save(target, settings_store.migrate_legacy(raw))
    except (OSError, ValueError) as exc:
        LOGGER.warning("legacy settings not migrated", exc_info=exc)


class NativeHotkeys(QAbstractNativeEventFilter):
    def __init__(self, on_hotkey) -> None:
        super().__init__()
        self.on_hotkey = on_hotkey

    def nativeEventFilter(self, event_type, message):  # noqa: N802
        if event_type not in (b"windows_generic_MSG", b"windows_dispatcher_MSG"):
            return False, 0
        hotkey_id = win32.is_hotkey_message(message)
        if hotkey_id is None:
            return False, 0
        self.on_hotkey(hotkey_id)
        return True, 0


class App(QObject):
    def __init__(self, application: QApplication, directory: Path) -> None:
        super().__init__()
        self.app = application
        self.dir = directory
        self.dir.mkdir(parents=True, exist_ok=True)
        self.staging = self.dir / "updates"
        migrate_if_needed(self.dir)
        if getattr(sys, "frozen", False):
            try:
                Path(sys.argv[0]).resolve().with_suffix(".old").unlink(missing_ok=True)
            except OSError:
                pass  # 上次替换留下的旧程序，仍被占用就下次再删
        self.settings = settings_store.load(self.dir / "settings.json")
        self.timeline = Timeline(capacity=200, visible_limit=6, fade_after=10.0)
        self.hud = Hud(self.timeline)
        self.hud.apply_style(self.settings.font_size, self.settings.hud_opacity, self.settings.hud_corner)
        self.hud.place(self.settings.monitor)
        self.hud.submitted.connect(self._on_submit)
        self.hud.cancelled.connect(self._cancel_input)
        self.hud.show()
        self.current: Target | None = None
        self.link: OopzLink | None = None
        self.game_window = 0
        self.settings_dialog: SettingsDialog | None = None
        self.sound_bytes = (ASSETS / "ding-dong.wav").read_bytes()
        self._registered: list[int] = []

        self.native = NativeHotkeys(self._on_hotkey)
        self.app.installNativeEventFilter(self.native)
        self._register_hotkeys()

        self.tray = QSystemTrayIcon(QIcon(str(ASSETS / "icon.ico")), self)
        menu = QMenu()
        open_action = QAction("设置", menu)
        open_action.triggered.connect(self.open_settings)
        quit_action = QAction("退出", menu)
        quit_action.triggered.connect(self.quit)
        menu.addAction(open_action)
        menu.addAction(quit_action)
        self.tray.setContextMenu(menu)
        self.tray.show()

        self.pump = QTimer(self)
        self.pump.timeout.connect(self._pump)
        self.pump.start(100)
        self.retry = QTimer(self)
        self.retry.timeout.connect(self._ensure_link)
        self.retry.start(5000)
        self.update_timer = QTimer(self)
        self.update_timer.timeout.connect(self._check_update)
        self.update_timer.start(UPDATE_INTERVAL_MS)
        self._show_updated_notice()
        self._ensure_link()
        QTimer.singleShot(3000, self._check_update)

    # ---- Oopz 连接 ----
    def _ensure_link(self) -> None:
        if self.link is not None:
            return
        try:
            login = session.parse_login_value(session.read_login_value())
        except session.SessionError as exc:
            self.hud.set_connected(False, str(exc))
            return
        pinned = None
        if self.settings.channel_mode == "pinned" and self.settings.channel_id:
            pinned = Target(self.settings.area_id, self.settings.area_name,
                            self.settings.channel_id, self.settings.channel_name)
        link = OopzLink()
        link.start(login, self.settings.channel_mode, pinned)
        self.link = link
        self.hud.set_connected(False, "正在连接 Oopz")

    def _restart_link(self) -> None:
        if self.link is not None:
            self.link.stop()
            self.link = None
        self.current = None
        self._ensure_link()

    def _pump(self) -> None:
        if self.link is None:
            return
        for event in self.link.drain():
            kind = event[0]
            if kind == "status":
                state = event[1]
                if state == "connected":
                    self.hud.set_connected(True, "")
                elif state == "no_voice":
                    self.hud.set_connected(False, "先进入 Oopz 语音频道")
                elif state == "connecting":
                    self.hud.set_connected(False, "正在连接 Oopz")
                else:
                    self.hud.set_connected(False, "Oopz 已断开，正在重连")
            elif kind == "target":
                self.current = event[1]
            elif kind == "history":
                self.timeline.replace_all(event[1], time.time())
                self.hud.update()
            elif kind == "message":
                message, stamp = event[1], event[2]
                self.timeline.add(message, stamp)
                if not message.own:
                    self._play_sound()
                self.hud.update()
            elif kind == "sent":
                self.hud.leave_input(keep_draft=False)
                self._give_back_focus()
            elif kind == "send_failed":
                self.hud.set_draft(event[1])
                self.hud.show_error(event[2])

    # ---- 输入 ----
    def _on_hotkey(self, hotkey_id: int) -> None:
        if hotkey_id == OPEN_ID:
            if not self.hud.input_mode:
                self.game_window = win32.foreground_window()
                self.hud.show_error("")
                self.hud.enter_input()
                if self.hud.draft:
                    self.hud.set_draft(self.hud.draft)
            else:
                self._cancel_input()
        elif hotkey_id == TOGGLE_ID:
            self.hud.setVisible(not self.hud.isVisible())

    def _on_submit(self, text: str) -> None:
        if self.link is None:
            self.hud.show_error("先打开 Oopz 并登录")
            return
        self.hud.show_error("")
        self.link.send(text)

    def _cancel_input(self) -> None:
        self.hud.leave_input(keep_draft=True)
        self._give_back_focus()

    def _give_back_focus(self) -> None:
        if self.game_window:
            win32.focus_window(self.game_window)

    def _register_hotkeys(self) -> None:
        for hotkey_id in self._registered:
            win32.unregister_hotkey(hotkey_id)
        self._registered = []
        for hotkey_id, text in ((OPEN_ID, self.settings.open_hotkey), (TOGGLE_ID, self.settings.toggle_hotkey)):
            try:
                spec = parse_hotkey(text)
            except HotkeyError as exc:
                LOGGER.warning("hotkey invalid: %s", exc)
                continue
            if win32.register_hotkey(hotkey_id, spec):
                self._registered.append(hotkey_id)
            else:
                LOGGER.warning("hotkey busy: %s", text)

    # ---- 提示音 ----
    def _play_sound(self) -> None:
        if self.settings.sound_enabled and self.settings.sound_volume > 0:
            win32.play_wav(scale_wav(self.sound_bytes, self.settings.sound_volume / 100.0))

    # ---- 设置 ----
    def open_settings(self) -> None:
        if self.settings_dialog is not None:
            self.settings_dialog.raise_()
            return
        label = self.current.channel_name if self.current else self.settings.channel_name
        dialog = SettingsDialog(self.settings, label)
        self.settings_dialog = dialog
        dialog.finished.connect(lambda result: self._settings_closed(dialog, result))
        dialog.show()

    def _settings_closed(self, dialog: SettingsDialog, result: int) -> None:
        self.settings_dialog = None
        if not result:
            return
        new = dialog.result_settings
        if dialog.pin_requested and self.current is not None:
            new = replace(new, area_id=self.current.area_id, area_name=self.current.area_name,
                          channel_id=self.current.channel_id, channel_name=self.current.channel_name)
        if new.channel_mode == "pinned" and not new.channel_id:
            new = replace(new, channel_mode="follow")
        relink = (new.channel_mode, new.channel_id, new.area_id) != (
            self.settings.channel_mode, self.settings.channel_id, self.settings.area_id)
        self.settings = new
        settings_store.save(self.dir / "settings.json", new)
        self.hud.apply_style(new.font_size, new.hud_opacity, new.hud_corner)
        self.hud.place(new.monitor)
        self._register_hotkeys()
        if relink:
            self._restart_link()

    # ---- 自动更新 ----
    def _show_updated_notice(self) -> None:
        record = self.staging / "updated.json"
        if not record.is_file():
            return
        try:
            version = json.loads(record.read_text(encoding="utf-8"))["version"]
        except (ValueError, KeyError, OSError):
            version = ""
        record.unlink(missing_ok=True)
        if version == __version__:
            self.timeline.add(ChatMessage("update", "纸条", f"纸条已更新到 {version}"), time.time())
            self.hud.update()

    def _check_update(self) -> None:
        pending = updater.staged_update(self.staging, __version__)
        if pending is not None:
            self._apply_pending(pending)
            return
        threading.Thread(target=self._download_latest, name="typeslip-update", daemon=True).start()

    def _download_latest(self) -> None:
        try:
            manifest = updater.fetch_manifest()
            if not is_newer(manifest.version, __version__):
                return
            updater.download_verified(manifest, self.staging)
            updater.record_pending(self.staging, manifest)
        except Exception as exc:  # noqa: BLE001 - 更新失败不影响聊天
            LOGGER.info("update check failed", exc_info=exc)

    def _apply_pending(self, pending) -> None:
        if not getattr(sys, "frozen", False):
            return
        manifest, staged = pending
        exe = Path(sys.argv[0]).resolve()
        updater.launch_helper(exe, staged, exe, os.getpid(), manifest.version)
        self.quit()

    def quit(self) -> None:
        if self.link is not None:
            self.link.stop()
        for hotkey_id in self._registered:
            win32.unregister_hotkey(hotkey_id)
        self.app.quit()


def _single_instance(app: QApplication, on_show) -> bool:
    socket = QLocalSocket()
    socket.connectToServer(SINGLE_NAME)
    if socket.waitForConnected(300):
        socket.write(b"show")
        socket.flush()
        socket.waitForBytesWritten(300)
        return False
    QLocalServer.removeServer(SINGLE_NAME)
    server = QLocalServer(app)
    server.listen(SINGLE_NAME)

    def accept() -> None:
        connection = server.nextPendingConnection()
        if connection is not None:
            connection.waitForReadyRead(300)
            on_show()
            connection.close()

    server.newConnection.connect(accept)
    return True


def run() -> int:
    logging.basicConfig(level=logging.INFO)
    application = QApplication(sys.argv)
    application.setQuitOnLastWindowClosed(False)
    application.setApplicationName("纸条")
    holder: dict[str, App] = {}
    if not _single_instance(application, lambda: holder["app"].open_settings()):
        return 0
    holder["app"] = App(application, state_dir())
    return application.exec()
