//! Axum handlers for read, import, paper assembly, export, and audit endpoints.

use std::{collections::HashSet, fs, path::Path};

use anyhow::{anyhow, Context};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use sqlx::{query, Row};

use super::{
    error::{ApiResult, HealthResponse},
    exports::{default_export_path, ensure_parent_dir, export_csv, export_jsonl, exported_path},
    imports::{
        commit_question_import as commit_question_import_service, validate_question_import_request,
    },
    models::{
        CreatePaperRequest, ExportFormat, ExportRequest, ExportResponse, PaperDetail,
        PaperWriteResponse, QualityCheckRequest, QuestionDetail, QuestionImportCommitResponse,
        QuestionImportRequest, QuestionImportValidationResponse, QuestionSummary, QuestionsParams,
        ReplacePaperQuestionsRequest, SearchParams,
    },
    quality::build_quality_report,
    queries::{
        execute_questions_query, map_paper_question_summary, map_paper_summary, map_question_asset,
        map_question_paper_ref, map_question_summary, validate_question_filters,
    },
    utils::{canonical_or_original, expand_path},
    AppState,
};

pub(crate) async fn health(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    if let Err(_err) = query("SELECT 1").execute(&state.pool).await {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    Ok(Json(HealthResponse {
        status: "ok",
        service: "qb_api_rust",
    }))
}

pub(crate) async fn list_papers(
    State(state): State<AppState>,
) -> Result<Json<Vec<super::PaperSummary>>, StatusCode> {
    let rows = query(
        r#"
        SELECT p.paper_id, p.edition, p.paper_type, p.title, p.notes,
               COUNT(pq.question_id) AS question_count
        FROM papers p
        LEFT JOIN paper_questions pq ON pq.paper_id = p.paper_id
        GROUP BY p.paper_id, p.edition, p.paper_type, p.title, p.notes
        ORDER BY p.edition, p.paper_id
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
    if !["regular", "semifinal", "final", "other"].contains(&request.paper_type.as_str()) {
        return Err(anyhow!("paper_type must be one of: regular, semifinal, final, other").into());
    }

    query(
        r#"
        INSERT INTO papers (paper_id, edition, paper_type, title, notes, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
        ON CONFLICT (paper_id)
        DO UPDATE SET
            edition = EXCLUDED.edition,
            paper_type = EXCLUDED.paper_type,
            title = EXCLUDED.title,
            notes = EXCLUDED.notes,
            updated_at = NOW()
        "#,
    )
    .bind(&request.paper_id)
    .bind(&request.edition)
    .bind(&request.paper_type)
    .bind(&request.title)
    .bind(request.notes.as_deref())
    .execute(&state.pool)
    .await
    .context("upsert paper failed")?;

    let count_row = query("SELECT COUNT(*) AS question_count FROM paper_questions WHERE paper_id = $1")
        .bind(&request.paper_id)
        .fetch_one(&state.pool)
        .await
        .context("query paper question count failed")?;

    Ok(Json(PaperWriteResponse {
        paper_id: request.paper_id,
        status: "saved",
        question_count: count_row.get::<i64, _>("question_count") as usize,
    }))
}

pub(crate) async fn replace_paper_questions(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(request): Json<ReplacePaperQuestionsRequest>,
) -> ApiResult<PaperWriteResponse> {
    let paper_exists = query("SELECT 1 FROM papers WHERE paper_id = $1")
        .bind(&paper_id)
        .fetch_optional(&state.pool)
        .await
        .context("check paper existence failed")?
        .is_some();
    if !paper_exists {
        return Err(anyhow!("paper not found: {paper_id}").into());
    }

    let mut seen_question_ids = HashSet::new();
    let mut seen_sort_orders = HashSet::new();
    for item in &request.question_refs {
        if !seen_question_ids.insert(item.question_id.clone()) {
            return Err(anyhow!("duplicate question_id in question_refs: {}", item.question_id).into());
        }
        if !seen_sort_orders.insert(item.sort_order) {
            return Err(anyhow!("duplicate sort_order in question_refs: {}", item.sort_order).into());
        }
    }

    let existing_questions = query("SELECT question_id FROM questions")
        .fetch_all(&state.pool)
        .await
        .context("load existing questions failed")?
        .into_iter()
        .map(|row| row.get::<String, _>("question_id"))
        .collect::<HashSet<_>>();

    for item in &request.question_refs {
        if !existing_questions.contains(&item.question_id) {
            return Err(anyhow!("unknown question_id in question_refs: {}", item.question_id).into());
        }
    }

    let mut tx = state.pool.begin().await.context("begin tx failed")?;
    query("DELETE FROM paper_questions WHERE paper_id = $1")
        .bind(&paper_id)
        .execute(&mut *tx)
        .await
        .context("delete old paper question refs failed")?;

    for item in &request.question_refs {
        query(
            r#"
            INSERT INTO paper_questions (paper_id, question_id, sort_order, question_label, created_at)
            VALUES ($1, $2, $3, $4, NOW())
            "#,
        )
        .bind(&paper_id)
        .bind(&item.question_id)
        .bind(item.sort_order)
        .bind(item.question_label.as_deref())
        .execute(&mut *tx)
        .await
        .with_context(|| {
            format!(
                "insert paper question ref failed: paper_id={}, question_id={}",
                paper_id, item.question_id
            )
        })?;
    }
    tx.commit().await.context("commit paper question refs failed")?;

    Ok(Json(PaperWriteResponse {
        paper_id,
        status: "saved",
        question_count: request.question_refs.len(),
    }))
}

pub(crate) async fn get_paper_detail(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<PaperDetail>, StatusCode> {
    let paper_row = query(
        r#"
        SELECT paper_id, edition, paper_type, title, notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM papers
        WHERE paper_id = $1
        "#,
    )
    .bind(&paper_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let question_rows = query(
        r#"
        SELECT q.question_id, pq.sort_order, pq.question_label, q.category, q.status, q.tags_json
        FROM paper_questions pq
        JOIN questions q ON q.question_id = pq.question_id
        WHERE pq.paper_id = $1
        ORDER BY pq.sort_order
        "#,
    )
    .bind(&paper_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(PaperDetail {
        paper_id: paper_row.get("paper_id"),
        edition: paper_row.get("edition"),
        paper_type: paper_row.get("paper_type"),
        title: paper_row.get("title"),
        notes: paper_row.get("notes"),
        created_at: paper_row.get("created_at"),
        updated_at: paper_row.get("updated_at"),
        questions: question_rows
            .into_iter()
            .map(map_paper_question_summary)
            .collect(),
    }))
}

pub(crate) async fn list_questions(
    Query(params): Query<QuestionsParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<QuestionSummary>>, StatusCode> {
    validate_question_filters(&params).map_err(|_| StatusCode::BAD_REQUEST)?;
    let plan = params.build_query();
    let rows = execute_questions_query(&state.pool, &params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.into_iter().map(map_question_summary).collect()))
}

pub(crate) async fn search_questions(
    Query(params): Query<SearchParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<QuestionSummary>>, StatusCode> {
    if params.q.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let list_params = QuestionsParams {
        edition: None,
        paper_id: None,
        paper_type: None,
        category: None,
        has_assets: None,
        has_answer: None,
        tag: None,
        q: Some(params.q),
        limit: params.limit,
        offset: params.offset,
    };
    let plan = list_params.build_query();
    let rows = execute_questions_query(&state.pool, &list_params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.into_iter().map(map_question_summary).collect()))
}

pub(crate) async fn get_question_detail(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<QuestionDetail>, StatusCode> {
    let row = query(
        r#"
        SELECT question_id, category,
               question_tex_object_id::text AS question_tex_object_id,
               answer_tex_object_id::text AS answer_tex_object_id,
               search_text, status, tags_json, notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        WHERE question_id = $1
        "#,
    )
    .bind(&question_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let assets = query(
        r#"
        SELECT asset_id, kind, object_id::text AS object_id, caption, sort_order
        FROM question_assets
        WHERE question_id = $1
        ORDER BY sort_order, asset_id
        "#,
    )
    .bind(&question_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(map_question_asset)
    .collect();

    let papers = query(
        r#"
        SELECT p.paper_id, p.edition, p.paper_type, p.title, pq.sort_order, pq.question_label
        FROM paper_questions pq
        JOIN papers p ON p.paper_id = pq.paper_id
        WHERE pq.question_id = $1
        ORDER BY p.paper_id, pq.sort_order
        "#,
    )
    .bind(&question_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(map_question_paper_ref)
    .collect();

    Ok(Json(QuestionDetail {
        question_id: row.get("question_id"),
        category: row.get("category"),
        question_tex_object_id: row.get("question_tex_object_id"),
        answer_tex_object_id: row.get("answer_tex_object_id"),
        search_text: row.get("search_text"),
        status: row.get("status"),
        tags: row.get("tags_json"),
        notes: row.get("notes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        assets,
        papers,
    }))
}

pub(crate) async fn validate_question_import(
    Json(request): Json<QuestionImportRequest>,
) -> ApiResult<QuestionImportValidationResponse> {
    Ok(Json(validate_question_import_request(request)?))
}

pub(crate) async fn commit_question_import(
    State(state): State<AppState>,
    Json(request): Json<QuestionImportRequest>,
) -> ApiResult<QuestionImportCommitResponse> {
    Ok(Json(
        commit_question_import_service(&state.pool, request).await?,
    ))
}

pub(crate) async fn run_export(
    State(state): State<AppState>,
    Json(request): Json<ExportRequest>,
) -> ApiResult<ExportResponse> {
    let output_path = request
        .output_path
        .as_deref()
        .map(expand_path)
        .unwrap_or_else(|| default_export_path(request.format, request.public));
    ensure_parent_dir(&output_path, "export")?;

    let include_answers = !request.public;
    let exported_count = match request.format {
        ExportFormat::Jsonl => export_jsonl(&state.pool, &output_path, include_answers).await?,
        ExportFormat::Csv => export_csv(&state.pool, &output_path, include_answers).await?,
    };

    Ok(Json(ExportResponse {
        format: match request.format {
            ExportFormat::Jsonl => "jsonl",
            ExportFormat::Csv => "csv",
        },
        public: request.public,
        output_path: exported_path(&output_path),
        exported_questions: exported_count,
    }))
}

pub(crate) async fn run_quality_check(
    State(state): State<AppState>,
    Json(request): Json<QualityCheckRequest>,
) -> ApiResult<serde_json::Value> {
    let output_path = request
        .output_path
        .as_deref()
        .map(expand_path)
        .unwrap_or_else(|| std::path::PathBuf::from("exports/quality_report.json"));

    let report = build_quality_report(&state.pool).await?;
    ensure_parent_dir(&output_path, "quality report")?;
    let serialized =
        serde_json::to_string_pretty(&report).context("serialize quality report failed")?;
    fs::write(&output_path, serialized).with_context(|| {
        format!(
            "write quality report failed: {}",
            output_path.to_string_lossy()
        )
    })?;

    Ok(Json(json!({
        "output_path": canonical_or_original(Path::new(&output_path)),
        "report": report,
    })))
}
