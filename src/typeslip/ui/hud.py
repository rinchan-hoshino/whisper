"""游戏上的 HUD：透明置顶，平时点击穿透、不抢焦点；输入时取消穿透并拿焦点。"""

from __future__ import annotations

import time

from PySide6.QtCore import QPoint, QRect, Qt, Signal
from PySide6.QtGui import QColor, QFont, QGuiApplication, QPainter
from PySide6.QtWidgets import QLabel, QLineEdit, QVBoxLayout, QWidget

from ..adapters import win32
from ..core.timeline import ChatMessage, Timeline

MARGIN = 24
ROW_HEIGHT_FACTOR = 1.6


class Hud(QWidget):
    submitted = Signal(str)
    cancelled = Signal()

    def __init__(self, timeline: Timeline) -> None:
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                         | Qt.WindowType.Tool | Qt.WindowType.WindowDoesNotAcceptFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.timeline = timeline
        self.font_size = 12
        self.opacity_percent = 100
        self.corner = "bottom-left"
        self.connected = False
        self.status_text = ""
        self.input_mode = False
        self.draft = ""
        self.error_text = ""
        self.history_rows = 20
        self._messages_cache: list[tuple[ChatMessage, float]] = []

        self.input = QLineEdit(self)
        self.input.hide()
        self.input.setPlaceholderText("输入后按 Enter 发送")
        self.input.returnPressed.connect(self._submit)
        self.input.installEventFilter(self)
        self.error = QLabel(self)
        self.error.hide()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 8, 12, 8)
        layout.addStretch(1)
        layout.addWidget(self.error)
        layout.addWidget(self.input)
        win32.set_passive(win32.hwnd_of(self), click_through=True)

    # ---- 外观与位置 ----
    def apply_style(self, font_size: int, opacity_percent: int, corner: str) -> None:
        self.font_size = font_size
        self.opacity_percent = opacity_percent
        self.corner = corner
        font = QFont()
        font.setPointSize(font_size)
        self.setFont(font)
        self.input.setFont(font)
        self.error.setFont(font)
        self.error.setStyleSheet("color: #ff8a80;")
        self.input.setStyleSheet("QLineEdit { background: rgba(20,20,24,220); color: #f5f5f5; "
                                 "border: 1px solid rgba(255,255,255,60); padding: 4px; }")
        self.update()

    def place(self, screen_name: str = "") -> None:
        screens = QGuiApplication.screens()
        screen = next((s for s in screens if s.name() == screen_name), QGuiApplication.primaryScreen())
        area = screen.availableGeometry()
        width = min(640, area.width() // 2)
        height = min(int(self.font_size * 2.2 * 22), area.height() - MARGIN * 2)
        x = area.left() + MARGIN if "left" in self.corner else area.right() - width - MARGIN
        y = area.bottom() - height - MARGIN if "bottom" in self.corner else area.top() + MARGIN
        self.setGeometry(x, y, width, height)

    def set_connected(self, connected: bool, status: str) -> None:
        self.connected = connected
        self.status_text = status
        self.update()

    def show_error(self, text: str) -> None:
        self.error.setText(text)
        self.error.setVisible(bool(text))

    # ---- 输入模式 ----
    def enter_input(self) -> None:
        self.input_mode = True
        win32.set_passive(win32.hwnd_of(self), click_through=False)
        self.input.show()
        self.show()
        self.activateWindow()
        self.raise_()
        self.input.setFocus(Qt.FocusReason.OtherFocusReason)
        self.update()

    def leave_input(self, keep_draft: bool = True) -> None:
        self.draft = self.input.text() if keep_draft else ""
        self.input.clear()
        self.input.hide()
        self.input_mode = False
        win32.set_passive(win32.hwnd_of(self), click_through=True)
        self.update()

    def set_draft(self, text: str) -> None:
        self.input.setText(text)

    def eventFilter(self, obj, event):  # noqa: N802
        if obj is self.input and event.type() == event.Type.KeyPress and event.key() == Qt.Key.Key_Escape:
            self.cancelled.emit()
            return True
        return super().eventFilter(obj, event)

    def keyPressEvent(self, event) -> None:  # noqa: N802
        if event.key() == Qt.Key.Key_Escape and self.input_mode:
            self.cancelled.emit()
            return
        super().keyPressEvent(event)

    def wheelEvent(self, event) -> None:  # noqa: N802
        if self.input_mode:
            step = -event.angleDelta().y() // 120
            self.history_rows = max(6, min(200, self.history_rows + step * 2))
            self.update()
            event.accept()
        else:
            super().wheelEvent(event)

    def _submit(self) -> None:
        text = self.input.text().strip()
        if text:
            self.submitted.emit(text)

    # ---- 绘制 ----
    def paintEvent(self, _event) -> None:  # noqa: N802
        now = time.time()
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing)
        rows = self.history_rows if self.input_mode else self.timeline.visible_limit
        items = self.timeline.recent(now, limit=rows)
        alpha_scale = self.opacity_percent / 100.0
        line = int(self.font_size * ROW_HEIGHT_FACTOR)
        bottom = self.height() - 12 - (self.input.height() + 8 if self.input_mode else 0)
        y = bottom
        for message, alpha in reversed(items):
            y -= line
            if y < 0:
                break
            color = QColor(245, 245, 245, int(255 * alpha * alpha_scale))
            if message.own:
                color = QColor(180, 230, 255, int(255 * alpha * alpha_scale))
            # 深色半透明底：游戏和聊天背景可能是白的，文字要看得清。
            painter.fillRect(QRect(8, y, self.width() - 16, line), QColor(16, 16, 20, int(150 * alpha * alpha_scale)))
            painter.setPen(color)
            painter.drawText(QRect(12, y, self.width() - 24, line), Qt.AlignmentFlag.AlignVCenter,
                             message.line)
        dot = QColor(90, 220, 120) if self.connected else QColor(230, 90, 90)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(dot)
        painter.drawEllipse(QPoint(self.width() - 12, 12), 4, 4)
        if self.status_text and not self.connected:
            painter.fillRect(QRect(8, 4, self.width() - 16, line), QColor(16, 16, 20, 150))
            painter.setPen(QColor(255, 220, 160))
            painter.drawText(QRect(12, 4, self.width() - 40, line), Qt.AlignmentFlag.AlignVCenter, self.status_text)
        painter.end()
