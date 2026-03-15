from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import DEFAULT_DB_PATH
from question_bank.quality import write_quality_report


def main() -> None:
    parser = argparse.ArgumentParser(description="执行题库质量检查并生成报告。")
    parser.add_argument("--db-path", default=str(DEFAULT_DB_PATH), help="SQLite 数据库路径。")
    parser.add_argument("--output", default=str(PROJECT_ROOT / "docs" / "quality_report.json"), help="输出 JSON 路径。")
    args = parser.parse_args()

    report = write_quality_report(Path(args.db_path), PROJECT_ROOT, Path(args.output))
    print(report)


if __name__ == "__main__":
    main()
