"""提示音：程序内合成 0.15 秒双音 sine「叮咚」，写成 16 位单声道 WAV。"""

from __future__ import annotations

import io
import math
import struct
import wave

SAMPLE_RATE = 44100
DURATION = 0.15
FREQUENCIES = (880.0, 1318.5)  # A5 + E6


def synthesize_ding_dong() -> bytes:
    frames = int(SAMPLE_RATE * DURATION)
    samples = bytearray()
    for index in range(frames):
        t = index / SAMPLE_RATE
        envelope = min(1.0, t / 0.005) * max(0.0, 1.0 - t / DURATION)
        value = sum(math.sin(2 * math.pi * f * t) for f in FREQUENCIES) / len(FREQUENCIES)
        samples += struct.pack("<h", int(value * envelope * 0.35 * 32767))
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(SAMPLE_RATE)
        stream.writeframes(bytes(samples))
    return buffer.getvalue()


def scale_wav(data: bytes, volume: float) -> bytes:
    """按音量（0.0–1.0）缩放 16 位 PCM WAV，返回新的 WAV 字节。"""
    factor = max(0.0, min(1.0, volume))
    with wave.open(io.BytesIO(data), "rb") as source:
        params = source.getparams()
        frames = source.readframes(source.getnframes())
    if params.sampwidth != 2:
        raise ValueError("只支持 16 位 WAV")
    count = len(frames) // 2
    values = struct.unpack(f"<{count}h", frames)
    scaled = struct.pack(f"<{count}h", *(max(-32768, min(32767, int(v * factor))) for v in values))
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as target:
        target.setparams(params)
        target.writeframes(scaled)
    return buffer.getvalue()
