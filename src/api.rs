use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env, fs,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use csv::{ReaderBuilder, StringRecord, WriterBuilder};
use mime_guess::MimeGuess;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder, Row, Transaction};
use strsim::normalized_levenshtein;
use uuid::Uuid;
use zip::ZipArchive;

static LATEX_CMD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\[A-Za-z@]+").expect("valid regex"));
static SPECIAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[{}\[\]$^_&%#~]").expect("valid regex"));
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("valid regex"));

const XLSX_MIME_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const OBJECT_BUCKET: &str = "local";
const SIMILARITY_THRESHOLD: f64 = 0.92;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

type ApiResult<T> = Result<Json<T>, ApiError>;

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let payload = Json(json!({ "error": self.message }));
        (self.status, payload).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err.to_string())
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PaperSummary {
    paper_id: String,
    edition: String,
    paper_type: String,
    title: String,
    paper_tex_object_id: Option<String>,
    source_pdf_object_id: Option<String>,
    question_index: Value,
    notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaperQuestionSummary {
    question_id: String,
    paper_index: i32,
    question_no: Option<String>,
    category: String,
    question_tex_object_id: Option<String>,
    answer_tex_object_id: Option<String>,
    status: String,
    tags: Value,
}

#[derive(Debug, Serialize)]
pub struct PaperDetail {
    paper_id: String,
    edition: String,
    paper_type: String,
    title: String,
    paper_tex_object_id: Option<String>,
    source_pdf_object_id: Option<String>,
    question_index: Value,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
    questions: Vec<PaperQuestionSummary>,
}

#[derive(Debug, Serialize)]
pub struct QuestionSummary {
    question_id: String,
    paper_id: String,
    paper_index: i32,
    question_no: Option<String>,
    category: String,
    status: String,
    search_text: Option<String>,
    question_tex_object_id: Option<String>,
    answer_tex_object_id: Option<String>,
    tags: Value,
    edition: String,
    paper_type: String,
    title: String,
}

#[derive(Debug, Serialize)]
pub struct QuestionAsset {
    asset_id: String,
    kind: String,
    object_id: String,
    caption: Option<String>,
    sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct QuestionStat {
    exam_session: String,
    source_workbook_id: Option<String>,
    participant_count: i32,
    avg_score: f64,
    score_std: f64,
    full_mark_rate: f64,
    zero_score_rate: f64,
    max_score: f64,
    min_score: f64,
    stats_source: String,
    stats_version: String,
}

#[derive(Debug, Serialize)]
pub struct DifficultyScore {
    exam_session: Option<String>,
    manual_level: Option<String>,
    derived_score: Option<f64>,
    method: String,
    method_version: String,
    confidence: Option<f64>,
    feature_json: Value,
}

#[derive(Debug, Serialize)]
pub struct ScoreWorkbookSummary {
    workbook_id: String,
    paper_id: String,
    exam_session: String,
    workbook_kind: String,
    workbook_object_id: String,
    source_filename: String,
    mime_type: Option<String>,
    sheet_names: Value,
    file_size: i64,
    sha256: String,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct QuestionDetail {
    question_id: String,
    paper_id: String,
    paper_index: i32,
    question_no: Option<String>,
    category: String,
    question_tex_object_id: Option<String>,
    answer_tex_object_id: Option<String>,
    search_text: Option<String>,
    status: String,
    tags: Value,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
    edition: String,
    paper_type: String,
    paper_title: String,
    paper_tex_object_id: Option<String>,
    source_pdf_object_id: Option<String>,
    paper_question_index: Value,
    assets: Vec<QuestionAsset>,
    stats: Vec<QuestionStat>,
    difficulty_scores: Vec<DifficultyScore>,
    score_workbooks: Vec<ScoreWorkbookSummary>,
}

#[derive(Debug, Deserialize)]
pub struct QuestionsParams {
    edition: Option<String>,
    paper_id: Option<String>,
    paper_type: Option<String>,
    category: Option<String>,
    has_assets: Option<bool>,
    has_answer: Option<bool>,
    min_avg_score: Option<f64>,
    max_avg_score: Option<f64>,
    tag: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: String,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ScoreWorkbookParams {
    paper_id: Option<String>,
    exam_session: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundleImportRequest {
    bundle_path: String,
    #[serde(default)]
    allow_similar: bool,
}

#[derive(Debug, Deserialize)]
struct WorkbookImportRequest {
    workbook_path: String,
    paper_id: String,
    exam_session: String,
    workbook_kind: String,
    workbook_id: String,
    #[serde(default)]
    notes: String,
}

#[derive(Debug, Deserialize)]
struct StatsImportRequest {
    csv_path: String,
    stats_source: String,
    stats_version: String,
    source_workbook_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExportRequest {
    format: ExportFormat,
    #[serde(default)]
    public: bool,
    output_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum ExportFormat {
    Jsonl,
    Csv,
}

#[derive(Debug, Deserialize)]
struct QualityCheckRequest {
    output_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DifficultyRunRequest {
    method_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct BundleValidationResponse {
    bundle_path: String,
    ok: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BundleCommitResponse {
    bundle_name: Option<String>,
    paper_id: Option<String>,
    status: String,
    question_count: usize,
    imported_questions: usize,
    imported_assets: usize,
    imported_workbooks: usize,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatsImportResponse {
    csv_path: String,
    imported_stats: usize,
    stats_source: String,
    stats_version: String,
    source_workbook_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExportResponse {
    format: &'static str,
    public: bool,
    output_path: String,
    exported_questions: usize,
}

#[derive(Debug, Serialize)]
struct DifficultyRunResponse {
    method_version: String,
    updated_count: usize,
}

#[derive(Debug)]
struct QuestionsQuery {
    sql: String,
    bind_count: usize,
    limit: i64,
    offset: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum EditionValue {
    Int(i64),
    Str(String),
}

impl EditionValue {
    fn as_string(&self) -> String {
        match self {
            EditionValue::Int(v) => v.to_string(),
            EditionValue::Str(v) => v.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
struct BundleManifest {
    bundle_name: String,
    run_label: String,
    paper: ManifestPaper,
    #[serde(default)]
    score_workbooks: Vec<ManifestWorkbook>,
}

#[derive(Debug, Deserialize, Clone)]
struct ManifestPaper {
    paper_id: String,
    edition: EditionValue,
    paper_type: String,
    title: String,
    paper_latex_path: String,
    source_pdf_path: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ManifestWorkbook {
    workbook_id: String,
    exam_session: String,
    workbook_kind: String,
    file_path: String,
    notes: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct BundleQuestion {
    question_id: String,
    question_no: String,
    paper_index: i32,
    category: String,
    latex_path: String,
    answer_latex_path: Option<String>,
    #[serde(rename = "latex_anchor")]
    _latex_anchor: Option<String>,
    search_text: Option<String>,
    status: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    assets: Vec<BundleAsset>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct BundleAsset {
    asset_id: String,
    kind: String,
    file_path: String,
    sha256: Option<String>,
    caption: Option<String>,
    sort_order: Option<i32>,
}

#[derive(Debug, Clone)]
struct HydratedQuestion {
    question: BundleQuestion,
    latex_source: String,
    answer_source: Option<String>,
    comparison_text: String,
}

#[derive(Debug)]
struct LoadedBundle {
    manifest: BundleManifest,
    questions: Vec<BundleQuestion>,
}

#[derive(Debug, Default)]
struct ValidationResult {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ValidationResult {
    fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, Clone)]
struct AggregatedScoreRow {
    question_id: String,
    exam_session: String,
    participant_count: i32,
    avg_score: f64,
    score_std: f64,
    full_mark_rate: f64,
    zero_score_rate: f64,
    max_score: f64,
    min_score: f64,
}

#[derive(Debug, Serialize)]
struct QualityReport {
    missing_question_tex_object: Vec<String>,
    missing_question_tex_source: Vec<String>,
    missing_answer_tex_object: Vec<String>,
    missing_answer_tex_source: Vec<String>,
    missing_paper_tex_object: Vec<String>,
    missing_paper_tex_source: Vec<String>,
    missing_assets_object: Vec<Value>,
    missing_workbook_blob: Vec<String>,
    duplicate_question_numbers: Vec<Value>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/papers", get(list_papers))
        .route("/papers/:paper_id", get(get_paper_detail))
        .route("/questions", get(list_questions))
        .route("/questions/:question_id", get(get_question_detail))
        .route("/search", get(search_questions))
        .route("/score-workbooks", get(list_score_workbooks))
        .route("/score-workbooks/:workbook_id", get(get_score_workbook_detail))
        .route(
            "/score-workbooks/:workbook_id/download",
            get(download_score_workbook),
        )
        .route("/imports/bundle/validate", post(validate_bundle_import))
        .route("/imports/bundle/commit", post(commit_bundle_import))
        .route("/imports/workbooks/commit", post(commit_workbook_import))
        .route("/imports/stats/commit", post(commit_stats_import))
        .route("/difficulty-scores/run", post(run_difficulty_scores))
        .route("/exports/run", post(run_export))
        .route("/quality-checks/run", post(run_quality_check))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    if let Err(_err) = query("SELECT 1").execute(&state.pool).await {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    Ok(Json(HealthResponse {
        status: "ok",
        service: "qb_api_rust",
    }))
}

async fn list_papers(State(state): State<AppState>) -> Result<Json<Vec<PaperSummary>>, StatusCode> {
    let rows = query(
        r#"
        SELECT paper_id, edition, paper_type, title,
               paper_tex_object_id::text AS paper_tex_object_id,
               source_pdf_object_id::text AS source_pdf_object_id,
               question_index_json, notes
        FROM papers
        ORDER BY edition, paper_id
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let payload = rows.into_iter().map(map_paper_summary).collect();
    Ok(Json(payload))
}

async fn get_paper_detail(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<PaperDetail>, StatusCode> {
    let paper_row = query(
        r#"
        SELECT paper_id, edition, paper_type, title,
               paper_tex_object_id::text AS paper_tex_object_id,
               source_pdf_object_id::text AS source_pdf_object_id,
               question_index_json, notes,
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
        SELECT question_id, paper_index, question_no, category,
               question_tex_object_id::text AS question_tex_object_id,
               answer_tex_object_id::text AS answer_tex_object_id,
               status, tags_json
        FROM questions
        WHERE paper_id = $1
        ORDER BY paper_index
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
        paper_tex_object_id: paper_row.get("paper_tex_object_id"),
        source_pdf_object_id: paper_row.get("source_pdf_object_id"),
        question_index: paper_row.get::<Value, _>("question_index_json"),
        notes: paper_row.get("notes"),
        created_at: paper_row.get("created_at"),
        updated_at: paper_row.get("updated_at"),
        questions: question_rows
            .into_iter()
            .map(map_paper_question_summary)
            .collect(),
    }))
}

async fn list_questions(
    Query(params): Query<QuestionsParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<QuestionSummary>>, StatusCode> {
    validate_question_filters(&params).map_err(|_| StatusCode::BAD_REQUEST)?;
    let plan = params.build_query();
    let rows = execute_questions_query(&state.pool, &params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload = rows.into_iter().map(map_question_summary).collect();
    Ok(Json(payload))
}

async fn search_questions(
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
        min_avg_score: None,
        max_avg_score: None,
        tag: None,
        q: Some(params.q),
        limit: params.limit,
        offset: params.offset,
    };
    let plan = list_params.build_query();
    let rows = execute_questions_query(&state.pool, &list_params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload = rows.into_iter().map(map_question_summary).collect();
    Ok(Json(payload))
}

async fn get_question_detail(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<QuestionDetail>, StatusCode> {
    let row = query(
        r#"
        SELECT q.question_id, q.paper_id, q.paper_index, q.question_no, q.category,
               q.question_tex_object_id::text AS question_tex_object_id,
               q.answer_tex_object_id::text AS answer_tex_object_id,
               q.search_text, q.status, q.tags_json, q.notes,
               to_char(q.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(q.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at,
               p.edition, p.paper_type, p.title AS paper_title,
               p.paper_tex_object_id::text AS paper_tex_object_id,
               p.source_pdf_object_id::text AS source_pdf_object_id,
               p.question_index_json
        FROM questions q
        JOIN papers p ON p.paper_id = q.paper_id
        WHERE q.question_id = $1
        "#,
    )
    .bind(&question_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let paper_id: String = row.get("paper_id");

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

    let stats = query(
        r#"
        SELECT exam_session, source_workbook_id, participant_count, avg_score, score_std,
               full_mark_rate, zero_score_rate, max_score, min_score, stats_source, stats_version
        FROM question_stats
        WHERE question_id = $1
        ORDER BY exam_session
        "#,
    )
    .bind(&question_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(map_question_stat)
    .collect();

    let difficulty_scores = query(
        r#"
        SELECT exam_session, manual_level, derived_score, method, method_version,
               confidence, feature_json
        FROM difficulty_scores
        WHERE question_id = $1
        ORDER BY exam_session NULLS FIRST, method, method_version
        "#,
    )
    .bind(&question_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(map_difficulty_score)
    .collect();

    let score_workbooks = query(
        r#"
        SELECT workbook_id, paper_id, exam_session, workbook_kind,
               workbook_object_id::text AS workbook_object_id, source_filename,
               mime_type, sheet_names_json, file_size, sha256, notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM score_workbooks
        WHERE paper_id = $1
        ORDER BY exam_session, workbook_id
        "#,
    )
    .bind(&paper_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(map_score_workbook_summary)
    .collect();

    Ok(Json(QuestionDetail {
        question_id: row.get("question_id"),
        paper_id,
        paper_index: row.get("paper_index"),
        question_no: row.get("question_no"),
        category: row.get("category"),
        question_tex_object_id: row.get("question_tex_object_id"),
        answer_tex_object_id: row.get("answer_tex_object_id"),
        search_text: row.get("search_text"),
        status: row.get("status"),
        tags: row.get::<Value, _>("tags_json"),
        notes: row.get("notes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        paper_title: row.get("paper_title"),
        paper_tex_object_id: row.get("paper_tex_object_id"),
        source_pdf_object_id: row.get("source_pdf_object_id"),
        paper_question_index: row.get::<Value, _>("question_index_json"),
        assets,
        stats,
        difficulty_scores,
        score_workbooks,
    }))
}

async fn list_score_workbooks(
    Query(params): Query<ScoreWorkbookParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ScoreWorkbookSummary>>, StatusCode> {
    let (sql, bind_count) = params.build_query();
    let rows = execute_score_workbooks_query(&state.pool, &params, &sql, bind_count)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload = rows.into_iter().map(map_score_workbook_summary).collect();
    Ok(Json(payload))
}

async fn get_score_workbook_detail(
    AxumPath(workbook_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<ScoreWorkbookSummary>, StatusCode> {
    let row = query(
        r#"
        SELECT workbook_id, paper_id, exam_session, workbook_kind,
               workbook_object_id::text AS workbook_object_id, source_filename,
               mime_type, sheet_names_json, file_size, sha256, notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM score_workbooks
        WHERE workbook_id = $1
        "#,
    )
    .bind(&workbook_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(map_score_workbook_summary(row)))
}

async fn download_score_workbook(
    AxumPath(workbook_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let row = query(
        r#"
        SELECT sw.source_filename,
               COALESCE(sw.mime_type, o.mime_type, 'application/octet-stream') AS mime_type,
               ob.content
        FROM score_workbooks sw
        JOIN objects o ON o.object_id = sw.workbook_object_id
        JOIN object_blobs ob ON ob.object_id = sw.workbook_object_id
        WHERE sw.workbook_id = $1
        "#,
    )
    .bind(&workbook_id)
    .fetch_optional(&state.pool)
    .await
    .context("query workbook download payload failed")?
    .ok_or_else(|| ApiError::not_found("Workbook not found"))?;

    let source_filename: String = row.get("source_filename");
    let mime_type: String = row.get("mime_type");
    let content: Vec<u8> = row.get("content");

    let disposition = format!("attachment; filename=\"{source_filename}\"");
    let mut response = Response::new(axum::body::Body::from(content));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

async fn validate_bundle_import(Json(request): Json<BundleImportRequest>) -> ApiResult<BundleValidationResponse> {
    let bundle_path = expand_path(&request.bundle_path);
    let (validation, _) = inspect_bundle(&bundle_path)?;
    Ok(Json(BundleValidationResponse {
        bundle_path: canonical_or_original(&bundle_path),
        ok: validation.ok(),
        warnings: validation.warnings,
        errors: validation.errors,
    }))
}

async fn commit_bundle_import(
    State(state): State<AppState>,
    Json(request): Json<BundleImportRequest>,
) -> ApiResult<BundleCommitResponse> {
    let bundle_path = expand_path(&request.bundle_path);
    let (mut validation, loaded_opt) = inspect_bundle(&bundle_path)?;
    let run_label_override = loaded_opt
        .as_ref()
        .map(|v| v.manifest.run_label.clone());

    let mut imported_questions = 0usize;
    let mut imported_assets = 0usize;
    let mut imported_workbooks = 0usize;

    let Some(loaded) = loaded_opt else {
        let response = BundleCommitResponse {
            bundle_name: None,
            paper_id: None,
            status: "failed".to_string(),
            question_count: 0,
            imported_questions,
            imported_assets,
            imported_workbooks,
            warnings: validation.warnings.clone(),
            errors: validation.errors.clone(),
        };
        insert_import_run(
            &state.pool,
            bundle_path.as_path(),
            None,
            false,
            &response.status,
            0,
            &response.warnings,
            &response.errors,
            None,
            None,
        )
        .await?;
        return Ok(Json(response));
    };

    let paper_id = Some(loaded.manifest.paper.paper_id.clone());
    let bundle_name = Some(loaded.manifest.bundle_name.clone());
    let question_count = loaded.questions.len();

    let hydrated_questions = hydrate_bundle_questions(&bundle_path, &loaded.questions)?;
    let (similarity_warnings, similarity_errors) =
        find_similarity_issues(&state.pool, &loaded.manifest.paper.paper_id, &hydrated_questions, request.allow_similar)
            .await?;
    validation.warnings.extend(similarity_warnings);
    validation.errors.extend(similarity_errors);

    let conflict_errors = detect_question_no_conflicts(
        &state.pool,
        &loaded.manifest.paper.paper_id,
        &loaded.questions,
    )
    .await?;
    validation.errors.extend(conflict_errors);

    let status = if validation.errors.is_empty() {
        "committed"
    } else {
        "failed"
    }
    .to_string();

    if validation.errors.is_empty() {
        let mut tx = state.pool.begin().await.context("begin tx failed")?;

        let paper_tex_path = join_bundle_path(&bundle_path, &loaded.manifest.paper.paper_latex_path);
        let paper_tex_bytes = fs::read(&paper_tex_path).with_context(|| {
            format!(
                "read paper tex failed: {}",
                paper_tex_path.to_string_lossy()
            )
        })?;
        let paper_tex_object_id = upsert_object_tx(
            &mut tx,
            "paper_tex",
            &paper_tex_path,
            &paper_tex_bytes,
            Some("text/x-tex"),
            "bundle_import",
        )
        .await?;

        let source_pdf_object_id = if let Some(path) = &loaded.manifest.paper.source_pdf_path {
            let source_pdf_path = join_bundle_path(&bundle_path, path);
            if source_pdf_path.exists() {
                let bytes = fs::read(&source_pdf_path).with_context(|| {
                    format!(
                        "read source pdf failed: {}",
                        source_pdf_path.to_string_lossy()
                    )
                })?;
                Some(
                    upsert_object_tx(
                        &mut tx,
                        "paper_pdf",
                        &source_pdf_path,
                        &bytes,
                        Some("application/pdf"),
                        "bundle_import",
                    )
                    .await?,
                )
            } else {
                None
            }
        } else {
            None
        };

        let question_index = Value::Array(
            loaded
                .questions
                .iter()
                .map(|q| {
                    json!({
                        "paper_index": q.paper_index,
                        "question_id": q.question_id,
                        "question_no": q.question_no,
                        "latex_path": q.latex_path,
                    })
                })
                .collect(),
        );

        query(
            r#"
            INSERT INTO papers (
                paper_id, edition, paper_type, title, paper_tex_object_id,
                source_pdf_object_id, question_index_json, notes, created_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5::uuid,
                $6::uuid, $7, $8, NOW(), NOW()
            )
            ON CONFLICT (paper_id)
            DO UPDATE SET
                edition = EXCLUDED.edition,
                paper_type = EXCLUDED.paper_type,
                title = EXCLUDED.title,
                paper_tex_object_id = EXCLUDED.paper_tex_object_id,
                source_pdf_object_id = EXCLUDED.source_pdf_object_id,
                question_index_json = EXCLUDED.question_index_json,
                notes = EXCLUDED.notes,
                updated_at = NOW()
            "#,
        )
        .bind(&loaded.manifest.paper.paper_id)
        .bind(loaded.manifest.paper.edition.as_string())
        .bind(&loaded.manifest.paper.paper_type)
        .bind(&loaded.manifest.paper.title)
        .bind(&paper_tex_object_id)
        .bind(source_pdf_object_id.as_deref())
        .bind(question_index)
        .bind(loaded.manifest.paper.notes.as_deref())
        .execute(&mut *tx)
        .await
        .context("upsert paper failed")?;

        for hydrated in &hydrated_questions {
            let latex_path = join_bundle_path(&bundle_path, &hydrated.question.latex_path);
            let question_tex_object_id = upsert_object_tx(
                &mut tx,
                "question_tex",
                &latex_path,
                hydrated.latex_source.as_bytes(),
                Some("text/x-tex"),
                "bundle_import",
            )
            .await?;

            let answer_tex_object_id = if let Some(answer_path_raw) = &hydrated.question.answer_latex_path {
                let answer_path = join_bundle_path(&bundle_path, answer_path_raw);
                if let Some(source) = &hydrated.answer_source {
                    Some(
                        upsert_object_tx(
                            &mut tx,
                            "answer_tex",
                            &answer_path,
                            source.as_bytes(),
                            Some("text/x-tex"),
                            "bundle_import",
                        )
                        .await?,
                    )
                } else {
                    None
                }
            } else {
                None
            };

            let search_text = hydrated
                .question
                .search_text
                .clone()
                .unwrap_or_else(|| hydrated.comparison_text.clone());

            query(
                r#"
                INSERT INTO questions (
                    question_id, paper_id, paper_index, question_no, category,
                    question_tex_object_id, answer_tex_object_id, search_text,
                    status, tags_json, notes, created_at, updated_at
                )
                VALUES (
                    $1, $2, $3, $4, $5,
                    $6::uuid, $7::uuid, $8,
                    $9, $10, $11, NOW(), NOW()
                )
                ON CONFLICT (question_id)
                DO UPDATE SET
                    paper_id = EXCLUDED.paper_id,
                    paper_index = EXCLUDED.paper_index,
                    question_no = EXCLUDED.question_no,
                    category = EXCLUDED.category,
                    question_tex_object_id = EXCLUDED.question_tex_object_id,
                    answer_tex_object_id = EXCLUDED.answer_tex_object_id,
                    search_text = EXCLUDED.search_text,
                    status = EXCLUDED.status,
                    tags_json = EXCLUDED.tags_json,
                    notes = EXCLUDED.notes,
                    updated_at = NOW()
                "#,
            )
            .bind(&hydrated.question.question_id)
            .bind(&loaded.manifest.paper.paper_id)
            .bind(hydrated.question.paper_index)
            .bind(&hydrated.question.question_no)
            .bind(&hydrated.question.category)
            .bind(&question_tex_object_id)
            .bind(answer_tex_object_id.as_deref())
            .bind(search_text)
            .bind(&hydrated.question.status)
            .bind(Value::Array(
                hydrated
                    .question
                    .tags
                    .iter()
                    .map(|v| Value::String(v.clone()))
                    .collect(),
            ))
            .bind(hydrated.question.notes.as_deref())
            .execute(&mut *tx)
            .await
            .with_context(|| format!("upsert question failed: {}", hydrated.question.question_id))?;
            imported_questions += 1;

            query("DELETE FROM question_assets WHERE question_id = $1")
                .bind(&hydrated.question.question_id)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!(
                        "delete existing assets failed: {}",
                        hydrated.question.question_id
                    )
                })?;

            for asset in &hydrated.question.assets {
                let asset_path = join_bundle_path(&bundle_path, &asset.file_path);
                let bytes = fs::read(&asset_path).with_context(|| {
                    format!("read asset failed: {}", asset_path.to_string_lossy())
                })?;
                let mime = MimeGuess::from_path(&asset_path)
                    .first_raw()
                    .map(|item| item.to_string())
                    .unwrap_or_else(|| "application/octet-stream".to_string());

                let object_id = upsert_object_tx(
                    &mut tx,
                    "question_asset",
                    &asset_path,
                    &bytes,
                    Some(&mime),
                    "bundle_import",
                )
                .await?;

                query(
                    r#"
                    INSERT INTO question_assets (
                        asset_id, question_id, kind, object_id, caption, sort_order, created_at
                    ) VALUES ($1, $2, $3, $4::uuid, $5, $6, NOW())
                    ON CONFLICT (asset_id)
                    DO UPDATE SET
                        question_id = EXCLUDED.question_id,
                        kind = EXCLUDED.kind,
                        object_id = EXCLUDED.object_id,
                        caption = EXCLUDED.caption,
                        sort_order = EXCLUDED.sort_order
                    "#,
                )
                .bind(&asset.asset_id)
                .bind(&hydrated.question.question_id)
                .bind(&asset.kind)
                .bind(&object_id)
                .bind(asset.caption.as_deref())
                .bind(asset.sort_order.unwrap_or(0))
                .execute(&mut *tx)
                .await
                .with_context(|| format!("insert asset failed: {}", asset.asset_id))?;
                imported_assets += 1;
            }
        }

        for workbook in &loaded.manifest.score_workbooks {
            upsert_workbook_tx(
                &mut tx,
                &loaded.manifest.paper.paper_id,
                workbook,
                &bundle_path,
            )
            .await?;
            imported_workbooks += 1;
        }

        tx.commit().await.context("commit bundle import failed")?;
    }

    let response = BundleCommitResponse {
        bundle_name,
        paper_id,
        status,
        question_count,
        imported_questions,
        imported_assets,
        imported_workbooks,
        warnings: validation.warnings.clone(),
        errors: validation.errors.clone(),
    };

    insert_import_run(
        &state.pool,
        bundle_path.as_path(),
        response.bundle_name.as_deref(),
        false,
        &response.status,
        response.question_count + response.imported_workbooks,
        &response.warnings,
        &response.errors,
        response.paper_id.as_deref(),
        run_label_override.as_deref(),
    )
    .await?;

    Ok(Json(response))
}

async fn commit_workbook_import(
    State(state): State<AppState>,
    Json(request): Json<WorkbookImportRequest>,
) -> ApiResult<ScoreWorkbookSummary> {
    let workbook_path = expand_path(&request.workbook_path);
    if !workbook_path.exists() {
        return Err(ApiError::bad_request(format!(
            "Workbook path does not exist: {}",
            workbook_path.to_string_lossy()
        )));
    }

    let paper_exists = query("SELECT 1 FROM papers WHERE paper_id = $1")
        .bind(&request.paper_id)
        .fetch_optional(&state.pool)
        .await
        .context("check paper existence failed")?
        .is_some();
    if !paper_exists {
        return Err(ApiError::not_found(format!(
            "Paper not found: {}",
            request.paper_id
        )));
    }

    let workbook_manifest = ManifestWorkbook {
        workbook_id: request.workbook_id,
        exam_session: request.exam_session,
        workbook_kind: request.workbook_kind,
        file_path: workbook_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| ApiError::bad_request("Invalid workbook filename"))?,
        notes: if request.notes.trim().is_empty() {
            None
        } else {
            Some(request.notes)
        },
    };

    let bundle_path = workbook_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| ApiError::bad_request("Workbook path has no parent directory"))?;

    let mut tx = state.pool.begin().await.context("begin tx failed")?;
    upsert_workbook_tx(&mut tx, &request.paper_id, &workbook_manifest, &bundle_path).await?;
    tx.commit().await.context("commit workbook import failed")?;

    let row = query(
        r#"
        SELECT workbook_id, paper_id, exam_session, workbook_kind,
               workbook_object_id::text AS workbook_object_id, source_filename,
               mime_type, sheet_names_json, file_size, sha256, notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM score_workbooks
        WHERE workbook_id = $1
        "#,
    )
    .bind(&workbook_manifest.workbook_id)
    .fetch_one(&state.pool)
    .await
    .context("query imported workbook failed")?;

    Ok(Json(map_score_workbook_summary(row)))
}

async fn commit_stats_import(
    State(state): State<AppState>,
    Json(request): Json<StatsImportRequest>,
) -> ApiResult<StatsImportResponse> {
    let csv_path = expand_path(&request.csv_path);
    let rows = aggregate_score_rows(&csv_path)?;

    let mut tx = state.pool.begin().await.context("begin tx failed")?;
    for row in &rows {
        query(
            r#"
            INSERT INTO question_stats (
                question_id, exam_session, source_workbook_id, participant_count,
                avg_score, score_std, full_mark_rate, zero_score_rate, max_score,
                min_score, stats_source, stats_version, created_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, NOW(), NOW()
            )
            ON CONFLICT (question_id, exam_session, stats_version)
            DO UPDATE SET
                source_workbook_id = EXCLUDED.source_workbook_id,
                participant_count = EXCLUDED.participant_count,
                avg_score = EXCLUDED.avg_score,
                score_std = EXCLUDED.score_std,
                full_mark_rate = EXCLUDED.full_mark_rate,
                zero_score_rate = EXCLUDED.zero_score_rate,
                max_score = EXCLUDED.max_score,
                min_score = EXCLUDED.min_score,
                stats_source = EXCLUDED.stats_source,
                updated_at = NOW()
            "#,
        )
        .bind(&row.question_id)
        .bind(&row.exam_session)
        .bind(request.source_workbook_id.as_deref())
        .bind(row.participant_count)
        .bind(row.avg_score)
        .bind(row.score_std)
        .bind(row.full_mark_rate)
        .bind(row.zero_score_rate)
        .bind(row.max_score)
        .bind(row.min_score)
        .bind(&request.stats_source)
        .bind(&request.stats_version)
        .execute(&mut *tx)
        .await
        .with_context(|| {
            format!(
                "upsert stats failed for question_id={}, exam_session={}",
                row.question_id, row.exam_session
            )
        })?;
    }
    tx.commit().await.context("commit stats import failed")?;

    Ok(Json(StatsImportResponse {
        csv_path: canonical_or_original(&csv_path),
        imported_stats: rows.len(),
        stats_source: request.stats_source,
        stats_version: request.stats_version,
        source_workbook_id: request.source_workbook_id,
    }))
}

async fn run_difficulty_scores(
    State(state): State<AppState>,
    Json(request): Json<DifficultyRunRequest>,
) -> ApiResult<DifficultyRunResponse> {
    let method_version = request
        .method_version
        .unwrap_or_else(|| "baseline-v1".to_string());

    let rows = query(
        r#"
        SELECT question_id, exam_session, participant_count, avg_score,
               zero_score_rate, full_mark_rate, max_score
        FROM question_stats
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .context("query question stats failed")?;

    let mut tx = state.pool.begin().await.context("begin tx failed")?;
    let mut updated = 0usize;
    for row in rows {
        let question_id: String = row.get("question_id");
        let exam_session: String = row.get("exam_session");
        let participant_count: i32 = row.get("participant_count");
        let avg_score: f64 = row.get("avg_score");
        let zero_score_rate: f64 = row.get("zero_score_rate");
        let full_mark_rate: f64 = row.get("full_mark_rate");
        let max_score: f64 = row.get("max_score");

        let feature_json = json!({
            "participant_count": participant_count,
            "avg_score": avg_score,
            "zero_score_rate": zero_score_rate,
            "full_mark_rate": full_mark_rate,
            "max_score": max_score,
        });

        let (derived_score, confidence) = if participant_count < 3 {
            (None, 0.0)
        } else {
            (
                Some(derive_difficulty(
                    avg_score,
                    max_score,
                    zero_score_rate,
                    full_mark_rate,
                )?),
                (participant_count as f64 / 50.0).min(1.0),
            )
        };

        query(
            r#"
            INSERT INTO difficulty_scores (
                question_id, exam_session, manual_level, derived_score,
                method, method_version, confidence, feature_json,
                created_at, updated_at
            )
            VALUES (
                $1, $2, NULL, $3,
                'baseline_rule', $4, $5, $6,
                NOW(), NOW()
            )
            ON CONFLICT (question_id, exam_session, method, method_version)
            DO UPDATE SET
                derived_score = EXCLUDED.derived_score,
                confidence = EXCLUDED.confidence,
                feature_json = EXCLUDED.feature_json,
                updated_at = NOW()
            "#,
        )
        .bind(&question_id)
        .bind(&exam_session)
        .bind(derived_score)
        .bind(&method_version)
        .bind(confidence)
        .bind(feature_json)
        .execute(&mut *tx)
        .await
        .with_context(|| {
            format!(
                "upsert difficulty failed for question_id={}, exam_session={}",
                question_id, exam_session
            )
        })?;
        updated += 1;
    }

    tx.commit().await.context("commit difficulty update failed")?;

    Ok(Json(DifficultyRunResponse {
        method_version,
        updated_count: updated,
    }))
}

async fn run_export(
    State(state): State<AppState>,
    Json(request): Json<ExportRequest>,
) -> ApiResult<ExportResponse> {
    let output_path = request
        .output_path
        .as_deref()
        .map(expand_path)
        .unwrap_or_else(|| default_export_path(request.format, request.public));
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create export parent directory failed: {}",
                parent.to_string_lossy()
            )
        })?;
    }

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
        output_path: canonical_or_original(&output_path),
        exported_questions: exported_count,
    }))
}

async fn run_quality_check(
    State(state): State<AppState>,
    Json(request): Json<QualityCheckRequest>,
) -> ApiResult<Value> {
    let output_path = request
        .output_path
        .as_deref()
        .map(expand_path)
        .unwrap_or_else(|| PathBuf::from("exports/quality_report.json"));

    let report = build_quality_report(&state.pool).await?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create quality report parent directory failed: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    let serialized = serde_json::to_string_pretty(&report).context("serialize quality report failed")?;
    fs::write(&output_path, serialized).with_context(|| {
        format!(
            "write quality report failed: {}",
            output_path.to_string_lossy()
        )
    })?;

    Ok(Json(json!({
        "output_path": canonical_or_original(&output_path),
        "report": report,
    })))
}

impl QuestionsParams {
    fn normalized_limit(&self) -> i64 {
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    fn normalized_offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }

    fn build_query(&self) -> QuestionsQuery {
        let mut builder = QueryBuilder::<Postgres>::new(
            "
            SELECT q.question_id, q.paper_id, q.paper_index, q.question_no, q.category, q.status,
                   q.search_text, q.question_tex_object_id::text AS question_tex_object_id,
                   q.answer_tex_object_id::text AS answer_tex_object_id, q.tags_json,
                   p.edition, p.paper_type, p.title
            FROM questions q
            JOIN papers p ON p.paper_id = q.paper_id
            WHERE 1 = 1",
        );
        let mut bind_count = 0;

        if let Some(edition) = &self.edition {
            builder.push(" AND p.edition = ").push_bind(edition);
            bind_count += 1;
        }
        if let Some(paper_id) = &self.paper_id {
            builder.push(" AND q.paper_id = ").push_bind(paper_id);
            bind_count += 1;
        }
        if let Some(paper_type) = &self.paper_type {
            builder.push(" AND p.paper_type = ").push_bind(paper_type);
            bind_count += 1;
        }
        if let Some(category) = &self.category {
            builder.push(" AND q.category = ").push_bind(category);
            bind_count += 1;
        }
        if let Some(has_assets) = self.has_assets {
            if has_assets {
                builder.push(" AND EXISTS (SELECT 1 FROM question_assets qa WHERE qa.question_id = q.question_id)");
            } else {
                builder.push(" AND NOT EXISTS (SELECT 1 FROM question_assets qa WHERE qa.question_id = q.question_id)");
            }
        }
        if let Some(has_answer) = self.has_answer {
            if has_answer {
                builder.push(" AND q.answer_tex_object_id IS NOT NULL");
            } else {
                builder.push(" AND q.answer_tex_object_id IS NULL");
            }
        }
        if let Some(min_avg_score) = self.min_avg_score {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_stats qs WHERE qs.question_id = q.question_id AND qs.avg_score >= ")
                .push_bind(min_avg_score)
                .push(')');
            bind_count += 1;
        }
        if let Some(max_avg_score) = self.max_avg_score {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_stats qs WHERE qs.question_id = q.question_id AND qs.avg_score <= ")
                .push_bind(max_avg_score)
                .push(')');
            bind_count += 1;
        }
        if let Some(tag) = &self.tag {
            builder
                .push(" AND q.tags_json @> ")
                .push_bind(serde_json::json!([tag]));
            bind_count += 1;
        }
        if let Some(search) = &self.q {
            let needle = format!("%{search}%");
            builder
                .push(" AND (COALESCE(q.search_text, '') ILIKE ")
                .push_bind(needle.clone())
                .push(" OR q.question_id ILIKE ")
                .push_bind(needle.clone())
                .push(" OR COALESCE(q.question_no, '') ILIKE ")
                .push_bind(needle)
                .push(')');
            bind_count += 3;
        }

        let limit = self.normalized_limit();
        let offset = self.normalized_offset();
        builder
            .push(" ORDER BY p.edition DESC, q.paper_id, q.paper_index LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        QuestionsQuery {
            sql: builder.sql().to_owned(),
            bind_count: bind_count + 2,
            limit,
            offset,
        }
    }
}

impl ScoreWorkbookParams {
    fn build_query(&self) -> (String, usize) {
        let mut builder = QueryBuilder::<Postgres>::new(
            "
            SELECT workbook_id, paper_id, exam_session, workbook_kind,
                   workbook_object_id::text AS workbook_object_id, source_filename,
                   mime_type, sheet_names_json, file_size, sha256, notes,
                   to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                   to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
            FROM score_workbooks
            WHERE 1 = 1",
        );
        let mut bind_count = 0;
        if let Some(paper_id) = &self.paper_id {
            builder.push(" AND paper_id = ").push_bind(paper_id);
            bind_count += 1;
        }
        if let Some(exam_session) = &self.exam_session {
            builder.push(" AND exam_session = ").push_bind(exam_session);
            bind_count += 1;
        }
        builder.push(" ORDER BY paper_id, exam_session, workbook_id");
        (builder.sql().to_owned(), bind_count)
    }
}

fn validate_question_filters(params: &QuestionsParams) -> Result<()> {
    if let Some(paper_type) = &params.paper_type {
        let valid = ["regular", "semifinal", "final", "other"];
        if !valid.contains(&paper_type.as_str()) {
            return Err(anyhow!("paper_type must be one of: regular, semifinal, final, other"));
        }
    }
    if let Some(category) = &params.category {
        let valid = ["theory", "experiment"];
        if !valid.contains(&category.as_str()) {
            return Err(anyhow!("category must be one of: theory, experiment"));
        }
    }
    Ok(())
}

async fn execute_questions_query(
    pool: &PgPool,
    params: &QuestionsParams,
    plan: &QuestionsQuery,
) -> Result<Vec<PgRow>, sqlx::Error> {
    let mut query = query(&plan.sql);
    if let Some(edition) = &params.edition {
        query = query.bind(edition);
    }
    if let Some(paper_id) = &params.paper_id {
        query = query.bind(paper_id);
    }
    if let Some(paper_type) = &params.paper_type {
        query = query.bind(paper_type);
    }
    if let Some(category) = &params.category {
        query = query.bind(category);
    }
    if let Some(min_avg_score) = params.min_avg_score {
        query = query.bind(min_avg_score);
    }
    if let Some(max_avg_score) = params.max_avg_score {
        query = query.bind(max_avg_score);
    }
    if let Some(tag) = &params.tag {
        query = query.bind(serde_json::json!([tag]));
    }
    if let Some(search) = &params.q {
        let needle = format!("%{search}%");
        query = query.bind(needle.clone()).bind(needle.clone()).bind(needle);
    }
    debug_assert_eq!(plan.bind_count, count_question_binds(params));
    query.bind(plan.limit).bind(plan.offset).fetch_all(pool).await
}

async fn execute_score_workbooks_query(
    pool: &PgPool,
    params: &ScoreWorkbookParams,
    sql: &str,
    bind_count: usize,
) -> Result<Vec<PgRow>, sqlx::Error> {
    let mut query = query(sql);
    if let Some(paper_id) = &params.paper_id {
        query = query.bind(paper_id);
    }
    if let Some(exam_session) = &params.exam_session {
        query = query.bind(exam_session);
    }
    debug_assert_eq!(bind_count, count_score_workbook_binds(params));
    query.fetch_all(pool).await
}

fn count_question_binds(params: &QuestionsParams) -> usize {
    usize::from(params.edition.is_some())
        + usize::from(params.paper_id.is_some())
        + usize::from(params.paper_type.is_some())
        + usize::from(params.category.is_some())
        + usize::from(params.min_avg_score.is_some())
        + usize::from(params.max_avg_score.is_some())
        + usize::from(params.tag.is_some())
        + params.q.as_ref().map(|_| 3).unwrap_or(0)
        + 2
}

fn count_score_workbook_binds(params: &ScoreWorkbookParams) -> usize {
    usize::from(params.paper_id.is_some()) + usize::from(params.exam_session.is_some())
}

fn map_paper_summary(row: PgRow) -> PaperSummary {
    PaperSummary {
        paper_id: row.get("paper_id"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
        paper_tex_object_id: row.get("paper_tex_object_id"),
        source_pdf_object_id: row.get("source_pdf_object_id"),
        question_index: row.get::<Value, _>("question_index_json"),
        notes: row.get("notes"),
    }
}

fn map_paper_question_summary(row: PgRow) -> PaperQuestionSummary {
    PaperQuestionSummary {
        question_id: row.get("question_id"),
        paper_index: row.get("paper_index"),
        question_no: row.get("question_no"),
        category: row.get("category"),
        question_tex_object_id: row.get("question_tex_object_id"),
        answer_tex_object_id: row.get("answer_tex_object_id"),
        status: row.get("status"),
        tags: row.get::<Value, _>("tags_json"),
    }
}

fn map_question_summary(row: PgRow) -> QuestionSummary {
    QuestionSummary {
        question_id: row.get("question_id"),
        paper_id: row.get("paper_id"),
        paper_index: row.get("paper_index"),
        question_no: row.get("question_no"),
        category: row.get("category"),
        status: row.get("status"),
        search_text: row.get("search_text"),
        question_tex_object_id: row.get("question_tex_object_id"),
        answer_tex_object_id: row.get("answer_tex_object_id"),
        tags: row.get::<Value, _>("tags_json"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
    }
}

fn map_question_asset(row: PgRow) -> QuestionAsset {
    QuestionAsset {
        asset_id: row.get("asset_id"),
        kind: row.get("kind"),
        object_id: row.get("object_id"),
        caption: row.get("caption"),
        sort_order: row.get("sort_order"),
    }
}

fn map_question_stat(row: PgRow) -> QuestionStat {
    QuestionStat {
        exam_session: row.get("exam_session"),
        source_workbook_id: row.get("source_workbook_id"),
        participant_count: row.get("participant_count"),
        avg_score: row.get("avg_score"),
        score_std: row.get("score_std"),
        full_mark_rate: row.get("full_mark_rate"),
        zero_score_rate: row.get("zero_score_rate"),
        max_score: row.get("max_score"),
        min_score: row.get("min_score"),
        stats_source: row.get("stats_source"),
        stats_version: row.get("stats_version"),
    }
}

fn map_difficulty_score(row: PgRow) -> DifficultyScore {
    DifficultyScore {
        exam_session: row.get("exam_session"),
        manual_level: row.get("manual_level"),
        derived_score: row.get("derived_score"),
        method: row.get("method"),
        method_version: row.get("method_version"),
        confidence: row.get("confidence"),
        feature_json: row.get::<Value, _>("feature_json"),
    }
}

fn map_score_workbook_summary(row: PgRow) -> ScoreWorkbookSummary {
    ScoreWorkbookSummary {
        workbook_id: row.get("workbook_id"),
        paper_id: row.get("paper_id"),
        exam_session: row.get("exam_session"),
        workbook_kind: row.get("workbook_kind"),
        workbook_object_id: row.get("workbook_object_id"),
        source_filename: row.get("source_filename"),
        mime_type: row.get("mime_type"),
        sheet_names: row.get::<Value, _>("sheet_names_json"),
        file_size: row.get("file_size"),
        sha256: row.get("sha256"),
        notes: row.get("notes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn expand_path(input: &str) -> PathBuf {
    if input == "~" {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(stripped) = input.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(input)
}

fn canonical_or_original(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn join_bundle_path(bundle_path: &Path, rel_or_abs: &str) -> PathBuf {
    let candidate = PathBuf::from(rel_or_abs);
    if candidate.is_absolute() {
        candidate
    } else {
        bundle_path.join(candidate)
    }
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)
        .with_context(|| format!("read json file failed: {}", path.to_string_lossy()))?;
    let stripped = strip_utf8_bom(&bytes);
    let parsed = serde_json::from_slice(stripped)
        .with_context(|| format!("parse json file failed: {}", path.to_string_lossy()))?;
    Ok(parsed)
}

fn normalize_search_text(parts: &[Option<&str>], limit: usize) -> String {
    let merged = parts.iter().flatten().copied().collect::<Vec<_>>().join(" ");
    let without_cmd = LATEX_CMD_RE.replace_all(&merged, " ");
    let without_special = SPECIAL_RE.replace_all(&without_cmd, " ");
    let compact = WHITESPACE_RE.replace_all(&without_special, " ").trim().to_string();
    if compact.chars().count() > limit {
        compact.chars().take(limit).collect()
    } else {
        compact
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn xlsx_sheet_names(path: &Path) -> Result<Vec<String>> {
    let file = fs::File::open(path)
        .with_context(|| format!("open xlsx failed: {}", path.to_string_lossy()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("open zip structure failed: {}", path.to_string_lossy()))?;
    let mut xml = String::new();
    archive
        .by_name("xl/workbook.xml")
        .context("xlsx workbook.xml not found")?
        .read_to_string(&mut xml)
        .context("read workbook.xml failed")?;
    let doc = roxmltree::Document::parse(&xml).context("parse workbook.xml failed")?;
    let mut names = Vec::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("sheet")) {
        if let Some(name) = node.attribute("name") {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

fn inspect_bundle(bundle_path: &Path) -> Result<(ValidationResult, Option<LoadedBundle>)> {
    let mut validation = ValidationResult::default();

    if !bundle_path.exists() {
        validation
            .errors
            .push(format!("bundle path does not exist: {}", bundle_path.to_string_lossy()));
        return Ok((validation, None));
    }
    if !bundle_path.is_dir() {
        validation.errors.push(format!(
            "bundle path must be a directory: {}",
            bundle_path.to_string_lossy()
        ));
        return Ok((validation, None));
    }

    let manifest_path = bundle_path.join("manifest.json");
    if !manifest_path.exists() {
        validation.errors.push("manifest.json is missing".to_string());
        return Ok((validation, None));
    }

    let manifest_value: Value = read_json_file(&manifest_path)?;
    let Some(manifest_obj) = manifest_value.as_object() else {
        validation
            .errors
            .push("manifest.json root must be an object".to_string());
        return Ok((validation, None));
    };

    let required_manifest_keys = ["bundle_name", "run_label", "paper"];
    let missing_manifest_keys = missing_keys(manifest_obj, &required_manifest_keys);
    if !missing_manifest_keys.is_empty() {
        validation.errors.push(format!(
            "manifest.json missing fields: {:?}",
            missing_manifest_keys
        ));
    }

    let paper_obj = manifest_obj
        .get("paper")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required_paper_keys = ["paper_id", "edition", "paper_type", "title", "paper_latex_path"];
    let missing_paper_keys = missing_keys(&paper_obj, &required_paper_keys);
    if !missing_paper_keys.is_empty() {
        validation.errors.push(format!(
            "paper config missing fields: {:?}",
            missing_paper_keys
        ));
    }

    if let Some(paper_type) = paper_obj.get("paper_type").and_then(Value::as_str) {
        if !["regular", "semifinal", "final", "other"].contains(&paper_type) {
            validation.errors.push(
                "paper.paper_type must be one of regular/semifinal/final/other".to_string(),
            );
        }
    }

    if let Some(path) = paper_obj.get("paper_latex_path").and_then(Value::as_str) {
        if !join_bundle_path(bundle_path, path).exists() {
            validation
                .errors
                .push(format!("paper_latex_path does not exist: {path}"));
        }
    }

    if let Some(path) = paper_obj.get("source_pdf_path").and_then(Value::as_str) {
        if !join_bundle_path(bundle_path, path).exists() {
            validation
                .warnings
                .push(format!("source_pdf_path does not exist: {path}"));
        }
    }

    let mut seen_workbook_ids = HashSet::new();
    if let Some(workbooks) = manifest_obj.get("score_workbooks").and_then(Value::as_array) {
        for workbook in workbooks {
            let Some(workbook_obj) = workbook.as_object() else {
                validation
                    .errors
                    .push("workbook entry must be an object".to_string());
                continue;
            };
            let workbook_id = workbook_obj
                .get("workbook_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if workbook_id.is_none() {
                validation
                    .errors
                    .push("score_workbooks entry missing workbook_id".to_string());
            }
            if let Some(id) = workbook_id {
                if !seen_workbook_ids.insert(id.clone()) {
                    validation
                        .errors
                        .push(format!("duplicate workbook_id: {id}"));
                }
            }
            for key in ["exam_session", "workbook_kind", "file_path"] {
                if !workbook_obj.contains_key(key) {
                    validation.errors.push(format!(
                        "workbook {} missing field: {key}",
                        workbook_obj
                            .get("workbook_id")
                            .and_then(Value::as_str)
                            .unwrap_or("<unknown>")
                    ));
                }
            }
            if let Some(path) = workbook_obj.get("file_path").and_then(Value::as_str) {
                if !join_bundle_path(bundle_path, path).exists() {
                    validation
                        .errors
                        .push(format!("workbook file does not exist: {path}"));
                }
            }
        }
    }

    let questions_dir = bundle_path.join("questions");
    if !questions_dir.exists() {
        validation.errors.push("questions/ directory is missing".to_string());
        return Ok((validation, None));
    }

    let mut question_files = fs::read_dir(&questions_dir)
        .with_context(|| format!("read questions dir failed: {}", questions_dir.to_string_lossy()))?
        .filter_map(|entry| entry.ok().map(|v| v.path()))
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    question_files.sort();

    if question_files.is_empty() {
        validation
            .errors
            .push("questions/ has no JSON files".to_string());
    }

    let mut parsed_questions = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut seen_numbers = HashSet::new();
    let mut seen_indexes = HashSet::new();
    let allowed_question_keys: BTreeSet<&str> = BTreeSet::from([
        "question_id",
        "question_no",
        "paper_index",
        "category",
        "latex_path",
        "answer_latex_path",
        "latex_anchor",
        "search_text",
        "status",
        "tags",
        "assets",
        "notes",
    ]);

    for (idx, path) in question_files.iter().enumerate() {
        let label = format!("question #{}", idx + 1);
        let value: Value = read_json_file(path)?;
        let Some(obj) = value.as_object() else {
            validation
                .errors
                .push(format!("{label} must be a JSON object"));
            continue;
        };

        let required_question_keys = [
            "question_id",
            "question_no",
            "paper_index",
            "category",
            "latex_path",
            "status",
            "tags",
            "assets",
        ];
        let missing = missing_keys(obj, &required_question_keys);
        if !missing.is_empty() {
            validation
                .errors
                .push(format!("{label} missing fields: {:?}", missing));
        }

        let unknown = obj
            .keys()
            .filter(|key| !allowed_question_keys.contains(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            validation
                .warnings
                .push(format!("{label} has unknown fields: {:?}", unknown));
        }

        let parsed: BundleQuestion = match serde_json::from_value(value.clone()) {
            Ok(v) => v,
            Err(err) => {
                validation
                    .errors
                    .push(format!("{label} failed to parse: {err}"));
                continue;
            }
        };

        if !seen_ids.insert(parsed.question_id.clone()) {
            validation
                .errors
                .push(format!("duplicate question_id: {}", parsed.question_id));
        }
        if !seen_numbers.insert(parsed.question_no.clone()) {
            validation
                .warnings
                .push(format!("duplicate question_no in bundle: {}", parsed.question_no));
        }
        if !seen_indexes.insert(parsed.paper_index) {
            validation
                .errors
                .push(format!("duplicate paper_index: {}", parsed.paper_index));
        }

        if !["theory", "experiment"].contains(&parsed.category.as_str()) {
            validation.errors.push(format!(
                "{} category must be theory/experiment",
                parsed.question_id
            ));
        }
        if !["raw", "reviewed", "published"].contains(&parsed.status.as_str()) {
            validation
                .errors
                .push(format!("{} status must be raw/reviewed/published", parsed.question_id));
        }

        let latex_path = join_bundle_path(bundle_path, &parsed.latex_path);
        if !latex_path.exists() {
            validation.errors.push(format!(
                "{} latex_path does not exist: {}",
                parsed.question_id, parsed.latex_path
            ));
        }
        if let Some(answer_path) = &parsed.answer_latex_path {
            let resolved = join_bundle_path(bundle_path, answer_path);
            if !resolved.exists() {
                validation.errors.push(format!(
                    "{} answer_latex_path does not exist: {}",
                    parsed.question_id, answer_path
                ));
            }
        }

        for asset in &parsed.assets {
            if asset.file_path.trim().is_empty() {
                validation
                    .errors
                    .push(format!("{} asset missing file_path", parsed.question_id));
                continue;
            }
            let asset_path = join_bundle_path(bundle_path, &asset.file_path);
            if !asset_path.exists() {
                validation.errors.push(format!(
                    "{} asset does not exist: {}",
                    parsed.question_id, asset.file_path
                ));
                continue;
            }
            let bytes = fs::read(&asset_path).with_context(|| {
                format!("read asset failed: {}", asset_path.to_string_lossy())
            })?;
            let actual = sha256_hex(&bytes);
            if let Some(expected) = &asset.sha256 {
                if expected.to_lowercase() != actual {
                    validation.errors.push(format!(
                        "{} asset checksum mismatch: {}",
                        parsed.question_id, asset.file_path
                    ));
                }
            } else {
                validation.warnings.push(format!(
                    "{} asset has no sha256: {}",
                    parsed.question_id, asset.file_path
                ));
            }
        }

        parsed_questions.push(parsed);
    }

    let manifest = match serde_json::from_value::<BundleManifest>(manifest_value) {
        Ok(v) => v,
        Err(err) => {
            validation
                .errors
                .push(format!("manifest parse failed: {err}"));
            return Ok((validation, None));
        }
    };

    let loaded = if parsed_questions.is_empty() {
        None
    } else {
        Some(LoadedBundle {
            manifest,
            questions: parsed_questions,
        })
    };

    Ok((validation, loaded))
}

fn missing_keys(map: &Map<String, Value>, required_keys: &[&str]) -> Vec<String> {
    required_keys
        .iter()
        .filter(|key| !map.contains_key(**key))
        .map(|key| key.to_string())
        .collect()
}

fn hydrate_bundle_questions(bundle_path: &Path, questions: &[BundleQuestion]) -> Result<Vec<HydratedQuestion>> {
    let mut hydrated = Vec::with_capacity(questions.len());
    for question in questions {
        let latex_path = join_bundle_path(bundle_path, &question.latex_path);
        let latex_bytes = fs::read(&latex_path).with_context(|| {
            format!("read question tex failed: {}", latex_path.to_string_lossy())
        })?;
        let latex_source = String::from_utf8_lossy(&latex_bytes).to_string();

        let answer_source = if let Some(path) = &question.answer_latex_path {
            let answer_path = join_bundle_path(bundle_path, path);
            if answer_path.exists() {
                let bytes = fs::read(&answer_path).with_context(|| {
                    format!("read answer tex failed: {}", answer_path.to_string_lossy())
                })?;
                Some(String::from_utf8_lossy(&bytes).to_string())
            } else {
                None
            }
        } else {
            None
        };

        let comparison_text = normalize_search_text(
            &[
                question.search_text.as_deref(),
                Some(latex_source.as_str()),
                answer_source.as_deref(),
            ],
            1000,
        );

        hydrated.push(HydratedQuestion {
            question: question.clone(),
            latex_source,
            answer_source,
            comparison_text,
        });
    }
    Ok(hydrated)
}

async fn detect_question_no_conflicts(
    pool: &PgPool,
    paper_id: &str,
    questions: &[BundleQuestion],
) -> Result<Vec<String>> {
    let mut errors = Vec::new();
    for question in questions {
        let existing = query(
            "SELECT question_id FROM questions WHERE paper_id = $1 AND question_no = $2",
        )
        .bind(paper_id)
        .bind(&question.question_no)
        .fetch_optional(pool)
        .await
        .with_context(|| {
            format!(
                "query question_no conflict failed: paper_id={}, question_no={}",
                paper_id, question.question_no
            )
        })?;
        if let Some(existing_row) = existing {
            let existing_id: String = existing_row.get("question_id");
            if existing_id != question.question_id {
                errors.push(format!(
                    "question_no conflict in paper {}: question_no {} already used by {}",
                    paper_id, question.question_no, existing_id
                ));
            }
        }
    }
    Ok(errors)
}

async fn find_similarity_issues(
    pool: &PgPool,
    _paper_id: &str,
    questions: &[HydratedQuestion],
    allow_similar: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let rows = query("SELECT question_id, COALESCE(search_text, '') AS comparison_text FROM questions")
        .fetch_all(pool)
        .await
        .context("query existing questions for similarity check failed")?;

    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for incoming in questions {
        if incoming.comparison_text.is_empty() {
            continue;
        }
        let mut matches = Vec::new();
        for row in &rows {
            let existing_id: String = row.get("question_id");
            let existing_text: String = row.get("comparison_text");
            if existing_text.is_empty() || existing_id == incoming.question.question_id {
                continue;
            }
            let ratio = normalized_levenshtein(&incoming.comparison_text, &existing_text);
            if ratio >= SIMILARITY_THRESHOLD {
                matches.push(format!("{} ({ratio:.3})", existing_id));
            }
        }

        if !matches.is_empty() {
            let msg = format!(
                "{} is highly similar to existing questions: {}",
                incoming.question.question_id,
                matches.join(", ")
            );
            if allow_similar {
                warnings.push(msg);
            } else {
                errors.push(msg);
            }
        }
    }

    Ok((warnings, errors))
}

async fn upsert_object_tx(
    tx: &mut Transaction<'_, Postgres>,
    kind: &str,
    source_path: &Path,
    bytes: &[u8],
    mime_type: Option<&str>,
    created_by: &str,
) -> Result<String> {
    let size = i64::try_from(bytes.len()).context("object bytes exceed i64 range")?;
    let sha = sha256_hex(bytes);

    if let Some(existing) = query(
        "SELECT object_id::text AS object_id FROM objects WHERE sha256 = $1 AND size_bytes = $2",
    )
    .bind(&sha)
    .bind(size)
    .fetch_optional(&mut **tx)
    .await
    .context("query existing object by hash failed")?
    {
        let object_id: String = existing.get("object_id");
        query("INSERT INTO object_blobs (object_id, content) VALUES ($1::uuid, $2) ON CONFLICT (object_id) DO NOTHING")
            .bind(&object_id)
            .bind(bytes)
            .execute(&mut **tx)
            .await
            .context("ensure existing object blob failed")?;
        return Ok(object_id);
    }

    let object_id = Uuid::new_v4().to_string();
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "blob.bin".to_string());
    let prefix = sha.get(0..4).unwrap_or("0000");
    let object_key = format!("qb/local/{kind}/{prefix}/{file_name}");

    query(
        r#"
        INSERT INTO objects (
            object_id, bucket, object_key, sha256, size_bytes,
            mime_type, storage_class, created_at, created_by, encryption
        ) VALUES (
            $1::uuid, $2, $3, $4, $5,
            $6, 'hot', NOW(), $7, 'sse'
        )
        "#,
    )
    .bind(&object_id)
    .bind(OBJECT_BUCKET)
    .bind(&object_key)
    .bind(&sha)
    .bind(size)
    .bind(mime_type)
    .bind(created_by)
    .execute(&mut **tx)
    .await
    .context("insert object failed")?;

    query("INSERT INTO object_blobs (object_id, content) VALUES ($1::uuid, $2)")
        .bind(&object_id)
        .bind(bytes)
        .execute(&mut **tx)
        .await
        .context("insert object blob failed")?;

    Ok(object_id)
}

async fn upsert_workbook_tx(
    tx: &mut Transaction<'_, Postgres>,
    paper_id: &str,
    workbook: &ManifestWorkbook,
    bundle_path: &Path,
) -> Result<()> {
    let workbook_path = join_bundle_path(bundle_path, &workbook.file_path);
    let workbook_bytes = fs::read(&workbook_path).with_context(|| {
        format!(
            "read workbook failed: {}",
            workbook_path.to_string_lossy()
        )
    })?;
    let workbook_object_id = upsert_object_tx(
        tx,
        "workbook",
        &workbook_path,
        &workbook_bytes,
        Some(XLSX_MIME_TYPE),
        "workbook_import",
    )
    .await?;
    let sheet_names = xlsx_sheet_names(&workbook_path)?;

    let file_size = i64::try_from(workbook_bytes.len()).context("workbook too large")?;
    let sha256 = sha256_hex(&workbook_bytes);
    let source_filename = workbook_path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| workbook.file_path.clone());

    query(
        r#"
        INSERT INTO score_workbooks (
            workbook_id, paper_id, exam_session, workbook_kind,
            workbook_object_id, source_filename, mime_type, sheet_names_json,
            file_size, sha256, notes, created_at, updated_at
        )
        VALUES (
            $1, $2, $3, $4,
            $5::uuid, $6, $7, $8,
            $9, $10, $11, NOW(), NOW()
        )
        ON CONFLICT (workbook_id)
        DO UPDATE SET
            paper_id = EXCLUDED.paper_id,
            exam_session = EXCLUDED.exam_session,
            workbook_kind = EXCLUDED.workbook_kind,
            workbook_object_id = EXCLUDED.workbook_object_id,
            source_filename = EXCLUDED.source_filename,
            mime_type = EXCLUDED.mime_type,
            sheet_names_json = EXCLUDED.sheet_names_json,
            file_size = EXCLUDED.file_size,
            sha256 = EXCLUDED.sha256,
            notes = EXCLUDED.notes,
            updated_at = NOW()
        "#,
    )
    .bind(&workbook.workbook_id)
    .bind(paper_id)
    .bind(&workbook.exam_session)
    .bind(&workbook.workbook_kind)
    .bind(&workbook_object_id)
    .bind(&source_filename)
    .bind(XLSX_MIME_TYPE)
    .bind(Value::Array(
        sheet_names
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>(),
    ))
    .bind(file_size)
    .bind(sha256)
    .bind(workbook.notes.as_deref())
    .execute(&mut **tx)
    .await
    .with_context(|| format!("upsert workbook failed: {}", workbook.workbook_id))?;

    Ok(())
}

async fn insert_import_run(
    pool: &PgPool,
    bundle_path: &Path,
    bundle_name: Option<&str>,
    dry_run: bool,
    status: &str,
    item_count: usize,
    warnings: &[String],
    errors: &[String],
    paper_id: Option<&str>,
    run_label_override: Option<&str>,
) -> Result<()> {
    let run_label = run_label_override
        .map(str::to_string)
        .or_else(|| {
            bundle_name
                .map(str::to_string)
                .or_else(|| bundle_path.file_name().map(|v| v.to_string_lossy().to_string()))
        })
        .unwrap_or_else(|| "bundle-import".to_string());

    let details = json!({
        "bundle_name": bundle_name,
        "paper_id": paper_id,
        "warnings": warnings,
        "errors": errors,
    });

    query(
        r#"
        INSERT INTO import_runs (
            run_label, bundle_path, dry_run, status, item_count,
            warning_count, error_count, details_json, started_at, finished_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())
        "#,
    )
    .bind(run_label)
    .bind(bundle_path.to_string_lossy().to_string())
    .bind(dry_run)
    .bind(status)
    .bind(i32::try_from(item_count).unwrap_or(i32::MAX))
    .bind(i32::try_from(warnings.len()).unwrap_or(i32::MAX))
    .bind(i32::try_from(errors.len()).unwrap_or(i32::MAX))
    .bind(details)
    .execute(pool)
    .await
    .context("insert import_run failed")?;

    Ok(())
}

fn field_index_map(headers: &StringRecord) -> HashMap<String, usize> {
    headers
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.trim_start_matches('\u{feff}').trim().to_string(), idx))
        .collect()
}

fn get_required_csv_index(map: &HashMap<String, usize>, name: &str) -> Result<usize> {
    map.get(name)
        .copied()
        .ok_or_else(|| anyhow!("CSV missing required field: {name}"))
}

fn aggregate_score_rows(csv_path: &Path) -> Result<Vec<AggregatedScoreRow>> {
    let mut reader = ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(csv_path)
        .with_context(|| format!("open csv failed: {}", csv_path.to_string_lossy()))?;

    let headers = reader
        .headers()
        .context("read csv headers failed")?
        .clone();
    let index = field_index_map(&headers);

    let question_idx = get_required_csv_index(&index, "question_id")?;
    let exam_idx = get_required_csv_index(&index, "exam_session")?;
    let score_idx = get_required_csv_index(&index, "score")?;
    let max_score_idx = get_required_csv_index(&index, "max_score")?;

    let mut grouped: HashMap<(String, String), Vec<(f64, f64)>> = HashMap::new();

    for row in reader.records() {
        let row = row.context("read csv row failed")?;
        let question_id = row
            .get(question_idx)
            .ok_or_else(|| anyhow!("question_id is missing in csv row"))?
            .to_string();
        let exam_session = row
            .get(exam_idx)
            .ok_or_else(|| anyhow!("exam_session is missing in csv row"))?
            .to_string();
        let score = row
            .get(score_idx)
            .ok_or_else(|| anyhow!("score is missing in csv row"))?
            .parse::<f64>()
            .with_context(|| format!("invalid score for question_id={question_id}"))?;
        let max_score = row
            .get(max_score_idx)
            .ok_or_else(|| anyhow!("max_score is missing in csv row"))?
            .parse::<f64>()
            .with_context(|| format!("invalid max_score for question_id={question_id}"))?;

        grouped
            .entry((question_id, exam_session))
            .or_default()
            .push((score, max_score));
    }

    let mut results = Vec::new();
    for ((question_id, exam_session), values) in grouped {
        let participant_count = i32::try_from(values.len()).context("participant_count overflow")?;
        if participant_count == 0 {
            continue;
        }

        let scores = values.iter().map(|(score, _)| *score).collect::<Vec<_>>();
        let max_scores = values.iter().map(|(_, max_score)| *max_score).collect::<Vec<_>>();

        let avg_score = scores.iter().sum::<f64>() / f64::from(participant_count);
        let variance = scores
            .iter()
            .map(|score| {
                let diff = *score - avg_score;
                diff * diff
            })
            .sum::<f64>()
            / f64::from(participant_count);
        let max_score = max_scores
            .into_iter()
            .fold(f64::MIN, |acc, value| acc.max(value));
        let min_score = scores
            .iter()
            .copied()
            .fold(f64::MAX, |acc, value| acc.min(value));

        let full_mark_rate = values
            .iter()
            .filter(|(score, max_s)| (*score - *max_s).abs() < f64::EPSILON)
            .count() as f64
            / f64::from(participant_count);
        let zero_score_rate = scores
            .iter()
            .filter(|score| score.abs() < f64::EPSILON)
            .count() as f64
            / f64::from(participant_count);

        results.push(AggregatedScoreRow {
            question_id,
            exam_session,
            participant_count,
            avg_score,
            score_std: variance.sqrt(),
            full_mark_rate,
            zero_score_rate,
            max_score,
            min_score,
        });
    }

    results.sort_by(|a, b| {
        a.exam_session
            .cmp(&b.exam_session)
            .then(a.question_id.cmp(&b.question_id))
    });

    Ok(results)
}

fn derive_difficulty(
    avg_score: f64,
    max_score: f64,
    zero_score_rate: f64,
    full_mark_rate: f64,
) -> Result<f64> {
    if max_score <= 0.0 {
        return Err(anyhow!("max_score must be greater than 0"));
    }
    let normalized_avg = (avg_score / max_score).clamp(0.0, 1.0);
    let derived = 0.55 * (1.0 - normalized_avg) + 0.25 * zero_score_rate + 0.20 * (1.0 - full_mark_rate);
    Ok(derived.clamp(0.0, 1.0))
}

fn default_export_path(format: ExportFormat, is_public: bool) -> PathBuf {
    let suffix = if is_public { "public" } else { "internal" };
    let ext = match format {
        ExportFormat::Jsonl => "jsonl",
        ExportFormat::Csv => "csv",
    };
    PathBuf::from("exports").join(format!("question_bank_{suffix}.{ext}"))
}

async fn fetch_text_object(pool: &PgPool, object_id: Option<&str>) -> Result<Option<String>> {
    let Some(object_id) = object_id else {
        return Ok(None);
    };

    let row = query("SELECT content FROM object_blobs WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("query object blob failed: {object_id}"))?;

    Ok(row.map(|r| {
        let content: Vec<u8> = r.get("content");
        String::from_utf8_lossy(&content).to_string()
    }))
}

async fn export_jsonl(pool: &PgPool, output_path: &Path, include_answers: bool) -> Result<usize> {
    let rows = query(
        r#"
        SELECT q.question_id, q.paper_id, q.paper_index, q.question_no, q.category,
               q.question_tex_object_id::text AS question_tex_object_id,
               q.answer_tex_object_id::text AS answer_tex_object_id,
               q.search_text, q.status, q.tags_json,
               p.title AS paper_title, p.edition, p.paper_type,
               p.paper_tex_object_id::text AS paper_tex_object_id,
               p.source_pdf_object_id::text AS source_pdf_object_id,
               p.question_index_json
        FROM questions q
        JOIN papers p ON p.paper_id = q.paper_id
        ORDER BY p.edition, p.paper_id, q.paper_index
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query questions for jsonl export failed")?;

    let file = fs::File::create(output_path)
        .with_context(|| format!("create export file failed: {}", output_path.to_string_lossy()))?;
    let mut writer = BufWriter::new(file);

    for row in &rows {
        let question_id: String = row.get("question_id");
        let paper_id: String = row.get("paper_id");
        let question_tex_id: Option<String> = row.get("question_tex_object_id");
        let answer_tex_id: Option<String> = row.get("answer_tex_object_id");
        let paper_tex_id: Option<String> = row.get("paper_tex_object_id");

        let assets = query(
            r#"
            SELECT asset_id, kind, object_id::text AS object_id, caption, sort_order
            FROM question_assets
            WHERE question_id = $1
            ORDER BY sort_order, asset_id
            "#,
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query assets for export failed: {}", question_id))?
        .into_iter()
        .map(|asset| {
            json!({
                "asset_id": asset.get::<String, _>("asset_id"),
                "kind": asset.get::<String, _>("kind"),
                "object_id": asset.get::<String, _>("object_id"),
                "caption": asset.get::<Option<String>, _>("caption"),
                "sort_order": asset.get::<i32, _>("sort_order"),
            })
        })
        .collect::<Vec<_>>();

        let stats = query(
            r#"
            SELECT exam_session, source_workbook_id, participant_count, avg_score,
                   score_std, full_mark_rate, zero_score_rate, max_score,
                   min_score, stats_source, stats_version
            FROM question_stats
            WHERE question_id = $1
            ORDER BY exam_session
            "#,
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query stats for export failed: {}", question_id))?
        .into_iter()
        .map(|stat| {
            json!({
                "exam_session": stat.get::<String, _>("exam_session"),
                "source_workbook_id": stat.get::<Option<String>, _>("source_workbook_id"),
                "participant_count": stat.get::<i32, _>("participant_count"),
                "avg_score": stat.get::<f64, _>("avg_score"),
                "score_std": stat.get::<f64, _>("score_std"),
                "full_mark_rate": stat.get::<f64, _>("full_mark_rate"),
                "zero_score_rate": stat.get::<f64, _>("zero_score_rate"),
                "max_score": stat.get::<f64, _>("max_score"),
                "min_score": stat.get::<f64, _>("min_score"),
                "stats_source": stat.get::<String, _>("stats_source"),
                "stats_version": stat.get::<String, _>("stats_version"),
            })
        })
        .collect::<Vec<_>>();

        let workbooks = query(
            r#"
            SELECT workbook_id, exam_session, workbook_kind, source_filename,
                   sheet_names_json, file_size, sha256
            FROM score_workbooks
            WHERE paper_id = $1
            ORDER BY exam_session, workbook_id
            "#,
        )
        .bind(&paper_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query workbooks for export failed: {}", paper_id))?
        .into_iter()
        .map(|wb| {
            json!({
                "workbook_id": wb.get::<String, _>("workbook_id"),
                "exam_session": wb.get::<String, _>("exam_session"),
                "workbook_kind": wb.get::<String, _>("workbook_kind"),
                "source_filename": wb.get::<String, _>("source_filename"),
                "sheet_names": wb.get::<Value, _>("sheet_names_json"),
                "file_size": wb.get::<i64, _>("file_size"),
                "sha256": wb.get::<String, _>("sha256"),
            })
        })
        .collect::<Vec<_>>();

        let question_tex_source = fetch_text_object(pool, question_tex_id.as_deref()).await?;
        let answer_tex_source = fetch_text_object(pool, answer_tex_id.as_deref()).await?;
        let paper_tex_source = fetch_text_object(pool, paper_tex_id.as_deref()).await?;

        let mut payload = json!({
            "question_id": question_id,
            "paper_id": paper_id,
            "paper_title": row.get::<String, _>("paper_title"),
            "edition": row.get::<String, _>("edition"),
            "paper_type": row.get::<String, _>("paper_type"),
            "paper_tex_object_id": row.get::<Option<String>, _>("paper_tex_object_id"),
            "paper_tex_source": paper_tex_source,
            "source_pdf_object_id": row.get::<Option<String>, _>("source_pdf_object_id"),
            "paper_question_index": row.get::<Value, _>("question_index_json"),
            "paper_index": row.get::<i32, _>("paper_index"),
            "question_no": row.get::<Option<String>, _>("question_no"),
            "category": row.get::<String, _>("category"),
            "question_tex_object_id": question_tex_id,
            "question_tex_source": question_tex_source,
            "search_text": row.get::<Option<String>, _>("search_text"),
            "status": row.get::<String, _>("status"),
            "tags": row.get::<Value, _>("tags_json"),
            "assets": assets,
            "stats": stats,
            "score_workbooks": workbooks,
        });

        if include_answers {
            payload["answer_tex_object_id"] = Value::String(answer_tex_id.unwrap_or_default());
            payload["answer_tex_source"] = answer_tex_source.map(Value::String).unwrap_or(Value::Null);
        }

        writer
            .write_all(serde_json::to_string(&payload)?.as_bytes())
            .context("write jsonl line failed")?;
        writer.write_all(b"\n").context("write newline failed")?;
    }

    writer.flush().context("flush jsonl writer failed")?;
    Ok(rows.len())
}

async fn export_csv(pool: &PgPool, output_path: &Path, include_answers: bool) -> Result<usize> {
    let rows = query(
        r#"
        SELECT q.question_id, q.paper_id, q.paper_index, q.question_no, q.category,
               q.status, p.edition, p.paper_type,
               q.question_tex_object_id::text AS question_tex_object_id,
               q.answer_tex_object_id::text AS answer_tex_object_id,
               q.search_text, q.tags_json
        FROM questions q
        JOIN papers p ON p.paper_id = q.paper_id
        ORDER BY p.edition, q.paper_index
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query questions for csv export failed")?;

    let file = fs::File::create(output_path)
        .with_context(|| format!("create export csv failed: {}", output_path.to_string_lossy()))?;
    let mut writer = WriterBuilder::new().from_writer(file);

    writer.write_record([
        "question_id",
        "paper_id",
        "paper_index",
        "question_no",
        "category",
        "status",
        "edition",
        "paper_type",
        "question_tex_object_id",
        "answer_tex_object_id",
        "search_text",
        "tags",
    ])?;

    for row in &rows {
        let answer_tex: Option<String> = row.get("answer_tex_object_id");
        writer.write_record([
            row.get::<String, _>("question_id"),
            row.get::<String, _>("paper_id"),
            row.get::<i32, _>("paper_index").to_string(),
            row.get::<Option<String>, _>("question_no").unwrap_or_default(),
            row.get::<String, _>("category"),
            row.get::<String, _>("status"),
            row.get::<String, _>("edition"),
            row.get::<String, _>("paper_type"),
            row.get::<Option<String>, _>("question_tex_object_id").unwrap_or_default(),
            if include_answers {
                answer_tex.unwrap_or_default()
            } else {
                String::new()
            },
            row.get::<Option<String>, _>("search_text").unwrap_or_default(),
            row.get::<Value, _>("tags_json").to_string(),
        ])?;
    }

    writer.flush().context("flush csv writer failed")?;
    Ok(rows.len())
}

async fn object_exists(pool: &PgPool, object_id: &str) -> Result<bool> {
    Ok(query("SELECT 1 FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("check object existence failed: {object_id}"))?
        .is_some())
}

async fn object_blob_nonempty(pool: &PgPool, object_id: &str) -> Result<bool> {
    let row = query("SELECT octet_length(content) AS size FROM object_blobs WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("check object blob failed: {object_id}"))?;

    Ok(row
        .and_then(|r| r.try_get::<Option<i32>, _>("size").ok().flatten())
        .unwrap_or(0)
        > 0)
}

async fn build_quality_report(pool: &PgPool) -> Result<QualityReport> {
    let mut report = QualityReport {
        missing_question_tex_object: Vec::new(),
        missing_question_tex_source: Vec::new(),
        missing_answer_tex_object: Vec::new(),
        missing_answer_tex_source: Vec::new(),
        missing_paper_tex_object: Vec::new(),
        missing_paper_tex_source: Vec::new(),
        missing_assets_object: Vec::new(),
        missing_workbook_blob: Vec::new(),
        duplicate_question_numbers: Vec::new(),
    };

    let question_rows = query(
        "SELECT question_id, question_tex_object_id::text AS question_tex_object_id, answer_tex_object_id::text AS answer_tex_object_id FROM questions",
    )
    .fetch_all(pool)
    .await
    .context("query questions for quality report failed")?;

    for row in question_rows {
        let question_id: String = row.get("question_id");
        let question_tex_object_id: Option<String> = row.get("question_tex_object_id");
        let answer_tex_object_id: Option<String> = row.get("answer_tex_object_id");

        if let Some(object_id) = question_tex_object_id.as_deref() {
            if !object_exists(pool, object_id).await? {
                report.missing_question_tex_object.push(question_id.clone());
            } else if !object_blob_nonempty(pool, object_id).await? {
                report.missing_question_tex_source.push(question_id.clone());
            }
        } else {
            report.missing_question_tex_object.push(question_id.clone());
        }

        if let Some(object_id) = answer_tex_object_id.as_deref() {
            if !object_exists(pool, object_id).await? {
                report.missing_answer_tex_object.push(question_id.clone());
            } else if !object_blob_nonempty(pool, object_id).await? {
                report.missing_answer_tex_source.push(question_id.clone());
            }
        }
    }

    let paper_rows = query(
        "SELECT paper_id, paper_tex_object_id::text AS paper_tex_object_id FROM papers",
    )
    .fetch_all(pool)
    .await
    .context("query papers for quality report failed")?;

    for row in paper_rows {
        let paper_id: String = row.get("paper_id");
        let paper_tex_object_id: Option<String> = row.get("paper_tex_object_id");
        if let Some(object_id) = paper_tex_object_id.as_deref() {
            if !object_exists(pool, object_id).await? {
                report.missing_paper_tex_object.push(paper_id.clone());
            } else if !object_blob_nonempty(pool, object_id).await? {
                report.missing_paper_tex_source.push(paper_id.clone());
            }
        } else {
            report.missing_paper_tex_object.push(paper_id.clone());
        }
    }

    let duplicate_rows = query(
        r#"
        SELECT paper_id, question_no, COUNT(*) AS duplicate_count
        FROM questions
        GROUP BY paper_id, question_no
        HAVING COUNT(*) > 1
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query duplicate question numbers failed")?;

    report.duplicate_question_numbers = duplicate_rows
        .into_iter()
        .map(|row| {
            json!({
                "paper_id": row.get::<String, _>("paper_id"),
                "question_no": row.get::<Option<String>, _>("question_no"),
                "duplicate_count": row.get::<i64, _>("duplicate_count"),
            })
        })
        .collect();

    let asset_rows = query(
        "SELECT asset_id, question_id, object_id::text AS object_id FROM question_assets",
    )
    .fetch_all(pool)
    .await
    .context("query question assets for quality report failed")?;

    for row in asset_rows {
        let object_id: String = row.get("object_id");
        if !object_exists(pool, &object_id).await? {
            report.missing_assets_object.push(json!({
                "asset_id": row.get::<String, _>("asset_id"),
                "question_id": row.get::<String, _>("question_id"),
                "object_id": object_id,
            }));
        }
    }

    let workbook_rows = query(
        "SELECT workbook_id, workbook_object_id::text AS workbook_object_id FROM score_workbooks",
    )
    .fetch_all(pool)
    .await
    .context("query score workbooks for quality report failed")?;

    for row in workbook_rows {
        let workbook_id: String = row.get("workbook_id");
        let object_id: String = row.get("workbook_object_id");
        if !object_blob_nonempty(pool, &object_id).await? {
            report.missing_workbook_blob.push(workbook_id);
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_score_rows, count_question_binds, count_score_workbook_binds, derive_difficulty,
        inspect_bundle, normalize_search_text, QuestionsParams, ScoreWorkbookParams,
    };
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn question_query_normalizes_limit_offset_and_counts_binds() {
        let params = QuestionsParams {
            edition: Some("18".into()),
            paper_id: Some("CPHOS-18-REGULAR".into()),
            paper_type: Some("regular".into()),
            category: Some("theory".into()),
            has_assets: Some(true),
            has_answer: Some(false),
            min_avg_score: Some(1.5),
            max_avg_score: Some(4.5),
            tag: Some("mechanics".into()),
            q: Some("pendulum".into()),
            limit: Some(999),
            offset: Some(-10),
        };

        let query = params.build_query();
        assert_eq!(query.limit, 100);
        assert_eq!(query.offset, 0);
        assert_eq!(query.bind_count, count_question_binds(&params));
        assert!(query.sql.contains("q.tags_json @>"));
        assert!(query.sql.contains("EXISTS (SELECT 1 FROM question_assets"));
        assert!(query.sql.contains("q.answer_tex_object_id IS NULL"));
    }

    #[test]
    fn score_workbook_query_counts_optional_filters() {
        let params = ScoreWorkbookParams {
            paper_id: Some("CPHOS-18-REGULAR".into()),
            exam_session: None,
        };
        let (sql, bind_count) = params.build_query();
        assert!(sql.contains("FROM score_workbooks"));
        assert_eq!(bind_count, count_score_workbook_binds(&params));
    }

    #[test]
    fn normalize_search_text_removes_latex_noise() {
        let normalized = normalize_search_text(
            &[Some("\\alpha + x_{1} with  spaces"), Some("line\\beta")],
            1000,
        );
        assert!(!normalized.contains("\\beta"));
        assert!(!normalized.contains("\\alpha"));
        assert!(normalized.contains("with spaces"));
    }

    #[test]
    fn aggregate_score_rows_groups_and_sorts_rows() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let csv_path = std::env::temp_dir().join(format!("qb_stats_test_{unique}.csv"));
        let csv_content = "question_id,exam_session,score,max_score\nQ1,s1,8,10\nQ1,s1,10,10\nQ2,s1,0,10\nQ2,s1,5,10\n";
        fs::write(&csv_path, csv_content).expect("write temp csv");

        let rows = aggregate_score_rows(&csv_path).expect("aggregate should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].question_id, "Q1");
        assert_eq!(rows[0].participant_count, 2);
        assert!((rows[0].avg_score - 9.0).abs() < 1e-9);
        assert_eq!(rows[1].question_id, "Q2");
        assert!((rows[1].zero_score_rate - 0.5).abs() < 1e-9);

        let _ = fs::remove_file(&csv_path);
    }

    #[test]
    fn derive_difficulty_matches_expected_range() {
        let score = derive_difficulty(4.5, 10.0, 0.25, 0.0).expect("derive difficulty");
        assert!((score - 0.565).abs() < 1e-9);
        assert!(derive_difficulty(1.0, 0.0, 0.0, 0.0).is_err());
    }

    #[test]
    fn inspect_bundle_accepts_demo_bundle() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bundle = root.join("samples/demo_bundle");
        let (validation, loaded) = inspect_bundle(&bundle).expect("inspect bundle");
        assert!(validation.errors.is_empty());
        assert!(loaded.is_some());
        let loaded = loaded.expect("loaded bundle");
        assert_eq!(loaded.manifest.paper.paper_id, "CPHOS-18-REGULAR-DEMO");
        assert_eq!(loaded.questions.len(), 3);
    }
}
