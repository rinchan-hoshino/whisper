"""Oopz 连接：oopz-sdk 跑在独立 asyncio 线程里，事件放进队列给界面线程取。"""

from __future__ import annotations

import asyncio
import logging
import queue
import threading
import time
from dataclasses import dataclass
from typing import Any

from oopz_sdk import OopzConfig
from oopz_sdk.client.rest import OopzRESTClient
from oopz_sdk.client.ws import OopzWSClient
from oopz_sdk.events.parser import EventParser
from oopz_sdk.models import JoinedAreaInfo, MessageEvent

from ..core.reconnect import backoff_seconds
from ..core.timeline import ChatMessage
from .session import LoginSession

LOGGER = logging.getLogger(__name__)
FOLLOW_INTERVAL = 20.0


@dataclass(frozen=True, slots=True)
class Target:
    area_id: str
    area_name: str
    channel_id: str
    channel_name: str


def _clean(value: str) -> str:
    return value.translate(str.maketrans("", "", "‎‏⁦⁧⁨⁩"))


def message_text(source: Any) -> str:
    plain = getattr(source, "plain_text", "")
    if isinstance(plain, str) and plain:
        return _clean(plain)
    attachments = list(getattr(source, "attachments", None) or [])
    kinds = {str(getattr(a, "attachment_type", "") or "").upper() for a in attachments}
    if "IMAGE" in kinds:
        return "[图片]"
    if kinds:
        return "[文件]"
    for field in ("text", "content"):
        value = getattr(source, field, "")
        if isinstance(value, str) and value:
            return "[图片]" if value.startswith("![IMAGE") else _clean(value)
    return "[非文字消息]"


def to_chat(source: Any, own_uid: str, names: dict[str, str]) -> ChatMessage:
    sender = str(getattr(source, "sender_id", "") or "")
    return ChatMessage(
        message_id=str(getattr(source, "message_id", "") or ""),
        nickname=names.get(sender) or ("我" if sender == own_uid else sender[-6:] or "未知"),
        text=message_text(source),
        own=sender == own_uid,
    )


class OopzLink:
    """emit(kind, *values) 在工作线程里调用，界面线程用 drain() 取出。"""

    def __init__(self) -> None:
        self.events: queue.Queue[tuple[Any, ...]] = queue.Queue()
        self._loop: asyncio.AbstractEventLoop | None = None
        self._thread: threading.Thread | None = None
        self._session: LoginSession | None = None
        self._target: Target | None = None
        self._rest: OopzRESTClient | None = None
        self._ws: OopzWSClient | None = None
        self._names: dict[str, str] = {}
        self._parser = EventParser()
        self._stopping = False

    # ---- 界面线程调用 ----
    def start(self, session: LoginSession, target_mode: str, pinned: Target | None) -> None:
        self._session = session
        self._pinned = pinned if target_mode == "pinned" else None
        self._thread = threading.Thread(target=self._run, name="typeslip-oopz", daemon=True)
        self._thread.start()

    def drain(self) -> list[tuple[Any, ...]]:
        items = []
        while True:
            try:
                items.append(self.events.get_nowait())
            except queue.Empty:
                return items

    def send(self, text: str) -> None:
        if self._loop is None or self._target is None:
            self.events.put(("send_failed", text, "尚未连接 Oopz"))
            return
        asyncio.run_coroutine_threadsafe(self._send(text), self._loop)

    def stop(self) -> None:
        self._stopping = True
        if self._loop is not None:
            self._loop.call_soon_threadsafe(self._loop.stop)

    # ---- 工作线程 ----
    def _emit(self, *values: Any) -> None:
        self.events.put(values)

    def _run(self) -> None:
        self._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self._loop)
        failures = 0
        while not self._stopping:
            try:
                self._loop.run_until_complete(self._session_once())
                failures = 0
            except Exception as exc:  # noqa: BLE001 - 任何错误都走退避重连
                LOGGER.warning("Oopz session ended", exc_info=exc)
                self._emit("status", "disconnected", str(exc))
                failures += 1
            if self._stopping:
                break
            self._loop.run_until_complete(asyncio.sleep(backoff_seconds(failures)))

    async def _session_once(self) -> None:
        assert self._session is not None
        session = self._session
        config = OopzConfig(
            device_id=session.device_id,
            person_uid=session.person_uid,
            jwt_token=session.jwt_token,
            app_version=session.app_version,
            auto_subscribe_joined_areas=False,
        )
        rest = OopzRESTClient(config)
        await rest.start()
        self._rest = rest
        try:
            joined = await self._joined_areas(rest)
            ws_task = asyncio.create_task(self._open_ws(config))
            self._emit("status", "connecting", "")
            follow = asyncio.create_task(self._follow_loop(rest, config, joined))
            try:
                await ws_task
            finally:
                follow.cancel()
        finally:
            self._rest = None
            self._ws = None
            self._target = None
            self._emit("status", "disconnected", "")
            await rest.close()

    async def _joined_areas(self, rest: OopzRESTClient) -> list[JoinedAreaInfo]:
        payload = await rest.areas._request_data("GET", "/userSubscribeArea/v1/list")
        if not isinstance(payload, list):
            return []
        return [JoinedAreaInfo.from_api(item) for item in payload if isinstance(item, dict)]

    async def _open_ws(self, config: OopzConfig) -> None:
        async def on_open() -> None:
            self._emit("status", "connected", "")
            if self._target is not None:
                await self._ws.send_subscribe_area_events([self._target.area_id])  # type: ignore[union-attr]

        async def on_raw(raw: str) -> None:
            await self._on_raw(raw)

        async def on_error(error: object) -> None:
            self._emit("status", "disconnected", str(error or ""))

        ws = OopzWSClient(
            config,
            on_open=on_open,
            on_message=on_raw,
            on_error=on_error,
            on_close=lambda _info: on_error(None),
            on_reconnect=lambda: on_error(None),
        )
        self._ws = ws
        await ws.start()

    async def _follow_loop(self, rest: OopzRESTClient, config: OopzConfig,
                           joined: list[JoinedAreaInfo]) -> None:
        while True:
            target = await self._resolve_target(rest, config, joined)
            if target is not None and (self._target is None or target != self._target):
                await self._switch(target)
            elif target is None and self._target is not None and self._pinned is None:
                self._target = None
                self._emit("target", None)
                self._emit("status", "no_voice", "")
            await asyncio.sleep(FOLLOW_INTERVAL)

    async def _resolve_target(self, rest: OopzRESTClient, config: OopzConfig,
                              joined: list[JoinedAreaInfo]) -> Target | None:
        if self._pinned is not None:
            return self._pinned
        voice_area: JoinedAreaInfo | None = None
        for area in joined:
            try:
                if await rest.channels.get_voice_channel_for_user(area.area_id, config.person_uid):
                    voice_area = area
                    break
            except Exception as exc:  # noqa: BLE001
                LOGGER.debug("voice lookup failed", exc_info=exc)
        if voice_area is None:
            return None
        groups = await rest.areas.get_area_channels(voice_area.area_id)
        for group in groups:
            for channel in group.channels:
                if channel.channel_type.upper() == "TEXT":
                    return Target(voice_area.area_id, voice_area.name, channel.channel_id, channel.name)
        return None

    async def _switch(self, target: Target) -> None:
        self._target = target
        self._emit("target", target)
        assert self._rest is not None
        try:
            messages = await self._rest.messages.get_channel_messages(target.area_id, target.channel_id, size=20)
            await self._resolve_names(messages)
            chat = [to_chat(m, self._session.person_uid, self._names) for m in messages]  # type: ignore[union-attr]
            self._emit("history", chat)
        except Exception as exc:  # noqa: BLE001
            LOGGER.debug("history failed", exc_info=exc)
        if self._ws is not None:
            await self._ws.send_subscribe_area_events([target.area_id])

    async def _resolve_names(self, messages: list[Any]) -> None:
        if self._rest is None:
            return
        missing = {str(getattr(m, "sender_id", "") or "") for m in messages} - self._names.keys() - {""}
        if not missing:
            return
        try:
            users = await self._rest.person.get_person_infos_batch(sorted(missing))
            self._names.update({u.uid: u.name for u in users if u.name})
        except Exception as exc:  # noqa: BLE001
            LOGGER.debug("name lookup failed", exc_info=exc)

    async def _on_raw(self, raw: str) -> None:
        target = self._target
        if target is None:
            return
        try:
            event = self._parser.parse(raw)
        except Exception:  # noqa: BLE001 - 畸形事件直接丢
            return
        if not isinstance(event, MessageEvent) or event.is_private:
            return
        message = event.message
        if message.area != target.area_id or message.channel != target.channel_id:
            return
        await self._resolve_names([message])
        chat = to_chat(message, self._session.person_uid, self._names)  # type: ignore[union-attr]
        self._emit("message", chat, time.time())

    async def _send(self, text: str) -> None:
        target = self._target
        if target is None or self._rest is None:
            self._emit("send_failed", text, "尚未连接 Oopz")
            return
        try:
            await self._rest.messages.send_message(text, area=target.area_id, channel=target.channel_id)
            self._emit("sent", text)
        except Exception as exc:  # noqa: BLE001
            self._emit("send_failed", text, str(exc) or "发送失败")
