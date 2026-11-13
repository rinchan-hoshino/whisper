"""自动更新清单解析与校验，纯逻辑。"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

ALLOWED_URL_PREFIX = "https://acv.k-neco.com/"
MANIFEST_URL = "https://acv.k-neco.com/tools/oopz-tactical-overlay/latest.json"


class ManifestError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class UpdateManifest:
    version: str
    url: str
    size: int
    sha256: str


def version_key(value: str) -> tuple[int, ...]:
    parts = re.findall(r"\d+", str(value))
    if not parts:
        raise ManifestError(f"版本号无法识别：{value}")
    return tuple(int(part) for part in parts)


def is_newer(candidate: str, current: str) -> bool:
    return version_key(candidate) > version_key(current)


def parse_manifest(payload: bytes) -> UpdateManifest:
    try:
        data = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ManifestError("更新清单不是合法 JSON") from exc
    if not isinstance(data, dict):
        raise ManifestError("更新清单格式错误")
    try:
        version = str(data["version"]).strip()
        url = str(data["url"]).strip()
        size = int(data["size"])
        sha256 = str(data["sha256"]).strip().lower()
    except (KeyError, TypeError, ValueError) as exc:
        raise ManifestError("更新清单缺少字段") from exc
    version_key(version)
    if not url.startswith(ALLOWED_URL_PREFIX):
        raise ManifestError("更新地址不在允许的域名下")
    if size <= 0:
        raise ManifestError("更新文件大小无效")
    if not re.fullmatch(r"[0-9a-f]{64}", sha256):
        raise ManifestError("SHA-256 格式无效")
    return UpdateManifest(version=version, url=url, size=size, sha256=sha256)


def verify_file(path: Path, manifest: UpdateManifest) -> bool:
    if not path.is_file() or path.stat().st_size != manifest.size:
        return False
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest() == manifest.sha256
