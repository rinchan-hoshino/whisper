"""消息时间线：最近消息、淡出计时，纯逻辑。"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class ChatMessage:
    message_id: str
    nickname: str
    text: str
    own: bool = False


class Timeline:
    """保留最近 `capacity` 条；HUD 只显示 `visible_limit` 条，每条 `fade_after` 秒后淡出。"""

    def __init__(self, capacity: int = 200, visible_limit: int = 6, fade_after: float = 10.0):
        self.capacity = capacity
        self.visible_limit = visible_limit
        self.fade_after = fade_after
        self._items: list[tuple[float, ChatMessage]] = []
        self._seen: set[str] = set()

    def add(self, message: ChatMessage, now: float) -> bool:
        if message.message_id and message.message_id in self._seen:
            return False
        self._seen.add(message.message_id)
        self._items.append((now, message))
        if len(self._items) > self.capacity:
            dropped = self._items[: len(self._items) - self.capacity]
            del self._items[: len(self._items) - self.capacity]
            for _, item in dropped:
                self._seen.discard(item.message_id)
        return True

    def replace_all(self, messages: list[ChatMessage], now: float) -> None:
        self._items = [(now, m) for m in messages[-self.capacity:]]
        self._seen = {m.message_id for _, m in self._items}

    def recent(self, now: float, limit: int | None = None) -> list[tuple[ChatMessage, float]]:
        """最近的消息与透明度（1.0 到 0.0，按时间从旧到新），已淡尽的不返回。"""
        size = self.visible_limit if limit is None else limit
        result: list[tuple[ChatMessage, float]] = []
        for stamp, message in self._items[-size:]:
            alpha = self.alpha(stamp, now)
            if alpha > 0.0:
                result.append((message, alpha))
        return result

    def alpha(self, stamp: float, now: float) -> float:
        age = now - stamp
        if age <= 0:
            return 1.0
        if age >= self.fade_after:
            return 0.0
        fade_start = self.fade_after - 2.0
        if age <= fade_start:
            return 1.0
        return max(0.0, (self.fade_after - age) / 2.0)

    def __len__(self) -> int:
        return len(self._items)
