from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import DEFAULT_DB_PATH
from question_bank.stats import aggregate_score_rows, upsert_stats


def main() -> None:
    parser = argparse.ArgumentParser(description="从逐题得分 CSV 聚合统计并写入数据库。")
    parser.add_argument("csv_path", type=Path, help="CSV 路径，需包含 question_id/exam_session/score/max_score。")
    parser.add_argument("--db-path", default=str(DEFAULT_DB_PATH), help="SQLite 数据库路径。")
    parser.add_argument("--stats-source", default="manual_csv", help="统计来源标签。")
    parser.add_argument("--stats-version", default="v1", help="统计版本标签。")
    args = parser.parse_args()

    rows = aggregate_score_rows(args.csv_path)
    count = upsert_stats(Path(args.db_path), rows, stats_source=args.stats_source, stats_version=args.stats_version)
    print(f"已写入 {count} 条统计记录。")


if __name__ == "__main__":
    main()
