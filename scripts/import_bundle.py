from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import DEFAULT_DB_PATH
from question_bank.importer import import_bundle


def main() -> None:
    parser = argparse.ArgumentParser(description="导入清洗包到 SQLite。")
    parser.add_argument("bundle_path", type=Path, help="清洗包目录。")
    parser.add_argument("--db-path", default=str(DEFAULT_DB_PATH), help="SQLite 数据库路径。")
    parser.add_argument("--commit", action="store_true", help="执行实际导入。默认仅 dry-run。")
    parser.add_argument("--allow-similar", action="store_true", help="允许与已有题目高相似时继续导入。")
    args = parser.parse_args()

    result = import_bundle(
        bundle_path=args.bundle_path,
        db_path=Path(args.db_path),
        dry_run=not args.commit,
        allow_similar=args.allow_similar,
    )
    print(result)
    if result["errors"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
