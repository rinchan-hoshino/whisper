"""热键字符串解析，例如 'F8'、'Ctrl+Shift+F9'。纯逻辑，返回 RegisterHotKey 需要的修饰位和虚拟键码。"""

from __future__ import annotations

from dataclasses import dataclass

MOD_ALT = 0x0001
MOD_CONTROL = 0x0002
MOD_SHIFT = 0x0004
MOD_WIN = 0x0008
MOD_NOREPEAT = 0x4000

_MODIFIERS = {"ctrl": MOD_CONTROL, "control": MOD_CONTROL, "shift": MOD_SHIFT, "alt": MOD_ALT}


class HotkeyError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class HotkeySpec:
    modifiers: int
    vk: int
    text: str


def _vk_for(key: str) -> int:
    upper = key.upper()
    if len(upper) == 1 and ("A" <= upper <= "Z" or "0" <= upper <= "9"):
        return ord(upper)
    if upper.startswith("F") and upper[1:].isdigit():
        number = int(upper[1:])
        if 1 <= number <= 24:
            return 0x70 + number - 1
    named = {"SPACE": 0x20, "TAB": 0x09, "INSERT": 0x2D, "DELETE": 0x2E, "HOME": 0x24, "END": 0x23,
             "PAGEUP": 0x21, "PAGEDOWN": 0x22}
    if upper in named:
        return named[upper]
    raise HotkeyError(f"不认识的按键：{key}")


def parse_hotkey(text: str) -> HotkeySpec:
    parts = [part.strip() for part in str(text or "").split("+") if part.strip()]
    if not parts:
        raise HotkeyError("热键不能为空")
    modifiers = 0
    for part in parts[:-1]:
        flag = _MODIFIERS.get(part.lower())
        if flag is None:
            raise HotkeyError(f"不认识的修饰键：{part}")
        modifiers |= flag
    vk = _vk_for(parts[-1])
    return HotkeySpec(modifiers=modifiers | MOD_NOREPEAT, vk=vk, text="+".join(parts))
