"""生成提示音 src/typeslip/assets/ding-dong.wav（代码合成，不用系统音）。"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from typeslip.core.sound import synthesize_ding_dong  # noqa: E402

if __name__ == "__main__":
    (ROOT / "src" / "typeslip" / "assets" / "ding-dong.wav").write_bytes(synthesize_ding_dong())
    print("ok")
