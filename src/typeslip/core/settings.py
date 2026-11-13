"""设置：纯数据与校验。持久化在 adapters 里做（设置不含凭据，明文 JSON）。"""

from __future__ import annotations

import json
import os
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

CORNERS = ("bottom-left", "bottom-right", "top-left", "top-right")


def _clamp_int(value: Any, low: int, high: int, fallback: int) -> int:
    try:
        number = int(value)
    except (TypeError, ValueError):
        return fallback
    return max(low, min(high, number))


def _text(value: Any, fallback: str = "") -> str:
    return value.strip() if isinstance(value, str) else fallback


@dataclass(frozen=True, slots=True)
class Settings:
    channel_mode: str = "follow"          # follow: 跟随语音服务器；pinned: 固定下面的频道
    area_id: str = ""
    area_name: str = ""
    channel_id: str = ""
    channel_name: str = ""
    open_hotkey: str = "F8"
    toggle_hotkey: str = "F9"
    hud_corner: str = "bottom-left"
    hud_opacity: int = 100                # 20-100
    font_size: int = 12                   # 9-20
    monitor: str = ""                     # 显示器名，空为主屏
    sound_enabled: bool = True
    sound_volume: int = 60                # 0-100

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> "Settings":
        base = cls()
        mode = raw.get("channel_mode", base.channel_mode)
        corner = raw.get("hud_corner", base.hud_corner)
        sound = raw.get("sound_enabled", base.sound_enabled)
        return cls(
            channel_mode=mode if mode in ("follow", "pinned") else base.channel_mode,
            area_id=_text(raw.get("area_id")),
            area_name=_text(raw.get("area_name")),
            channel_id=_text(raw.get("channel_id")),
            channel_name=_text(raw.get("channel_name")),
            open_hotkey=_text(raw.get("open_hotkey"), base.open_hotkey) or base.open_hotkey,
            toggle_hotkey=_text(raw.get("toggle_hotkey"), base.toggle_hotkey) or base.toggle_hotkey,
            hud_corner=corner if corner in CORNERS else base.hud_corner,
            hud_opacity=_clamp_int(raw.get("hud_opacity"), 20, 100, base.hud_opacity),
            font_size=_clamp_int(raw.get("font_size"), 9, 20, base.font_size),
            monitor=_text(raw.get("monitor")),
            sound_enabled=sound if isinstance(sound, bool) else base.sound_enabled,
            sound_volume=_clamp_int(raw.get("sound_volume"), 0, 100, base.sound_volume),
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def migrate_legacy(raw: dict[str, Any]) -> Settings:
    """把 1.x 解密后的设置映射到 2.0。频道固定（1.x 只能固定频道），坐标与旧版显示相关项不迁移。"""
    channel_id = _text(raw.get("channel_id"))
    sound_name = _text(raw.get("notification_sound"))
    return Settings.from_dict({
        "channel_mode": "pinned" if channel_id else "follow",
        "area_id": raw.get("area_id"),
        "area_name": raw.get("area_name"),
        "channel_id": channel_id,
        "channel_name": raw.get("channel_name"),
        "open_hotkey": raw.get("hotkey"),
        "toggle_hotkey": raw.get("visibility_hotkey"),
        "font_size": raw.get("font_size"),
        "hud_opacity": raw.get("text_opacity"),
        "monitor": raw.get("screen_name"),
        "sound_enabled": bool(sound_name),
    })


def load(path: Path) -> Settings:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return Settings()
    if not isinstance(raw, dict):
        raise ValueError("设置文件格式错误")
    return Settings.from_dict(raw)


def save(path: Path, settings: Settings) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(settings.to_dict(), ensure_ascii=False, indent=2), encoding="utf-8")
    os.replace(temporary, path)
