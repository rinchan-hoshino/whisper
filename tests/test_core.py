from __future__ import annotations

import base64
import hashlib
import io
import json
import wave
from pathlib import Path

import pytest

from typeslip.adapters.session import SessionError, parse_login_value
from typeslip.core.hotkeys import HotkeyError, MOD_CONTROL, MOD_SHIFT, parse_hotkey
from typeslip.core.reconnect import backoff_seconds
from typeslip.core.settings import Settings, migrate_legacy
from typeslip.core.sound import DURATION, SAMPLE_RATE, scale_wav, synthesize_ding_dong
from typeslip.core.timeline import ChatMessage, Timeline
from typeslip.core.updates import ManifestError, is_newer, parse_manifest, verify_file

SHA = "a" * 64


def manifest_payload(**overrides) -> bytes:
    data = {"version": "2.0.1", "url": "https://acv.k-neco.com/tools/oopz-tactical-overlay/2.0.1/OopzTacticalOverlay.exe",
            "size": 10, "sha256": SHA, "publishedAt": "2026-11-13T00:00:00+08:00"}
    data.update(overrides)
    return json.dumps(data).encode()


# ---- 时间线与淡出 ----
def test_timeline_fades_after_ten_seconds_and_keeps_last_six():
    timeline = Timeline()
    for index in range(8):
        timeline.add(ChatMessage(str(index), "a", str(index)), now=100.0)
    shown = timeline.recent(now=100.0)
    assert [m.message_id for m, _ in shown] == ["2", "3", "4", "5", "6", "7"]
    assert timeline.recent(now=110.0) == []


def test_timeline_alpha_holds_then_fades_over_last_two_seconds():
    timeline = Timeline(fade_after=10.0)
    assert timeline.alpha(0.0, 7.9) == 1.0
    assert timeline.alpha(0.0, 9.0) == pytest.approx(0.5)
    assert timeline.alpha(0.0, 10.0) == 0.0


def test_timeline_dedupes_by_message_id_and_trims_capacity():
    timeline = Timeline(capacity=2)
    assert timeline.add(ChatMessage("x", "a", "1"), 0.0)
    assert not timeline.add(ChatMessage("x", "a", "1"), 0.0)
    timeline.add(ChatMessage("y", "a", "2"), 0.0)
    timeline.add(ChatMessage("z", "a", "3"), 0.0)
    assert len(timeline) == 2


# ---- 重连退避 ----
def test_backoff_starts_at_two_seconds_and_caps_at_120():
    assert backoff_seconds(0) == 2
    assert backoff_seconds(1) == 4
    assert backoff_seconds(3) == 16
    assert backoff_seconds(50) == 120


# ---- 热键 ----
def test_hotkey_parses_function_keys_and_modifiers():
    spec = parse_hotkey("Ctrl+Shift+F9")
    assert spec.vk == 0x78
    assert spec.modifiers & MOD_CONTROL and spec.modifiers & MOD_SHIFT
    assert parse_hotkey("F8").vk == 0x77


@pytest.mark.parametrize("text", ["", "Hyper+F8", "F25", "Ctrl+"])
def test_hotkey_rejects_bad_input(text):
    with pytest.raises(HotkeyError):
        parse_hotkey(text)


# ---- 提示音 ----
def test_ding_dong_is_015_seconds_mono_16bit():
    with wave.open(io.BytesIO(synthesize_ding_dong()), "rb") as stream:
        assert stream.getnchannels() == 1
        assert stream.getsampwidth() == 2
        assert stream.getframerate() == SAMPLE_RATE
        assert stream.getnframes() == int(SAMPLE_RATE * DURATION)


def test_scale_wav_zero_volume_is_silent():
    silent = scale_wav(synthesize_ding_dong(), 0.0)
    with wave.open(io.BytesIO(silent), "rb") as stream:
        frames = stream.readframes(stream.getnframes())
    assert set(frames) == {0}


# ---- 更新清单 ----
def test_manifest_accepts_only_acv_urls_and_valid_hash():
    manifest = parse_manifest(manifest_payload())
    assert manifest.version == "2.0.1"
    with pytest.raises(ManifestError):
        parse_manifest(manifest_payload(url="https://example.com/a.exe"))
    with pytest.raises(ManifestError):
        parse_manifest(manifest_payload(sha256="xyz"))
    with pytest.raises(ManifestError):
        parse_manifest(manifest_payload(size=0))


def test_version_comparison_is_numeric():
    assert is_newer("2.0.10", "2.0.9")
    assert not is_newer("2.0.0", "2.0.0")


def test_verify_file_checks_size_and_sha256(tmp_path: Path):
    body = b"0123456789"
    path = tmp_path / "x.exe"
    path.write_bytes(body)
    manifest = parse_manifest(manifest_payload(size=10, sha256=hashlib.sha256(body).hexdigest()))
    assert verify_file(path, manifest)
    path.write_bytes(b"0123456780")
    assert not verify_file(path, manifest)
    path.write_bytes(b"short")
    assert not verify_file(path, manifest)


# ---- 设置与迁移 ----
def test_settings_clamp_and_fall_back():
    settings = Settings.from_dict({"font_size": 99, "hud_opacity": 1, "hud_corner": "middle", "sound_enabled": "yes"})
    assert settings.font_size == 20
    assert settings.hud_opacity == 20
    assert settings.hud_corner == "bottom-left"
    assert settings.sound_enabled is True


def test_legacy_migration_pins_channel_and_keeps_hotkeys_and_font():
    legacy = {
        "area_id": "A1", "area_name": "服", "channel_id": "C9", "channel_name": "开黑",
        "hotkey": "Ctrl+F8", "visibility_hotkey": "F10", "font_size": 15,
        "text_opacity": 80, "screen_name": "DISPLAY2", "notification_sound": "",
        "position_x": 0.3, "always_visible": False,
    }
    settings = migrate_legacy(legacy)
    assert settings.channel_mode == "pinned"
    assert (settings.area_id, settings.channel_id) == ("A1", "C9")
    assert settings.open_hotkey == "Ctrl+F8"
    assert settings.toggle_hotkey == "F10"
    assert settings.font_size == 15
    assert settings.hud_opacity == 80
    assert settings.monitor == "DISPLAY2"
    assert settings.sound_enabled is False


def test_legacy_without_channel_follows_voice():
    assert migrate_legacy({}).channel_mode == "follow"


# ---- 登录值 ----
def _login(payload: dict) -> str:
    return base64.b64encode(json.dumps(payload).encode()).decode()


def test_login_value_parses_uid_signature_device():
    session = parse_login_value(_login({"uid": "U1", "signature": "a.b.c", "deviceId": "D1", "appVersion": "70000"}))
    assert (session.person_uid, session.device_id, session.app_version) == ("U1", "D1", "70000")


def test_login_value_rejects_incomplete_or_garbage():
    with pytest.raises(SessionError):
        parse_login_value(_login({"uid": "U1"}))
    with pytest.raises(SessionError):
        parse_login_value("not base64!!")
