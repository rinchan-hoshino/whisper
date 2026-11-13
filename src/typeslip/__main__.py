from __future__ import annotations

import sys


def main() -> int:
    if len(sys.argv) >= 6 and sys.argv[1] == "--apply-update":
        from pathlib import Path

        from .adapters.updater import apply_update

        apply_update(Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4]), sys.argv[5])
        return 0
    from .app import run

    return run()


if __name__ == "__main__":
    raise SystemExit(main())
