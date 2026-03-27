use std::collections::HashSet;

use anyhow::Context;
use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},
    http::StatusCode,
    Json,
};
use sqlx::{query, Postgres, QueryBuilder, Row};
use uuid::Uuid;

use super::{
    imports::{import_paper_zip, replace_paper_zip, MAX_UPLOAD_BYTES},
    models::{
        CreatePaperRequest, PaperDeleteResponse, PaperDetail, PaperFileReplaceResponse,
        PaperImportResponse, PapersParams, UpdatePaperRequest,
    },
    queries::{execute_papers_query, validate_and_build_papers_query},
};
use crate::api::{
    questions::queries::{
        load_question_tags, map_paper_detail, map_paper_question_summary, map_paper_summary,
    },
    shared::error::{ApiError, ApiResult},
    AppState,
};

pub(crate) async fn list_papers(
    Query(params): Query<PapersParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<super::models::PaperSummary>>, StatusCode> {
    let plan = validate_and_build_papers_query(&params).map_err(|_| StatusCode::BAD_REQUEST)?;
    let rows = execute_papers_query(&state.pool, &params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows.into_iter().map(map_paper_summary).collect()))
}

pub(crate) async fn create_paper(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<PaperImportResponse> {
    let mut file_name = None;
    let mut description = None;
    let mut title = None;
    let mut subtitle = None;
    let mut authors = None;
    let mut reviewers = None;
    let mut question_ids = None;
    let mut bytes = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("read multipart field failed: {err}")))?
    {
        let Some(name) = field.name() else {
            continue;
        };
        match name {
            "file" => {
                file_name = field.file_name().map(str::to_string);
                bytes = field
                    .bytes()
                    .await
                    .map_err(|err| {
                        ApiError::bad_request(format!("read uploaded file failed: {err}"))
                    })?
                    .to_vec();
            }
            "description" => {
                description = Some(read_text_field(field, "description").await?);
            }
            "title" => {
                title = Some(read_text_field(field, "title").await?);
            }
            "subtitle" => {
                subtitle = Some(read_text_field(field, "subtitle").await?);
            }
            "authors" => {
                authors = Some(read_json_string_list_field(field, "authors").await?);
            }
            "reviewers" => {
                reviewers = Some(read_json_string_list_field(field, "reviewers").await?);
            }
            "question_ids" => {
                question_ids = Some(read_json_string_list_field(field, "question_ids").await?);
            }
            _ => {}
        }
    }

    if bytes.is_empty() {
        return Err(ApiError::bad_request(
            "multipart form must include a non-empty 'file' field",
        ));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_request("uploaded zip exceeds 20 MiB limit"));
    }

    let request = CreatePaperRequest {
        description: description.ok_or_else(|| {
            ApiError::bad_request("multipart form must include a non-empty 'description' field")
        })?,
        title: title.ok_or_else(|| {
            ApiError::bad_request("multipart form must include a non-empty 'title' field")
        })?,
        subtitle: subtitle.ok_or_else(|| {
            ApiError::bad_request("multipart form must include a non-empty 'subtitle' field")
        })?,
        authors: authors.ok_or_else(|| {
            ApiError::bad_request("multipart form must include an 'authors' field")
        })?,
        reviewers: reviewers.ok_or_else(|| {
            ApiError::bad_request("multipart form must include a 'reviewers' field")
        })?,
        question_ids: question_ids.ok_or_else(|| {
            ApiError::bad_request("multipart form must include a non-empty 'question_ids' field")
        })?,
    }
    .normalize()
    .map_err(|err| ApiError::bad_request(err.to_string()))?;

    validate_question_ids(&request.question_ids)?;
    ensure_paper_questions_valid(&state.pool, &request.question_ids).await?;

    Ok(Json(
        import_paper_zip(&state.pool, file_name.as_deref(), &request, bytes)
            .await
            .map_err(map_paper_create_error)?,
    ))
}

pub(crate) async fn replace_paper_file(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<PaperFileReplaceResponse> {
    Uuid::parse_str(&paper_id)
        .map_err(|_| ApiError::bad_request(format!("invalid paper_id: {paper_id}")))?;

    let mut file_name = None;
    let mut bytes = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("read multipart field failed: {err}")))?
    {
        if field.name() != Some("file") {
            continue;
        }

        file_name = field.file_name().map(str::to_string);
        bytes = field
            .bytes()
            .await
            .map_err(|err| ApiError::bad_request(format!("read uploaded file failed: {err}")))?
            .to_vec();
    }

    if bytes.is_empty() {
        return Err(ApiError::bad_request(
            "multipart form must include a non-empty 'file' field",
        ));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_request("uploaded zip exceeds 20 MiB limit"));
    }

    Ok(Json(
        replace_paper_zip(&state.pool, &paper_id, file_name.as_deref(), bytes)
            .await
            .map_err(map_paper_file_replace_error)?,
    ))
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
        validate_question_ids(question_ids)?;
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

    let final_question_ids = if let Some(question_ids) = &update.question_ids {
        question_ids.clone()
    } else {
        query(
            "SELECT question_id::text AS question_id FROM paper_questions WHERE paper_id = $1::uuid ORDER BY sort_order",
        )
        .bind(&paper_id)
        .fetch_all(&mut *tx)
        .await
        .context("load paper question refs for validation failed")?
        .into_iter()
        .map(|row| row.get::<String, _>("question_id"))
        .collect()
    };

    ensure_paper_questions_valid(&mut *tx, &final_question_ids).await?;

    if let Some(description) = &update.description {
        query("UPDATE papers SET description = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(description)
            .execute(&mut *tx)
            .await
            .context("update paper description failed")?;
    }

    if let Some(title) = &update.title {
        query("UPDATE papers SET title = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(title)
            .execute(&mut *tx)
            .await
            .context("update paper title failed")?;
    }

    if let Some(subtitle) = &update.subtitle {
        query("UPDATE papers SET subtitle = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(subtitle)
            .execute(&mut *tx)
            .await
            .context("update paper subtitle failed")?;
    }

    if let Some(authors) = &update.authors {
        query("UPDATE papers SET authors = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(authors)
            .execute(&mut *tx)
            .await
            .context("update paper authors failed")?;
    }

    if let Some(reviewers) = &update.reviewers {
        query("UPDATE papers SET reviewers = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(reviewers)
            .execute(&mut *tx)
            .await
            .context("update paper reviewers failed")?;
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
        SELECT paper_id::text AS paper_id, description, title, subtitle, authors, reviewers,
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

async fn read_text_field(
    field: axum::extract::multipart::Field<'_>,
    field_name: &str,
) -> Result<String, ApiError> {
    field
        .text()
        .await
        .map_err(|err| ApiError::bad_request(format!("read {field_name} field failed: {err}")))
}

async fn read_json_string_list_field(
    field: axum::extract::multipart::Field<'_>,
    field_name: &str,
) -> Result<Vec<String>, ApiError> {
    let text = read_text_field(field, field_name).await?;
    serde_json::from_str::<Vec<String>>(&text)
        .map_err(|err| ApiError::bad_request(format!("invalid {field_name} field: {err}")))
}

fn validate_question_ids(question_ids: &[String]) -> Result<(), ApiError> {
    let mut seen_question_ids = HashSet::new();
    for question_id in question_ids {
        if !seen_question_ids.insert(question_id.clone()) {
            return Err(ApiError::bad_request(format!(
                "duplicate question_id in question_ids: {question_id}"
            )));
        }
        Uuid::parse_str(question_id)
            .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaperQuestionValidationRow {
    question_id: String,
    category: String,
    status: String,
}

async fn ensure_paper_questions_valid<'e, E>(
    executor: E,
    question_ids: &[String],
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let question_rows = load_paper_question_validation_rows(executor, question_ids).await?;
    validate_paper_question_rows(&question_rows)
}

async fn load_paper_question_validation_rows<'e, E>(
    executor: E,
    question_ids: &[String],
) -> Result<Vec<PaperQuestionValidationRow>, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    if question_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT q.question_id::text AS question_id, q.category, q.status FROM questions q WHERE q.question_id IN (",
    );
    for (idx, question_id) in question_ids.iter().enumerate() {
        if idx > 0 {
            builder.push(", ");
        }
        builder.push_bind(question_id).push("::uuid");
    }
    builder.push(')');

    let question_rows = builder
        .build()
        .fetch_all(executor)
        .await
        .context("load questions for paper validation failed")
        .map_err(ApiError::from)?
        .into_iter()
        .map(|row| PaperQuestionValidationRow {
            question_id: row.get("question_id"),
            category: row.get("category"),
            status: row.get("status"),
        })
        .collect::<Vec<_>>();

    let existing_question_ids = question_rows
        .iter()
        .map(|row| row.question_id.as_str())
        .collect::<HashSet<_>>();

    for question_id in question_ids {
        if !existing_question_ids.contains(question_id.as_str()) {
            return Err(ApiError::bad_request(format!(
                "unknown question_id in question_ids: {question_id}"
            )));
        }
    }

    Ok(question_rows)
}

fn validate_paper_question_rows(
    question_rows: &[PaperQuestionValidationRow],
) -> Result<(), ApiError> {
    let mut expected_category = None;

    for row in question_rows {
        match row.category.as_str() {
            "T" | "E" => {}
            other => {
                return Err(ApiError::bad_request(format!(
                    "question {} has category {other}; paper questions must all have category T or all have category E",
                    row.question_id
                )));
            }
        }

        if let Some(expected) = expected_category {
            if expected != row.category {
                return Err(ApiError::bad_request(format!(
                    "paper questions must all have the same category; found both {expected} and {}",
                    row.category
                )));
            }
        } else {
            expected_category = Some(row.category.as_str());
        }

        if !matches!(row.status.as_str(), "reviewed" | "used") {
            return Err(ApiError::bad_request(format!(
                "question {} has status {}; paper questions must all have status reviewed or used",
                row.question_id, row.status
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

fn map_paper_create_error(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("uploaded file is empty")
        || message.contains("uploaded zip exceeds")
        || message.contains("open zip archive failed")
    {
        ApiError::bad_request(message)
    } else {
        ApiError::from(err)
    }
}

fn map_paper_file_replace_error(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.starts_with("paper not found:") {
        ApiError {
            status: StatusCode::NOT_FOUND,
            message,
        }
    } else if message.contains("uploaded file is empty")
        || message.contains("uploaded zip exceeds")
        || message.contains("open zip archive failed")
    {
        ApiError::bad_request(message)
    } else {
        ApiError::from(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_paper_question_rows, PaperQuestionValidationRow};

    fn row(question_id: &str, category: &str, status: &str) -> PaperQuestionValidationRow {
        PaperQuestionValidationRow {
            question_id: question_id.to_string(),
            category: category.to_string(),
            status: status.to_string(),
        }
    }

    #[test]
    fn paper_question_validation_accepts_uniform_category_with_reviewed_or_used_status() {
        let rows = vec![row("q1", "T", "reviewed"), row("q2", "T", "used")];

        validate_paper_question_rows(&rows).expect("paper questions should validate");
    }

    #[test]
    fn paper_question_validation_rejects_none_category() {
        let err = validate_paper_question_rows(&[row("q1", "none", "reviewed")])
            .expect_err("none category should be rejected");

        assert!(err
            .message
            .contains("paper questions must all have category T or all have category E"));
    }

    #[test]
    fn paper_question_validation_rejects_mixed_categories() {
        let err =
            validate_paper_question_rows(&[row("q1", "T", "reviewed"), row("q2", "E", "used")])
                .expect_err("mixed categories should be rejected");

        assert!(err
            .message
            .contains("paper questions must all have the same category"));
    }

    #[test]
    fn paper_question_validation_rejects_none_status() {
        let err = validate_paper_question_rows(&[row("q1", "E", "none")])
            .expect_err("none status should be rejected");

        assert!(err
            .message
            .contains("paper questions must all have status reviewed or used"));
    }
}
