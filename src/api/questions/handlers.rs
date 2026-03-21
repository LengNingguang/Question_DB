use anyhow::{anyhow, Context};
use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},
    http::StatusCode,
    Json,
};
use sqlx::{query, Row};
use uuid::Uuid;

use super::{
    imports::{import_question_zip, MAX_UPLOAD_BYTES},
    models::{
        QuestionDeleteResponse, QuestionDetail, QuestionImportResponse, QuestionPaperRef,
        QuestionSummary, QuestionsParams, UpdateQuestionMetadataRequest,
    },
    queries::{
        execute_questions_query, load_question_algorithms, load_question_files, load_question_tags,
        map_question_detail, map_question_paper_ref, map_question_summary,
        validate_question_filters,
    },
};
use crate::api::{
    shared::error::{ApiError, ApiResult},
    AppState,
};

pub(crate) async fn list_questions(
    Query(params): Query<QuestionsParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<QuestionSummary>>, StatusCode> {
    validate_question_filters(&params).map_err(|_| StatusCode::BAD_REQUEST)?;
    let plan = params.build_query();
    let rows = execute_questions_query(&state.pool, &params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut questions = Vec::with_capacity(rows.len());
    for row in rows {
        let question_id: String = row.get("question_id");
        let tags = load_question_tags(&state.pool, &question_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let algorithms = load_question_algorithms(&state.pool, &question_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        questions.push(map_question_summary(row, tags, algorithms));
    }

    Ok(Json(questions))
}

pub(crate) async fn get_question_detail(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<QuestionDetail>, StatusCode> {
    Uuid::parse_str(&question_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    fetch_question_detail(&state, &question_id)
        .await
        .map(Json)
        .map_err(map_question_detail_error)
}

pub(crate) async fn create_question(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<QuestionImportResponse> {
    let mut file_name = None;
    let mut bytes = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("read multipart field failed: {err}")))?
    {
        let Some(name) = field.name() else {
            continue;
        };
        if name != "file" {
            continue;
        }

        file_name = field.file_name().map(str::to_string);
        bytes = field
            .bytes()
            .await
            .map_err(|err| ApiError::bad_request(format!("read uploaded file failed: {err}")))?
            .to_vec();
        break;
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
        import_question_zip(&state.pool, file_name.as_deref(), bytes)
            .await
            .map_err(ApiError::from)?,
    ))
}

pub(crate) async fn update_question_metadata(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateQuestionMetadataRequest>,
) -> ApiResult<QuestionDetail> {
    Uuid::parse_str(&question_id)
        .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;
    let update = request
        .normalize()
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin question metadata update tx failed")?;

    let exists = query("SELECT 1 FROM questions WHERE question_id = $1::uuid")
        .bind(&question_id)
        .fetch_optional(&mut *tx)
        .await
        .context("check question existence failed")?
        .is_some();
    if !exists {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("question not found: {question_id}"),
        });
    }

    if let Some(category) = &update.category {
        query(
            "UPDATE questions SET category = $2, updated_at = NOW() WHERE question_id = $1::uuid",
        )
        .bind(&question_id)
        .bind(category)
        .execute(&mut *tx)
        .await
        .context("update question category failed")?;
    }

    if let Some(notes) = &update.notes {
        query("UPDATE questions SET notes = $2, updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .bind(notes)
            .execute(&mut *tx)
            .await
            .context("update question notes failed")?;
    }

    if let Some(status) = &update.status {
        query("UPDATE questions SET status = $2, updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .bind(status)
            .execute(&mut *tx)
            .await
            .context("update question status failed")?;
    }

    if let Some(difficulty) = &update.difficulty {
        if difficulty.clear_all {
            query(
                "UPDATE questions SET difficulty_human = NULL, difficulty_notes = NULL, updated_at = NOW() WHERE question_id = $1::uuid",
            )
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("clear question difficulty failed")?;
            query("DELETE FROM question_difficulty_algorithms WHERE question_id = $1::uuid")
                .bind(&question_id)
                .execute(&mut *tx)
                .await
                .context("clear difficulty algorithms failed")?;
        } else {
            if let Some(human) = difficulty.human {
                query(
                    "UPDATE questions SET difficulty_human = $2, updated_at = NOW() WHERE question_id = $1::uuid",
                )
                .bind(&question_id)
                .bind(human)
                .execute(&mut *tx)
                .await
                .context("update difficulty human failed")?;
            }

            if let Some(notes) = &difficulty.notes {
                query(
                    "UPDATE questions SET difficulty_notes = $2, updated_at = NOW() WHERE question_id = $1::uuid",
                )
                .bind(&question_id)
                .bind(notes.as_deref())
                .execute(&mut *tx)
                .await
                .context("update difficulty notes failed")?;
            }

            if let Some(algorithm_scores) = &difficulty.algorithm {
                query("DELETE FROM question_difficulty_algorithms WHERE question_id = $1::uuid")
                    .bind(&question_id)
                    .execute(&mut *tx)
                    .await
                    .context("replace difficulty algorithms failed")?;

                for (algorithm_tag, score) in algorithm_scores {
                    query(
                        "INSERT INTO question_difficulty_algorithms (question_id, algorithm_tag, score) VALUES ($1::uuid, $2, $3)",
                    )
                    .bind(&question_id)
                    .bind(algorithm_tag)
                    .bind(*score)
                    .execute(&mut *tx)
                    .await
                    .with_context(|| {
                        format!("insert updated algorithm score failed: {algorithm_tag}")
                    })?;
                }
            }
        }

        query("UPDATE questions SET updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("touch question updated_at after difficulty update failed")?;
    }

    if let Some(tags) = &update.tags {
        query("DELETE FROM question_tags WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("replace question tags failed")?;

        for (idx, tag) in tags.iter().enumerate() {
            query("INSERT INTO question_tags (question_id, tag, sort_order) VALUES ($1::uuid, $2, $3)")
                .bind(&question_id)
                .bind(tag)
                .bind(i32::try_from(idx).unwrap_or(i32::MAX))
                .execute(&mut *tx)
                .await
                .with_context(|| format!("insert updated question tag failed: {tag}"))?;
        }

        query("UPDATE questions SET updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("touch question updated_at after tag update failed")?;
    }

    tx.commit()
        .await
        .context("commit question metadata update failed")?;

    fetch_question_detail(&state, &question_id)
        .await
        .map(Json)
        .map_err(|err| ApiError {
            status: map_question_detail_error(err),
            message: "load updated question detail failed".to_string(),
        })
}

pub(crate) async fn delete_question(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
) -> ApiResult<QuestionDeleteResponse> {
    Uuid::parse_str(&question_id)
        .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;

    let result = query("DELETE FROM questions WHERE question_id = $1::uuid")
        .bind(&question_id)
        .execute(&state.pool)
        .await
        .context("delete question failed")?;

    if result.rows_affected() == 0 {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("question not found: {question_id}"),
        });
    }

    Ok(Json(QuestionDeleteResponse {
        question_id,
        status: "deleted",
    }))
}

async fn fetch_question_detail(
    state: &AppState,
    question_id: &str,
) -> Result<QuestionDetail, anyhow::Error> {
    let row = query(
        r#"
        SELECT question_id::text AS question_id, source_tex_path, category, status,
               COALESCE(notes, '') AS notes, difficulty_human, difficulty_notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        WHERE question_id = $1::uuid
        "#,
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await
    .context("load question detail failed")?
    .ok_or_else(|| anyhow!("question not found: {question_id}"))?;

    let tex_files = load_question_files(&state.pool, question_id, "tex")
        .await
        .context("load question tex files failed")?;
    let tex_object_id = tex_files
        .first()
        .map(|file| file.object_id.clone())
        .ok_or_else(|| anyhow!("question is missing a tex object: {question_id}"))?;
    let assets = load_question_files(&state.pool, question_id, "asset")
        .await
        .context("load question assets failed")?;
    let tags = load_question_tags(&state.pool, question_id)
        .await
        .context("load question tags failed")?;
    let algorithms = load_question_algorithms(&state.pool, question_id)
        .await
        .context("load question algorithms failed")?;

    let papers = query(
        r#"
        SELECT p.paper_id::text AS paper_id, p.edition, p.paper_type, p.title, pq.sort_order
        FROM paper_questions pq
        JOIN papers p ON p.paper_id = pq.paper_id
        WHERE pq.question_id = $1::uuid
        ORDER BY p.created_at DESC, pq.sort_order
        "#,
    )
    .bind(question_id)
    .fetch_all(&state.pool)
    .await
    .context("load question papers failed")?
    .into_iter()
    .map(map_question_paper_ref)
    .collect::<Vec<QuestionPaperRef>>();

    Ok(map_question_detail(
        row,
        tex_object_id,
        tags,
        algorithms,
        assets,
        papers,
    ))
}

fn map_question_detail_error(err: anyhow::Error) -> StatusCode {
    if err.to_string().starts_with("question not found:") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}
