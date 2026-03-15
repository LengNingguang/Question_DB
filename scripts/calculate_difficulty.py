from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import DEFAULT_DB_PATH
from question_bank.difficulty import update_difficulty_scores


def main() -> None:
    parser = argparse.ArgumentParser(description="根据题目统计生成基础难度评分。")
    parser.add_argument("--db-path", default=str(DEFAULT_DB_PATH), help="SQLite 数据库路径。")
    parser.add_argument("--method-version", default="baseline-v1", help="算法版本号。")
    args = parser.parse_args()

    count = update_difficulty_scores(Path(args.db_path), method_version=args.method_version)
    print(f"已更新 {count} 条难度记录。")


if __name__ == "__main__":
    main()
