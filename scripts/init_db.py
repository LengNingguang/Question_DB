from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import DEFAULT_DB_PATH
from question_bank.db import initialize_database


def main() -> None:
    parser = argparse.ArgumentParser(description="初始化 CPHOS 题库 SQLite 数据库。")
    parser.add_argument("--db-path", default=str(DEFAULT_DB_PATH), help="SQLite 数据库路径。")
    args = parser.parse_args()
    initialize_database(Path(args.db_path))
    print(f"数据库已初始化: {args.db_path}")


if __name__ == "__main__":
    main()
