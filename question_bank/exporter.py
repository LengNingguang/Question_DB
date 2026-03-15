from __future__ import annotations

import csv
import json
from pathlib import Path

from .db import connect


def _question_payload(conn, include_answers: bool) -> list[dict]:
    rows = conn.execute(
        """
        SELECT q.*, p.title AS paper_title, p.year, p.stage
        FROM questions q
        JOIN papers p ON p.paper_id = q.paper_id
        ORDER BY p.year, p.paper_id, q.question_no
        """
    ).fetchall()
    payload: list[dict] = []
    for row in rows:
        assets = conn.execute(
            """
            SELECT asset_id, kind, file_path, caption, sort_order
            FROM question_assets
            WHERE question_id = ?
            ORDER BY sort_order, asset_id
            """,
            (row["question_id"],),
        ).fetchall()
        stats = conn.execute(
            """
            SELECT exam_session, participant_count, avg_score, score_std, full_mark_rate,
                   zero_score_rate, max_score, min_score, stats_source, stats_version
            FROM question_stats
            WHERE question_id = ?
            ORDER BY exam_session
            """,
            (row["question_id"],),
        ).fetchall()
        item = {
            "question_id": row["question_id"],
            "paper_id": row["paper_id"],
            "paper_title": row["paper_title"],
            "year": row["year"],
            "stage": row["stage"],
            "question_no": row["question_no"],
            "category": row["category"],
            "latex_body": row["latex_body"],
            "plain_text": row["plain_text"],
            "status": row["status"],
            "source_pages": {"start": row["source_page_start"], "end": row["source_page_end"]},
            "tags": json.loads(row["tags_json"]),
            "assets": [dict(asset) for asset in assets],
            "stats": [dict(stat) for stat in stats],
        }
        if include_answers:
            item["answer_latex"] = row["answer_latex"]
            item["answer_text"] = row["answer_text"]
        payload.append(item)
    return payload


def export_jsonl(db_path: Path, output_path: Path, include_answers: bool) -> int:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with connect(db_path) as conn, output_path.open("w", encoding="utf-8") as handle:
        payload = _question_payload(conn, include_answers=include_answers)
        for item in payload:
            handle.write(json.dumps(item, ensure_ascii=False) + "\n")
    return len(payload)


def export_csv(db_path: Path, output_path: Path, include_answers: bool) -> int:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with connect(db_path) as conn, output_path.open("w", encoding="utf-8-sig", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[
                "question_id",
                "paper_id",
                "question_no",
                "category",
                "status",
                "year",
                "stage",
                "plain_text",
                "answer_text",
                "tags",
            ],
        )
        writer.writeheader()
        rows = conn.execute(
            """
            SELECT q.question_id, q.paper_id, q.question_no, q.category, q.status,
                   p.year, p.stage, q.plain_text, q.answer_text, q.tags_json
            FROM questions q
            JOIN papers p ON p.paper_id = q.paper_id
            ORDER BY p.year, q.question_no
            """
        ).fetchall()
        for row in rows:
            writer.writerow(
                {
                    "question_id": row["question_id"],
                    "paper_id": row["paper_id"],
                    "question_no": row["question_no"],
                    "category": row["category"],
                    "status": row["status"],
                    "year": row["year"],
                    "stage": row["stage"],
                    "plain_text": row["plain_text"],
                    "answer_text": row["answer_text"] if include_answers else "",
                    "tags": row["tags_json"],
                }
            )
    return len(rows)
