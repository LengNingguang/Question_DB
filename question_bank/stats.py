from __future__ import annotations

import csv
import math
from collections import defaultdict
from pathlib import Path

from .db import connect
from .utils import utc_now_iso


def aggregate_score_rows(csv_path: Path) -> list[dict]:
    grouped: dict[tuple[str, str], list[tuple[float, float]]] = defaultdict(list)
    with csv_path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle)
        required = {"question_id", "exam_session", "score", "max_score"}
        missing = required - set(reader.fieldnames or [])
        if missing:
            raise ValueError(f"成绩表缺少字段: {sorted(missing)}")
        for row in reader:
            grouped[(row["question_id"], row["exam_session"])].append(
                (float(row["score"]), float(row["max_score"]))
            )

    results: list[dict] = []
    for (question_id, exam_session), values in grouped.items():
        scores = [score for score, _ in values]
        max_scores = [max_score for _, max_score in values]
        participant_count = len(scores)
        avg_score = sum(scores) / participant_count
        max_score = max(max_scores)
        min_score = min(scores)
        variance = sum((score - avg_score) ** 2 for score in scores) / participant_count
        full_mark_rate = sum(1 for score, max_s in values if math.isclose(score, max_s)) / participant_count
        zero_score_rate = sum(1 for score in scores if math.isclose(score, 0.0)) / participant_count
        results.append(
            {
                "question_id": question_id,
                "exam_session": exam_session,
                "participant_count": participant_count,
                "avg_score": avg_score,
                "score_std": math.sqrt(variance),
                "full_mark_rate": full_mark_rate,
                "zero_score_rate": zero_score_rate,
                "max_score": max_score,
                "min_score": min_score,
            }
        )
    return sorted(results, key=lambda item: (item["exam_session"], item["question_id"]))


def upsert_stats(db_path: Path, stats_rows: list[dict], stats_source: str, stats_version: str) -> int:
    now = utc_now_iso()
    with connect(db_path) as conn:
        for row in stats_rows:
            conn.execute(
                """
                INSERT OR REPLACE INTO question_stats (
                    question_id, exam_session, participant_count, avg_score, score_std,
                    full_mark_rate, zero_score_rate, max_score, min_score,
                    stats_source, stats_version, created_at, updated_at
                ) VALUES (
                    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    COALESCE((
                        SELECT created_at FROM question_stats
                        WHERE question_id = ? AND exam_session = ? AND stats_version = ?
                    ), ?), ?
                )
                """,
                (
                    row["question_id"],
                    row["exam_session"],
                    row["participant_count"],
                    row["avg_score"],
                    row["score_std"],
                    row["full_mark_rate"],
                    row["zero_score_rate"],
                    row["max_score"],
                    row["min_score"],
                    stats_source,
                    stats_version,
                    row["question_id"],
                    row["exam_session"],
                    stats_version,
                    now,
                    now,
                ),
            )
        conn.commit()
    return len(stats_rows)
