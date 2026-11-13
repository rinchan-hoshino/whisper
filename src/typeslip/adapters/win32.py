"""Windows 原生能力：热键、窗口样式、焦点。只在 Windows 上生效，其他系统是空操作。"""

from __future__ import annotations

import ctypes
import os
from ctypes import wintypes

from ..core.hotkeys import HotkeySpec

IS_WINDOWS = os.name == "nt"
GWL_EXSTYLE = -20
WS_EX_TRANSPARENT = 0x00000020
WS_EX_NOACTIVATE = 0x08000000
WS_EX_TOOLWINDOW = 0x00000080
SWP_NOMOVE = 0x0002
SWP_NOSIZE = 0x0001
SWP_NOACTIVATE = 0x0010
HWND_TOPMOST = -1
WM_HOTKEY = 0x0312
SW_RESTORE = 9

_user32 = ctypes.windll.user32 if IS_WINDOWS else None
_kernel32 = ctypes.windll.kernel32 if IS_WINDOWS else None


def hwnd_of(widget) -> int:
    return int(widget.winId()) if IS_WINDOWS else 0


def register_hotkey(hotkey_id: int, spec: HotkeySpec) -> bool:
    if not IS_WINDOWS:
        return False
    return bool(_user32.RegisterHotKey(None, hotkey_id, spec.modifiers, spec.vk))


def unregister_hotkey(hotkey_id: int) -> None:
    if IS_WINDOWS:
        _user32.UnregisterHotKey(None, hotkey_id)


def set_passive(hwnd: int, click_through: bool) -> None:
    """不抢焦点始终有；点击穿透只在平时开，输入时关。"""
    if not IS_WINDOWS or not hwnd:
        return
    style = _user32.GetWindowLongW(hwnd, GWL_EXSTYLE)
    style |= WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
    style = style | WS_EX_TRANSPARENT if click_through else style & ~WS_EX_TRANSPARENT
    _user32.SetWindowLongW(hwnd, GWL_EXSTYLE, style)


def set_topmost(hwnd: int) -> None:
    if IS_WINDOWS and hwnd:
        _user32.SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)


def foreground_window() -> int:
    return int(_user32.GetForegroundWindow()) if IS_WINDOWS else 0


def focus_window(hwnd: int) -> bool:
    """把焦点还给游戏窗口。Windows 可能拒绝切换前台，返回 False。"""
    if not IS_WINDOWS or not hwnd:
        return False
    _user32.ShowWindow(hwnd, SW_RESTORE)
    current = _kernel32.GetCurrentThreadId()
    target = _user32.GetWindowThreadProcessId(hwnd, None)
    attached = bool(target) and bool(_user32.AttachThreadInput(current, target, True))
    try:
        return bool(_user32.SetForegroundWindow(hwnd))
    finally:
        if attached:
            _user32.AttachThreadInput(current, target, False)


def play_wav(data: bytes) -> None:
    """异步播放内存中的 WAV；非 Windows 不做事。"""
    if not IS_WINDOWS:
        return
    import winsound

    winsound.PlaySound(data, winsound.SND_MEMORY | winsound.SND_ASYNC)


def is_hotkey_message(message) -> int | None:
    """从 Qt 原生事件里取出 WM_HOTKEY 的 id。"""
    if not IS_WINDOWS:
        return None
    msg = wintypes.MSG.from_address(int(message))
    return int(msg.wParam) if msg.message == WM_HOTKEY else None
