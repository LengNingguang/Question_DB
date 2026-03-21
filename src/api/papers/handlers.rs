use std::collections::HashSet;

use anyhow::Context;
use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    Json,
};
use sqlx::{query, Row};
use uuid::Uuid;

use super::models::{
    validate_paper_type, CreatePaperRequest, PaperDeleteResponse, PaperDetail, PaperWriteResponse,
    UpdatePaperRequest,
};
use crate::api::{
    questions::queries::{
        load_question_tags, map_paper_detail, map_paper_question_summary, map_paper_summary,
    },
    shared::error::{ApiError, ApiResult},
    AppState,
};

pub(crate) async fn list_papers(
    State(state): State<AppState>,
) -> Result<Json<Vec<super::models::PaperSummary>>, StatusCode> {
    let rows = query(
        r#"
        SELECT p.paper_id::text AS paper_id, p.edition, p.paper_type, p.title, p.notes,
               COUNT(pq.question_id) AS question_count,
               to_char(p.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM papers p
        LEFT JOIN paper_questions pq ON pq.paper_id = p.paper_id
        GROUP BY p.paper_id, p.edition, p.paper_type, p.title, p.notes, p.created_at, p.updated_at
        ORDER BY p.created_at DESC, p.paper_id
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows.into_iter().map(map_paper_summary).collect()))
}

pub(crate) async fn create_paper(
    State(state): State<AppState>,
    Json(request): Json<CreatePaperRequest>,
) -> ApiResult<PaperWriteResponse> {
    let paper_type = request.paper_type.trim().to_string();
    validate_paper_type(&paper_type).map_err(|err| ApiError::bad_request(err.to_string()))?;

    if request.question_ids.is_empty() {
        return Err(ApiError::bad_request("question_ids must not be empty"));
    }

    let mut seen_question_ids = HashSet::new();
    for question_id in &request.question_ids {
        if !seen_question_ids.insert(question_id.clone()) {
            return Err(ApiError::bad_request(format!(
                "duplicate question_id in question_ids: {question_id}"
            )));
        }
        Uuid::parse_str(question_id)
            .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;
    }

    ensure_questions_exist(&state.pool, &request.question_ids).await?;

    let paper_id = Uuid::new_v4().to_string();
    let mut tx = state.pool.begin().await.context("begin paper tx failed")?;

    query(
        r#"
        INSERT INTO papers (
            paper_id, edition, paper_type, title, notes, created_at, updated_at
        )
        VALUES ($1::uuid, $2, $3, $4, $5, NOW(), NOW())
        "#,
    )
    .bind(&paper_id)
    .bind(request.edition.as_deref())
    .bind(&paper_type)
    .bind(&request.title)
    .bind(request.notes.as_deref())
    .execute(&mut *tx)
    .await
    .context("insert paper failed")?;

    for (idx, question_id) in request.question_ids.iter().enumerate() {
        query(
            r#"
            INSERT INTO paper_questions (paper_id, question_id, sort_order, created_at)
            VALUES ($1::uuid, $2::uuid, $3, NOW())
            "#,
        )
        .bind(&paper_id)
        .bind(question_id)
        .bind(i32::try_from(idx + 1).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await
        .with_context(|| format!("insert paper question ref failed: {question_id}"))?;
    }

    tx.commit().await.context("commit paper failed")?;

    Ok(Json(PaperWriteResponse {
        paper_id,
        status: "saved",
        question_count: request.question_ids.len(),
    }))
}

pub(crate) async fn update_paper(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdatePaperRequest>,
) -> ApiResult<PaperDetail> {
    Uuid::parse_str(&paper_id)
        .map_err(|_| ApiError::bad_request(format!("invalid paper_id: {paper_id}")))?;

    let update = request
        .normalize()
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    if let Some(question_ids) = &update.question_ids {
        for question_id in question_ids {
            Uuid::parse_str(question_id).map_err(|_| {
                ApiError::bad_request(format!("invalid question_id: {question_id}"))
            })?;
        }
        ensure_questions_exist(&state.pool, question_ids).await?;
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin paper update tx failed")?;
    let exists = query("SELECT 1 FROM papers WHERE paper_id = $1::uuid")
        .bind(&paper_id)
        .fetch_optional(&mut *tx)
        .await
        .context("check paper existence failed")?
        .is_some();
    if !exists {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("paper not found: {paper_id}"),
        });
    }

    if let Some(edition) = &update.edition {
        query("UPDATE papers SET edition = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(edition.as_deref())
            .execute(&mut *tx)
            .await
            .context("update paper edition failed")?;
    }

    if let Some(paper_type) = &update.paper_type {
        query("UPDATE papers SET paper_type = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(paper_type)
            .execute(&mut *tx)
            .await
            .context("update paper type failed")?;
    }

    if let Some(title) = &update.title {
        query("UPDATE papers SET title = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(title)
            .execute(&mut *tx)
            .await
            .context("update paper title failed")?;
    }

    if let Some(notes) = &update.notes {
        query("UPDATE papers SET notes = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(notes.as_deref())
            .execute(&mut *tx)
            .await
            .context("update paper notes failed")?;
    }

    if let Some(question_ids) = &update.question_ids {
        query("DELETE FROM paper_questions WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .execute(&mut *tx)
            .await
            .context("replace paper questions failed")?;

        for (idx, question_id) in question_ids.iter().enumerate() {
            query(
                r#"
                INSERT INTO paper_questions (paper_id, question_id, sort_order, created_at)
                VALUES ($1::uuid, $2::uuid, $3, NOW())
                "#,
            )
            .bind(&paper_id)
            .bind(question_id)
            .bind(i32::try_from(idx + 1).unwrap_or(i32::MAX))
            .execute(&mut *tx)
            .await
            .with_context(|| format!("replace paper question ref failed: {question_id}"))?;
        }

        query("UPDATE papers SET updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .execute(&mut *tx)
            .await
            .context("touch paper updated_at after question update failed")?;
    }

    tx.commit().await.context("commit paper update failed")?;

    fetch_paper_detail(&state, &paper_id)
        .await
        .map(Json)
        .map_err(map_paper_detail_error)
}

pub(crate) async fn delete_paper(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
) -> ApiResult<PaperDeleteResponse> {
    Uuid::parse_str(&paper_id)
        .map_err(|_| ApiError::bad_request(format!("invalid paper_id: {paper_id}")))?;

    let result = query("DELETE FROM papers WHERE paper_id = $1::uuid")
        .bind(&paper_id)
        .execute(&state.pool)
        .await
        .context("delete paper failed")?;

    if result.rows_affected() == 0 {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("paper not found: {paper_id}"),
        });
    }

    Ok(Json(PaperDeleteResponse {
        paper_id,
        status: "deleted",
    }))
}

pub(crate) async fn get_paper_detail(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<PaperDetail>, StatusCode> {
    Uuid::parse_str(&paper_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    fetch_paper_detail(&state, &paper_id)
        .await
        .map(Json)
        .map_err(map_paper_detail_status)
}

async fn fetch_paper_detail(state: &AppState, paper_id: &str) -> Result<PaperDetail, ApiError> {
    let paper_row = query(
        r#"
        SELECT paper_id::text AS paper_id, edition, paper_type, title, notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM papers
        WHERE paper_id = $1::uuid
        "#,
    )
    .bind(paper_id)
    .fetch_optional(&state.pool)
    .await
    .context("load paper detail failed")
    .map_err(ApiError::from)?
    .ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("paper not found: {paper_id}"),
    })?;

    let question_rows = query(
        r#"
        SELECT q.question_id::text AS question_id, pq.sort_order, q.category, q.status
        FROM paper_questions pq
        JOIN questions q ON q.question_id = pq.question_id
        WHERE pq.paper_id = $1::uuid
        ORDER BY pq.sort_order
        "#,
    )
    .bind(paper_id)
    .fetch_all(&state.pool)
    .await
    .context("load paper questions failed")
    .map_err(ApiError::from)?;

    let mut questions = Vec::with_capacity(question_rows.len());
    for row in question_rows {
        let question_id: String = row.get("question_id");
        let tags = load_question_tags(&state.pool, &question_id)
            .await
            .context("load paper question tags failed")
            .map_err(ApiError::from)?;
        questions.push(map_paper_question_summary(row, tags));
    }

    Ok(map_paper_detail(paper_row, questions))
}

async fn ensure_questions_exist(
    pool: &sqlx::PgPool,
    question_ids: &[String],
) -> Result<(), ApiError> {
    let existing_questions = query("SELECT question_id::text AS question_id FROM questions")
        .fetch_all(pool)
        .await
        .context("load existing questions failed")
        .map_err(ApiError::from)?
        .into_iter()
        .map(|row| row.get::<String, _>("question_id"))
        .collect::<HashSet<_>>();

    for question_id in question_ids {
        if !existing_questions.contains(question_id) {
            return Err(ApiError::bad_request(format!(
                "unknown question_id in question_ids: {question_id}"
            )));
        }
    }

    Ok(())
}

fn map_paper_detail_error(err: ApiError) -> ApiError {
    if err.status == StatusCode::NOT_FOUND || err.status == StatusCode::BAD_REQUEST {
        err
    } else {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.message,
        }
    }
}

fn map_paper_detail_status(err: ApiError) -> StatusCode {
    err.status
}
