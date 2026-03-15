from __future__ import annotations

import json
from pathlib import Path

from .db import connect


def list_papers(db_path: Path) -> list[dict]:
    with connect(db_path) as conn:
        rows = conn.execute(
            "SELECT paper_id, year, stage, title, source_pdf_path, is_official, notes FROM papers ORDER BY year, paper_id"
        ).fetchall()
        return [dict(row) for row in rows]


def list_questions(
    db_path: Path,
    *,
    year: int | None = None,
    paper_id: str | None = None,
    category: str | None = None,
    has_assets: bool | None = None,
    has_answer: bool | None = None,
    min_avg_score: float | None = None,
    max_avg_score: float | None = None,
    tag: str | None = None,
    query: str | None = None,
    limit: int = 20,
    offset: int = 0,
) -> list[dict]:
    clauses = ["1 = 1"]
    params: list[object] = []
    if year is not None:
        clauses.append("p.year = ?")
        params.append(year)
    if paper_id is not None:
        clauses.append("q.paper_id = ?")
        params.append(paper_id)
    if category is not None:
        clauses.append("q.category = ?")
        params.append(category)
    if has_assets is not None:
        clauses.append(
            "EXISTS (SELECT 1 FROM question_assets qa WHERE qa.question_id = q.question_id)"
            if has_assets
            else "NOT EXISTS (SELECT 1 FROM question_assets qa WHERE qa.question_id = q.question_id)"
        )
    if has_answer is not None:
        clauses.append(
            "(COALESCE(q.answer_text, '') <> '' OR COALESCE(q.answer_latex, '') <> '')"
            if has_answer
            else "(COALESCE(q.answer_text, '') = '' AND COALESCE(q.answer_latex, '') = '')"
        )
    if min_avg_score is not None:
        clauses.append("EXISTS (SELECT 1 FROM question_stats qs WHERE qs.question_id = q.question_id AND qs.avg_score >= ?)")
        params.append(min_avg_score)
    if max_avg_score is not None:
        clauses.append("EXISTS (SELECT 1 FROM question_stats qs WHERE qs.question_id = q.question_id AND qs.avg_score <= ?)")
        params.append(max_avg_score)
    if tag is not None:
        clauses.append("q.tags_json LIKE ?")
        params.append(f'%"{tag}"%')
    if query is not None:
        clauses.append("(q.plain_text LIKE ? OR q.latex_body LIKE ?)")
        params.extend([f"%{query}%", f"%{query}%"])

    sql = f"""
        SELECT q.question_id, q.paper_id, q.question_no, q.category, q.status,
               q.plain_text, q.tags_json, p.year, p.stage, p.title
        FROM questions q
        JOIN papers p ON p.paper_id = q.paper_id
        WHERE {' AND '.join(clauses)}
        ORDER BY p.year DESC, q.paper_id, q.question_no
        LIMIT ? OFFSET ?
    """
    params.extend([limit, offset])
    with connect(db_path) as conn:
        rows = conn.execute(sql, params).fetchall()
        payload = []
        for row in rows:
            item = dict(row)
            item["tags"] = json.loads(item.pop("tags_json"))
            payload.append(item)
        return payload


def get_question_detail(db_path: Path, question_id: str) -> dict | None:
    with connect(db_path) as conn:
        row = conn.execute(
            """
            SELECT q.*, p.year, p.stage, p.title AS paper_title, p.source_pdf_path
            FROM questions q
            JOIN papers p ON p.paper_id = q.paper_id
            WHERE q.question_id = ?
            """,
            (question_id,),
        ).fetchone()
        if row is None:
            return None
        assets = conn.execute(
            """
            SELECT asset_id, kind, file_path, sha256, caption, sort_order
            FROM question_assets
            WHERE question_id = ?
            ORDER BY sort_order, asset_id
            """,
            (question_id,),
        ).fetchall()
        stats = conn.execute(
            """
            SELECT exam_session, participant_count, avg_score, score_std, full_mark_rate,
                   zero_score_rate, max_score, min_score, stats_source, stats_version
            FROM question_stats
            WHERE question_id = ?
            ORDER BY exam_session
            """,
            (question_id,),
        ).fetchall()
        difficulty = conn.execute(
            """
            SELECT exam_session, manual_level, derived_score, method, method_version,
                   confidence, feature_json
            FROM difficulty_scores
            WHERE question_id = ?
            ORDER BY exam_session
            """,
            (question_id,),
        ).fetchall()
        payload = dict(row)
        payload["tags"] = json.loads(payload.pop("tags_json"))
        payload["source_pages"] = {
            "start": payload.pop("source_page_start"),
            "end": payload.pop("source_page_end"),
        }
        payload["assets"] = [dict(item) for item in assets]
        payload["stats"] = [dict(item) for item in stats]
        payload["difficulty_scores"] = [
            {**dict(item), "feature_json": json.loads(item["feature_json"])} for item in difficulty
        ]
        return payload
