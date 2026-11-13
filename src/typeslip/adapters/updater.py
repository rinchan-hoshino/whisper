"""自动更新：下载到暂存目录、校验，下次启动前由 helper 替换。"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
import urllib.request
from pathlib import Path

from ..core.updates import MANIFEST_URL, UpdateManifest, is_newer, parse_manifest, verify_file

TIMEOUT = 20


def _get(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "TypeSlip-Updater"})
    with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
        return response.read()


def fetch_manifest() -> UpdateManifest:
    return parse_manifest(_get(MANIFEST_URL))


def download_verified(manifest: UpdateManifest, staging: Path) -> Path:
    staging.mkdir(parents=True, exist_ok=True)
    target = staging / f"update-{manifest.version}.exe"
    if verify_file(target, manifest):
        return target
    partial = target.with_suffix(".part")
    with urllib.request.urlopen(manifest.url, timeout=TIMEOUT) as response, partial.open("wb") as out:
        shutil.copyfileobj(response, out, 1 << 20)
    if not verify_file(partial, manifest):
        partial.unlink(missing_ok=True)
        raise ValueError("更新文件大小或 SHA-256 不对")
    os.replace(partial, target)
    return target


def staged_update(staging: Path, current_version: str) -> tuple[UpdateManifest, Path] | None:
    """找到已下好、校验过、比当前新的更新。记录文件写的是清单快照。"""
    record = staging / "pending.json"
    if not record.is_file():
        return None
    try:
        data = json.loads(record.read_text(encoding="utf-8"))
        manifest = UpdateManifest(data["version"], data["url"], int(data["size"]), data["sha256"])
    except (ValueError, KeyError, TypeError, OSError):
        return None
    if not is_newer(manifest.version, current_version):
        return None
    path = staging / f"update-{manifest.version}.exe"
    if not verify_file(path, manifest):
        return None
    return manifest, path


def record_pending(staging: Path, manifest: UpdateManifest) -> None:
    staging.mkdir(parents=True, exist_ok=True)
    (staging / "pending.json").write_text(json.dumps({
        "version": manifest.version, "url": manifest.url,
        "size": manifest.size, "sha256": manifest.sha256,
    }), encoding="utf-8")


def launch_helper(exe: Path, staged: Path, target: Path, parent_pid: int, new_version: str) -> None:
    """启动 helper：等父进程退出后替换 target，然后启动新版。"""
    command = [str(exe), "--apply-update", str(staged), str(target), str(parent_pid), new_version]
    creation = 0x00000008 | 0x00000200 if os.name == "nt" else 0  # DETACHED | NEW_PROCESS_GROUP
    subprocess.Popen(command, creationflags=creation, close_fds=True)


def apply_update(staged: Path, target: Path, parent_pid: int, new_version: str) -> None:
    """helper 入口：等父进程退出，重试替换，记下新版本，然后启动。"""
    deadline = time.time() + 60
    while time.time() < deadline and _alive(parent_pid):
        time.sleep(0.5)
    last: Exception | None = None
    while time.time() < deadline:
        try:
            temporary = target.with_suffix(".new")
            shutil.copy2(staged, temporary)
            os.replace(temporary, target)
            (staged.parent / "updated.json").write_text(json.dumps({"version": new_version}), encoding="utf-8")
            subprocess.Popen([str(target)], close_fds=True)
            return
        except OSError as exc:
            last = exc
            time.sleep(0.5)
    raise RuntimeError(f"替换失败：{last}")


def _alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        import ctypes

        kernel32 = ctypes.windll.kernel32
        handle = kernel32.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
        if not handle:
            return False
        try:
            return kernel32.WaitForSingleObject(handle, 0) == 0x102  # WAIT_TIMEOUT: still running
        finally:
            kernel32.CloseHandle(handle)
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True
