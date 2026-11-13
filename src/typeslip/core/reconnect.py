"""断线重连退避：2 秒起，每次翻倍，上限 120 秒。"""

from __future__ import annotations

BASE_SECONDS = 2.0
MAX_SECONDS = 120.0


def backoff_seconds(failures: int) -> float:
    if failures <= 0:
        return BASE_SECONDS
    return min(MAX_SECONDS, BASE_SECONDS * (2 ** min(failures, 16)))
