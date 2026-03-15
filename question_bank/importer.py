from __future__ import annotations

import difflib
from pathlib import Path

from .bundle import load_bundle, validate_bundle
from .db import connect
from .utils import dumps_json, sha256_file, utc_now_iso


def _find_similar_questions(conn, plain_text: str, threshold: float = 0.92) -> list[str]:
    matches: list[str] = []
    for row in conn.execute("SELECT question_id, plain_text FROM questions").fetchall():
        ratio = difflib.SequenceMatcher(a=plain_text, b=row["plain_text"]).ratio()
        if ratio >= threshold:
            matches.append(f"{row['question_id']} ({ratio:.3f})")
    return matches


def _project_root_from_bundle(bundle_path: Path) -> Path:
    return bundle_path.resolve().parents[1]


def import_bundle(bundle_path: Path, db_path: Path, dry_run: bool = True, allow_similar: bool = False) -> dict:
    validation = validate_bundle(bundle_path)
    manifest, questions = load_bundle(bundle_path)
    warnings = list(validation.warnings)
    errors = list(validation.errors)
    imported_questions = 0
    imported_assets = 0
    started_at = utc_now_iso()
    finished_at = started_at
    project_root = _project_root_from_bundle(bundle_path)

    with connect(db_path) as conn:
        paper = manifest["paper"]
        for question in questions:
            existing = conn.execute(
                "SELECT question_id FROM questions WHERE paper_id = ? AND question_no = ?",
                (paper["paper_id"], question["question_no"]),
            ).fetchone()
            if existing and existing["question_id"] != question["question_id"]:
                errors.append(
                    f"题号冲突: 同一试卷 {paper['paper_id']} 的题号 {question['question_no']} 已被 {existing['question_id']} 使用。"
                )
            similar = _find_similar_questions(conn, question["plain_text"])
            if similar and question["question_id"] not in {item.split()[0] for item in similar}:
                message = f"{question['question_id']} 与已有题目文本高度相似: {', '.join(similar)}"
                if allow_similar:
                    warnings.append(message)
                else:
                    errors.append(message)

        status = "failed" if errors else ("dry_run" if dry_run else "committed")
        details = {
            "bundle_name": manifest.get("bundle_name"),
            "paper_id": paper.get("paper_id"),
            "warnings": warnings,
            "errors": errors,
        }
        if not errors and not dry_run:
            now = utc_now_iso()
            conn.execute(
                """
                INSERT OR REPLACE INTO papers (
                    paper_id, year, stage, title, source_pdf_path, is_official, notes, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, COALESCE((SELECT created_at FROM papers WHERE paper_id = ?), ?), ?)
                """,
                (
                    paper["paper_id"],
                    paper["year"],
                    paper["stage"],
                    paper["title"],
                    paper.get("source_pdf_path"),
                    1 if paper.get("is_official", True) else 0,
                    paper.get("notes"),
                    paper["paper_id"],
                    now,
                    now,
                ),
            )
            for question in questions:
                conn.execute(
                    """
                    INSERT OR REPLACE INTO questions (
                        question_id, paper_id, question_no, category, latex_body, plain_text,
                        answer_latex, answer_text, status, source_page_start, source_page_end,
                        tags_json, created_at, updated_at
                    ) VALUES (
                        ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                        COALESCE((SELECT created_at FROM questions WHERE question_id = ?), ?), ?
                    )
                    """,
                    (
                        question["question_id"],
                        paper["paper_id"],
                        question["question_no"],
                        question["category"],
                        question["latex_body"],
                        question["plain_text"],
                        question.get("answer_latex"),
                        question.get("answer_text"),
                        question["status"],
                        question["source_pages"]["start"],
                        question["source_pages"]["end"],
                        dumps_json(question.get("tags", [])),
                        question["question_id"],
                        now,
                        now,
                    ),
                )
                imported_questions += 1
                conn.execute("DELETE FROM question_assets WHERE question_id = ?", (question["question_id"],))
                for asset in question.get("assets", []):
                    asset_path = (bundle_path / asset["file_path"]).resolve()
                    stored_path = asset_path.relative_to(project_root).as_posix()
                    conn.execute(
                        """
                        INSERT INTO question_assets (
                            asset_id, question_id, kind, file_path, sha256, caption, sort_order, created_at
                        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                        """,
                        (
                            asset["asset_id"],
                            question["question_id"],
                            asset["kind"],
                            stored_path,
                            asset.get("sha256") or sha256_file(asset_path),
                            asset.get("caption"),
                            asset.get("sort_order", 0),
                            now,
                        ),
                    )
                    imported_assets += 1
            finished_at = utc_now_iso()
            conn.commit()
        else:
            finished_at = utc_now_iso()

        conn.execute(
            """
            INSERT INTO import_runs (
                run_label, bundle_path, dry_run, status, item_count, warning_count, error_count,
                details_json, started_at, finished_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                manifest.get("run_label", bundle_path.name),
                str(bundle_path.resolve()),
                1 if dry_run else 0,
                status,
                len(questions),
                len(warnings),
                len(errors),
                dumps_json(details),
                started_at,
                finished_at,
            ),
        )
        conn.commit()

    return {
        "bundle_name": manifest.get("bundle_name"),
        "paper_id": manifest["paper"]["paper_id"],
        "status": status,
        "question_count": len(questions),
        "imported_questions": imported_questions,
        "imported_assets": imported_assets,
        "warnings": warnings,
        "errors": errors,
    }
