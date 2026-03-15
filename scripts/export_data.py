from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import DEFAULT_DB_PATH, EXPORTS_DIR
from question_bank.exporter import export_csv, export_jsonl


def main() -> None:
    parser = argparse.ArgumentParser(description="导出题库为 JSONL 或 CSV。")
    parser.add_argument("--db-path", default=str(DEFAULT_DB_PATH), help="SQLite 数据库路径。")
    parser.add_argument("--format", choices=["jsonl", "csv"], default="jsonl", help="导出格式。")
    parser.add_argument("--public", action="store_true", help="导出半公开版本，不包含答案。")
    parser.add_argument("--output", default="", help="输出文件路径。")
    args = parser.parse_args()

    suffix = "public" if args.public else "internal"
    output = Path(args.output) if args.output else EXPORTS_DIR / f"question_bank_{suffix}.{args.format}"
    include_answers = not args.public
    if args.format == "jsonl":
        count = export_jsonl(Path(args.db_path), output, include_answers=include_answers)
    else:
        count = export_csv(Path(args.db_path), output, include_answers=include_answers)
    print(f"已导出 {count} 道题到 {output}")


if __name__ == "__main__":
    main()
