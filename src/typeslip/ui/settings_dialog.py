"""设置窗：托盘右键「设置」打开。"""

from __future__ import annotations

from dataclasses import replace

from PySide6.QtCore import Qt
from PySide6.QtGui import QGuiApplication
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFormLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QSlider, QSpinBox, QVBoxLayout,
)

from ..core.hotkeys import HotkeyError, parse_hotkey
from ..core.settings import CORNERS, Settings

CORNER_LABELS = {"bottom-left": "左下", "bottom-right": "右下", "top-left": "左上", "top-right": "右上"}


class SettingsDialog(QDialog):
    def __init__(self, settings: Settings, pinned_label: str, parent=None) -> None:
        super().__init__(parent)
        self.setWindowTitle("纸条 设置")
        self.setWindowFlag(Qt.WindowType.WindowStaysOnTopHint, True)
        self.settings = settings
        form = QFormLayout()

        self.follow = QComboBox()
        self.follow.addItem("跟随当前语音服务器的文字频道", "follow")
        self.follow.addItem(f"固定：{pinned_label or '未设置'}", "pinned")
        self.follow.setCurrentIndex(1 if settings.channel_mode == "pinned" else 0)
        self.pin_now = QPushButton("固定为当前频道")
        self.pin_now.setEnabled(bool(pinned_label))
        self.pin_requested = False
        self.pin_now.clicked.connect(self._pin_current)
        form.addRow("频道", self.follow)
        form.addRow("", self.pin_now)

        self.open_hotkey = QLineEdit(settings.open_hotkey)
        self.toggle_hotkey = QLineEdit(settings.toggle_hotkey)
        form.addRow("打开输入", self.open_hotkey)
        form.addRow("显示/隐藏", self.toggle_hotkey)

        self.corner = QComboBox()
        for key in CORNERS:
            self.corner.addItem(CORNER_LABELS[key], key)
        self.corner.setCurrentIndex(CORNERS.index(settings.hud_corner))
        form.addRow("位置", self.corner)

        self.monitor = QComboBox()
        self.monitor.addItem("主屏", "")
        for screen in QGuiApplication.screens():
            self.monitor.addItem(screen.name(), screen.name())
        index = self.monitor.findData(settings.monitor)
        self.monitor.setCurrentIndex(max(0, index))
        form.addRow("显示器", self.monitor)

        self.opacity = QSlider(Qt.Orientation.Horizontal)
        self.opacity.setRange(20, 100)
        self.opacity.setValue(settings.hud_opacity)
        form.addRow("透明度", self.opacity)

        self.font = QSpinBox()
        self.font.setRange(9, 20)
        self.font.setValue(settings.font_size)
        form.addRow("字号", self.font)

        self.sound = QCheckBox("收到别人的消息时响一声")
        self.sound.setChecked(settings.sound_enabled)
        self.volume = QSlider(Qt.Orientation.Horizontal)
        self.volume.setRange(0, 100)
        self.volume.setValue(settings.sound_volume)
        form.addRow("提示音", self.sound)
        form.addRow("音量", self.volume)

        self.error = QLabel("")
        self.error.setStyleSheet("color: #c0392b;")
        self.save = QPushButton("保存")
        self.save.clicked.connect(self._accept)
        cancel = QPushButton("取消")
        cancel.clicked.connect(self.reject)
        buttons = QHBoxLayout()
        buttons.addStretch(1)
        buttons.addWidget(cancel)
        buttons.addWidget(self.save)
        layout = QVBoxLayout(self)
        layout.addLayout(form)
        layout.addWidget(self.error)
        layout.addLayout(buttons)
        self.resize(420, 0)

    def _pin_current(self) -> None:
        self.pin_requested = True
        self.follow.setCurrentIndex(1)

    def _accept(self) -> None:
        try:
            open_spec = parse_hotkey(self.open_hotkey.text())
            toggle_spec = parse_hotkey(self.toggle_hotkey.text())
        except HotkeyError as exc:
            self.error.setText(str(exc))
            return
        if open_spec.text == toggle_spec.text:
            self.error.setText("两个热键不能相同")
            return
        self.error.setText("")
        self.result_settings = replace(
            self.settings,
            channel_mode=self.follow.currentData(),
            open_hotkey=open_spec.text,
            toggle_hotkey=toggle_spec.text,
            hud_corner=self.corner.currentData(),
            monitor=self.monitor.currentData() or "",
            hud_opacity=self.opacity.value(),
            font_size=self.font.value(),
            sound_enabled=self.sound.isChecked(),
            sound_volume=self.volume.value(),
        )
        self.accept()
