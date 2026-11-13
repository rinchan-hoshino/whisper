"""本机 Oopz 登录态：注册表 HKCU\\Software\\Oopz\\OopzData 的 login 值（base64 JSON）。"""

from __future__ import annotations

import base64
import json
import os
import time
from dataclasses import dataclass


class SessionError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class LoginSession:
    person_uid: str
    jwt_token: str
    device_id: str
    app_version: str


def _jwt_exp(token: str) -> int | None:
    parts = token.split(".")
    if len(parts) != 3:
        return None
    try:
        payload = parts[1] + "=" * (-len(parts[1]) % 4)
        value = json.loads(base64.urlsafe_b64decode(payload.encode()))
        return int(value["exp"]) if "exp" in value else None
    except (ValueError, TypeError, KeyError, json.JSONDecodeError):
        return None


def parse_login_value(value: str, *, now: float | None = None) -> LoginSession:
    try:
        payload = json.loads(base64.b64decode(value, validate=True))
    except (ValueError, TypeError, json.JSONDecodeError) as exc:
        raise SessionError("本机 Oopz 登录态无法识别，请先更新并重新登录 Oopz。") from exc
    if not isinstance(payload, dict):
        raise SessionError("本机 Oopz 登录态格式不完整。")
    person_uid = str(payload.get("uid") or "").strip()
    token = str(payload.get("signature") or "").strip()
    device_id = str(payload.get("deviceId") or "").strip()
    if not (person_uid and token and device_id):
        raise SessionError("没有找到完整的登录态，请先打开并登录 Oopz。")
    expires = _jwt_exp(token)
    if expires is not None and expires <= int(now if now is not None else time.time()) + 60:
        raise SessionError("Oopz 登录已过期，请打开 Oopz 重新登录。")
    return LoginSession(
        person_uid=person_uid,
        jwt_token=token,
        device_id=device_id,
        app_version=str(payload.get("appVersion") or "69514"),
    )


def read_login_value() -> str:
    """只在 Windows 上读注册表；Oopz 没开或没登录抛 SessionError。"""
    if os.name != "nt":
        raise SessionError("只能在 Windows 上读取 Oopz 登录态。")
    import winreg

    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Oopz\OopzData") as key:
            value, _ = winreg.QueryValueEx(key, "login")
    except OSError as exc:
        raise SessionError("先打开 Oopz 并登录。") from exc
    return str(value)
