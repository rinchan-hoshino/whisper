"""本机 Oopz 登录态：注册表 HKCU\\Software\\Oopz\\OopzData 的 login 值（base64 JSON）。"""

from __future__ import annotations

import base64
import ctypes
import json
import os
import time
from ctypes import wintypes
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


class _ProcessEntry(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_size_t),
        ("th32ModuleID", wintypes.DWORD),
        ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase", wintypes.LONG),
        ("dwFlags", wintypes.DWORD),
        ("szExeFile", wintypes.WCHAR * 260),
    ]


def oopz_running() -> bool:
    """Windows 进程快照里有没有 Oopz.exe。注册表里的登录值可能是上次留下的，要看进程。"""
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    for name in ("Process32FirstW", "Process32NextW"):
        getattr(kernel, name).argtypes = [wintypes.HANDLE, ctypes.POINTER(_ProcessEntry)]
        getattr(kernel, name).restype = wintypes.BOOL
    snapshot = kernel.CreateToolhelp32Snapshot(0x2, 0)  # TH32CS_SNAPPROCESS
    if snapshot == ctypes.c_void_p(-1).value:
        raise SessionError("无法检查 Oopz 是否运行，请稍后重试。")
    try:
        entry = _ProcessEntry()
        entry.dwSize = ctypes.sizeof(entry)
        found = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
        while found:
            if entry.szExeFile.casefold() == "oopz.exe":
                return True
            found = kernel.Process32NextW(snapshot, ctypes.byref(entry))
        return False
    finally:
        kernel.CloseHandle(snapshot)


def read_login_value() -> str:
    """只在 Windows 上读注册表；Oopz 没开或没登录抛 SessionError。"""
    if os.name != "nt":
        raise SessionError("只能在 Windows 上读取 Oopz 登录态。")
    if not oopz_running():
        raise SessionError("先打开 Oopz 并登录。")
    import winreg

    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Oopz\OopzData") as key:
            value, _ = winreg.QueryValueEx(key, "login")
    except OSError as exc:
        raise SessionError("先打开 Oopz 并登录。") from exc
    return str(value)
