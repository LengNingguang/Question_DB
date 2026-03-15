from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.bundle import validate_bundle


def main() -> None:
    parser = argparse.ArgumentParser(description="校验中间清洗包格式。")
    parser.add_argument("bundle_path", type=Path, help="清洗包目录。")
    args = parser.parse_args()
    result = validate_bundle(args.bundle_path)
    for warning in result.warnings:
        print(f"[WARN] {warning}")
    for error in result.errors:
        print(f"[ERROR] {error}")
    if not result.ok:
        raise SystemExit(1)
    print(f"校验通过: {args.bundle_path}")


if __name__ == "__main__":
    main()
