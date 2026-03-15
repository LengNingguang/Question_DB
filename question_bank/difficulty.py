from __future__ import annotations

from pathlib import Path

from .db import connect
from .utils import dumps_json, utc_now_iso


def derive_difficulty(avg_score: float, max_score: float, zero_score_rate: float, full_mark_rate: float) -> float:
    if max_score <= 0:
        raise ValueError("max_score 必须大于 0。")
    normalized_avg = max(0.0, min(1.0, avg_score / max_score))
    derived = 0.55 * (1 - normalized_avg) + 0.25 * zero_score_rate + 0.20 * (1 - full_mark_rate)
    return max(0.0, min(1.0, derived))


def update_difficulty_scores(db_path: Path, method_version: str = "baseline-v1") -> int:
    now = utc_now_iso()
    updated = 0
    with connect(db_path) as conn:
        rows = conn.execute(
            """
            SELECT question_id, exam_session, participant_count, avg_score, zero_score_rate,
                   full_mark_rate, max_score
            FROM question_stats
            """
        ).fetchall()
        for row in rows:
            feature_json = {
                "participant_count": row["participant_count"],
                "avg_score": row["avg_score"],
                "zero_score_rate": row["zero_score_rate"],
                "full_mark_rate": row["full_mark_rate"],
                "max_score": row["max_score"],
            }
            if row["participant_count"] < 3:
                derived_score = None
                confidence = 0.0
            else:
                derived_score = derive_difficulty(
                    avg_score=row["avg_score"],
                    max_score=row["max_score"],
                    zero_score_rate=row["zero_score_rate"],
                    full_mark_rate=row["full_mark_rate"],
                )
                confidence = min(1.0, row["participant_count"] / 50)
            conn.execute(
                """
                INSERT OR REPLACE INTO difficulty_scores (
                    question_id, exam_session, manual_level, derived_score, method,
                    method_version, confidence, feature_json, created_at, updated_at
                ) VALUES (
                    ?, ?, NULL, ?, 'baseline_rule',
                    ?, ?, ?,
                    COALESCE((
                        SELECT created_at FROM difficulty_scores
                        WHERE question_id = ? AND exam_session = ? AND method = 'baseline_rule' AND method_version = ?
                    ), ?), ?
                )
                """,
                (
                    row["question_id"],
                    row["exam_session"],
                    derived_score,
                    method_version,
                    confidence,
                    dumps_json(feature_json),
                    row["question_id"],
                    row["exam_session"],
                    method_version,
                    now,
                    now,
                ),
            )
            updated += 1
        conn.commit()
    return updated
