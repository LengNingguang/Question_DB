from __future__ import annotations

try:
    from fastapi import FastAPI, HTTPException, Query
except ModuleNotFoundError as exc:
    raise RuntimeError(
        "FastAPI 未安装。请先执行 `pip install -r requirements.txt` 后再启动 API。"
    ) from exc

from question_bank.config import DEFAULT_DB_PATH
from question_bank.repository import get_question_detail, list_papers, list_questions

app = FastAPI(title="CPHOS Question Bank API", version="1.0.0")


@app.get("/health")
def health() -> dict:
    return {"status": "ok"}


@app.get("/papers")
def papers() -> list[dict]:
    return list_papers(DEFAULT_DB_PATH)


@app.get("/questions")
def questions(
    year: int | None = None,
    paper_id: str | None = None,
    category: str | None = Query(default=None, pattern="^(theory|experiment)$"),
    has_assets: bool | None = None,
    has_answer: bool | None = None,
    min_avg_score: float | None = None,
    max_avg_score: float | None = None,
    tag: str | None = None,
    q: str | None = None,
    limit: int = Query(default=20, ge=1, le=100),
    offset: int = Query(default=0, ge=0),
) -> list[dict]:
    return list_questions(
        DEFAULT_DB_PATH,
        year=year,
        paper_id=paper_id,
        category=category,
        has_assets=has_assets,
        has_answer=has_answer,
        min_avg_score=min_avg_score,
        max_avg_score=max_avg_score,
        tag=tag,
        query=q,
        limit=limit,
        offset=offset,
    )


@app.get("/questions/{question_id}")
def question_detail(question_id: str) -> dict:
    result = get_question_detail(DEFAULT_DB_PATH, question_id)
    if result is None:
        raise HTTPException(status_code=404, detail="Question not found")
    return result


@app.get("/search")
def search(
    q: str = Query(..., min_length=1),
    limit: int = Query(default=20, ge=1, le=100),
    offset: int = Query(default=0, ge=0),
) -> list[dict]:
    return list_questions(DEFAULT_DB_PATH, query=q, limit=limit, offset=offset)
