from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from question_bank.config import RAW_DIR


def detect_kind(path: Path) -> str:
    ext = path.suffix.lower()
    if ext == ".pdf":
        return "pdf"
    if ext in {".png", ".jpg", ".jpeg", ".svg"}:
        return "image"
    if ext in {".txt", ".md", ".tex", ".docx"}:
        return "text"
    if ext in {".xls", ".xlsx", ".csv"}:
        return "scores"
    return "other"


def main() -> None:
    parser = argparse.ArgumentParser(description="扫描 raw/ 下原始资料并生成 inventory CSV。")
    parser.add_argument("--raw-dir", default=str(RAW_DIR), help="原始资料目录。")
    parser.add_argument("--output", default=str(PROJECT_ROOT / "docs" / "material_inventory.csv"), help="输出 CSV 路径。")
    args = parser.parse_args()

    raw_dir = Path(args.raw_dir)
    rows: list[dict] = []
    for path in sorted(p for p in raw_dir.rglob("*") if p.is_file()):
        rows.append(
            {
                "asset_id": path.stem,
                "relative_path": str(path.relative_to(PROJECT_ROOT)).replace("\\", "/"),
                "kind": detect_kind(path),
                "year_guess": "",
                "status": "unreviewed",
                "parse_risk": "",
                "notes": "",
            }
        )

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8-sig", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["asset_id", "relative_path", "kind", "year_guess", "status", "parse_risk", "notes"],
        )
        writer.writeheader()
        writer.writerows(rows)
    print(f"已生成资料总表: {output_path} ({len(rows)} 条)")


if __name__ == "__main__":
    main()
