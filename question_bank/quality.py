from __future__ import annotations

import json
from pathlib import Path

from .db import connect


def run_quality_checks(db_path: Path, project_root: Path) -> dict:
    report = {
        "missing_answers": [],
        "missing_assets": [],
        "missing_source_pages": [],
        "duplicate_question_numbers": [],
    }
    with connect(db_path) as conn:
        for row in conn.execute(
            "SELECT question_id FROM questions WHERE COALESCE(answer_text, '') = '' AND COALESCE(answer_latex, '') = ''"
        ).fetchall():
            report["missing_answers"].append(row["question_id"])
        for row in conn.execute(
            """
            SELECT question_id, source_page_start, source_page_end
            FROM questions
            WHERE source_page_start IS NULL OR source_page_end IS NULL
            """
        ).fetchall():
            report["missing_source_pages"].append(row["question_id"])
        duplicates = conn.execute(
            """
            SELECT paper_id, question_no, COUNT(*) AS duplicate_count
            FROM questions
            GROUP BY paper_id, question_no
            HAVING COUNT(*) > 1
            """
        ).fetchall()
        report["duplicate_question_numbers"] = [dict(item) for item in duplicates]
        for row in conn.execute("SELECT question_id, file_path FROM question_assets").fetchall():
            if not (project_root / row["file_path"]).exists():
                report["missing_assets"].append(dict(row))
    return report


def write_quality_report(db_path: Path, project_root: Path, output_path: Path) -> dict:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    report = run_quality_checks(db_path, project_root)
    with output_path.open("w", encoding="utf-8") as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2)
    return report
