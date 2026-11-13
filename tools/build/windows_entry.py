"""Nuitka 入口：把 src 放进路径后交给 typeslip。"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from typeslip.__main__ import main  # noqa: E402

if __name__ == "__main__":
    raise SystemExit(main())
