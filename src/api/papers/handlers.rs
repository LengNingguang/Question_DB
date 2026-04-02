// ============================================================
// 文件：src/api/papers/handlers.rs
// 说明：试卷管理的 HTTP 请求处理器
// ============================================================

//! 试卷管理 API 的请求处理器
//!
//! 实现试卷的查询、创建、详情、更新、删除、文件替换等 HTTP 端点

// 导入标准库集合类型
use std::collections::HashSet;

// 导入 anyhow 错误处理库
use anyhow::Context;

// 导入 Axum Web 框架类型
use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},  // 请求提取器
    http::StatusCode,                                        // HTTP 状态码
    Json,                                                    // JSON 响应
};

// 导入 SQLx 数据库操作
use sqlx::{query, Postgres, QueryBuilder, Row};

// 导入 UUID 库
use uuid::Uuid;

// 导入当前模块的子模块
use super::{
    // 导入功能：试卷 ZIP 导入和文件替换
    imports::{import_paper_zip, replace_paper_zip, MAX_UPLOAD_BYTES},
    // 导入数据模型
    models::{
        CreatePaperRequest, PaperDeleteResponse, PaperDetail, PaperFileReplaceResponse,
        PaperImportResponse, PapersParams, UpdatePaperRequest,
    },
    // 导入查询构建函数
    queries::{execute_papers_query, validate_and_build_papers_query},
};

// 导入题目模块的查询函数
use crate::api::questions::queries::{
    load_question_tags, map_paper_detail, map_paper_question_summary, map_paper_summary,
};

// 导入 API 共享模块
use crate::api::{
    shared::error::{ApiError, ApiResult},  // 错误类型和结果
    AppState,                               // 应用共享状态
};

// ============================================================
// list_papers 函数
// ============================================================
/// 获取试卷列表
///
/// # 参数
/// - params: 查询参数（支持题目 ID、分类、标签、搜索等过滤）
/// - state: 应用状态（包含数据库连接池）
///
/// # 返回
/// - Ok: 试卷摘要列表
/// - Err: HTTP 状态码错误
pub(crate) async fn list_papers(
    // Query 提取器：从 URL 查询字符串解析参数
    Query(params): Query<PapersParams>,
    // State 提取器：获取共享的应用状态
    State(state): State<AppState>,
) -> Result<Json<Vec<super::models::PaperSummary>>, StatusCode> {
    // 步骤 1: 验证并构建查询计划
    let plan = validate_and_build_papers_query(&params)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // 步骤 2: 执行查询获取试卷行
    let rows = execute_papers_query(&state.pool, &params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 步骤 3: 将数据库行映射为 PaperSummary 并收集为 Vec
    Ok(Json(rows.into_iter().map(map_paper_summary).collect()))
}

// ============================================================
// create_paper 函数
// ============================================================
/// 创建新试卷
///
/// # 请求格式
/// Content-Type: multipart/form-data
/// - file: ZIP 文件（附录/原始材料）
/// - description: 试卷描述（文本）
/// - title: 标题（文本）
/// - subtitle: 子标题（文本）
/// - authors: 作者列表（JSON 数组字符串）
/// - reviewers: 审核者列表（JSON 数组字符串）
/// - question_ids: 题目 ID 列表（JSON 数组字符串）
///
/// # 验证规则
/// - 题目必须都有相同分类（T 或 E）
/// - 题目状态必须是 reviewed 或 used
pub(crate) async fn create_paper(
    State(state): State<AppState>,
    // Multipart 提取器：解析 multipart/form-data 请求
    mut multipart: Multipart,
) -> ApiResult<PaperImportResponse> {
    // 存储从 multipart 表单中解析的字段
    let mut file_name = None;        // ZIP 文件名
    let mut description = None;      // 描述
    let mut title = None;            // 标题
    let mut subtitle = None;         // 子标题
    let mut authors = None;          // 作者列表
    let mut reviewers = None;        // 审核者列表
    let mut question_ids = None;     // 题目 ID 列表
    let mut bytes = Vec::new();      // ZIP 文件字节

    // 遍历 multipart 表单的每个字段
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("read multipart field failed: {err}")))?
    {
        // 获取字段名称
        let Some(name) = field.name() else {
            continue;  // 跳过无名字段
        };

        // 根据字段名分别处理
        match name {
            // file 字段：上传的 ZIP 文件
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
            // description 字段
            "description" => {
                description = Some(read_text_field(field, "description").await?);
            }
            // title 字段
            "title" => {
                title = Some(read_text_field(field, "title").await?);
            }
            // subtitle 字段
            "subtitle" => {
                subtitle = Some(read_text_field(field, "subtitle").await?);
            }
            // authors 字段：JSON 数组字符串
            "authors" => {
                authors = Some(read_json_string_list_field(field, "authors").await?);
            }
            // reviewers 字段：JSON 数组字符串
            "reviewers" => {
                reviewers = Some(read_json_string_list_field(field, "reviewers").await?);
            }
            // question_ids 字段：JSON 数组字符串
            "question_ids" => {
                question_ids = Some(read_json_string_list_field(field, "question_ids").await?);
            }
            // 忽略其他字段
            _ => {}
        }
    }

    // 验证：ZIP 文件不能为空
    if bytes.is_empty() {
        return Err(ApiError::bad_request(
            "multipart form must include a non-empty 'file' field",
        ));
    }
    // 验证：文件大小不超过 20 MiB
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_request("uploaded zip exceeds 20 MiB limit"));
    }

    // 组装创建请求
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
    // 规范化请求（验证、去重、修剪空白）
    .normalize()
    .map_err(|err| ApiError::bad_request(err.to_string()))?;

    // 验证题目 ID 格式
    validate_question_ids(&request.question_ids)?;
    // 验证题目是否满足试卷要求（分类一致、状态合法）
    ensure_paper_questions_valid(&state.pool, &request.question_ids).await?;

    // 执行试卷导入
    Ok(Json(
        import_paper_zip(&state.pool, file_name.as_deref(), &request, bytes)
            .await
            .map_err(map_paper_create_error)?,  // 转换错误
    ))
}

// ============================================================
// replace_paper_file 函数
// ============================================================
/// 替换试卷文件（重新上传 ZIP）
///
/// # 请求
/// PUT /papers/:paper_id/file
/// Content-Type: multipart/form-data
/// - file: 新的 ZIP 文件
pub(crate) async fn replace_paper_file(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<PaperFileReplaceResponse> {
    // 验证试卷 ID 是合法的 UUID
    Uuid::parse_str(&paper_id)
        .map_err(|_| ApiError::bad_request(format!("invalid paper_id: {paper_id}")))?;

    // 从 multipart 中读取上传的文件
    let mut file_name = None;
    let mut bytes = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("read multipart field failed: {err}")))?
    {
        // 只处理 file 字段
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

    // 验证：文件不能为空
    if bytes.is_empty() {
        return Err(ApiError::bad_request(
            "multipart form must include a non-empty 'file' field",
        ));
    }
    // 验证：文件大小限制
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_request("uploaded zip exceeds 20 MiB limit"));
    }

    // 执行文件替换
    Ok(Json(
        replace_paper_zip(&state.pool, &paper_id, file_name.as_deref(), bytes)
            .await
            .map_err(map_paper_file_replace_error)?,  // 转换错误
    ))
}

// ============================================================
// update_paper 函数
// ============================================================
/// 更新试卷元数据
///
/// # 请求
/// PATCH /papers/:paper_id
/// Content-Type: application/json
///
/// # 可更新字段
/// - description: 描述
/// - title: 标题
/// - subtitle: 子标题
/// - authors: 作者列表
/// - reviewers: 审核者列表
/// - question_ids: 题目 ID 列表（会更新顺序）
pub(crate) async fn update_paper(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
    // Json 提取器：解析 JSON 请求体
    Json(request): Json<UpdatePaperRequest>,
) -> ApiResult<PaperDetail> {
    // 验证试卷 ID 格式
    Uuid::parse_str(&paper_id)
        .map_err(|_| ApiError::bad_request(format!("invalid paper_id: {paper_id}")))?;

    // 规范化请求数据
    let update = request
        .normalize()
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    // 如果更新了题目 ID，先验证格式
    if let Some(question_ids) = &update.question_ids {
        validate_question_ids(question_ids)?;
    }

    // 开启数据库事务
    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin paper update tx failed")?;

    // 检查试卷是否存在
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

    // 确定最终使用的题目 ID 列表
    // 如果请求中提供了 question_ids，使用新的；否则保留原有的
    let final_question_ids = if let Some(question_ids) = &update.question_ids {
        question_ids.clone()
    } else {
        // 从数据库加载当前的题目 ID 列表
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

    // 验证题目是否满足试卷要求
    ensure_paper_questions_valid(&mut *tx, &final_question_ids).await?;

    // ========== 更新描述 ==========
    if let Some(description) = &update.description {
        query("UPDATE papers SET description = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(description)
            .execute(&mut *tx)
            .await
            .context("update paper description failed")?;
    }

    // ========== 更新标题 ==========
    if let Some(title) = &update.title {
        query("UPDATE papers SET title = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(title)
            .execute(&mut *tx)
            .await
            .context("update paper title failed")?;
    }

    // ========== 更新子标题 ==========
    if let Some(subtitle) = &update.subtitle {
        query("UPDATE papers SET subtitle = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(subtitle)
            .execute(&mut *tx)
            .await
            .context("update paper subtitle failed")?;
    }

    // ========== 更新作者列表 ==========
    if let Some(authors) = &update.authors {
        query("UPDATE papers SET authors = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(authors)
            .execute(&mut *tx)
            .await
            .context("update paper authors failed")?;
    }

    // ========== 更新审核者列表 ==========
    if let Some(reviewers) = &update.reviewers {
        query("UPDATE papers SET reviewers = $2, updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .bind(reviewers)
            .execute(&mut *tx)
            .await
            .context("update paper reviewers failed")?;
    }

    // ========== 更新题目列表（如果需要） ==========
    if let Some(question_ids) = &update.question_ids {
        // 删除旧的题目关联
        query("DELETE FROM paper_questions WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .execute(&mut *tx)
            .await
            .context("replace paper questions failed")?;

        // 插入新的题目关联（带排序索引）
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

        // 更新时间戳
        query("UPDATE papers SET updated_at = NOW() WHERE paper_id = $1::uuid")
            .bind(&paper_id)
            .execute(&mut *tx)
            .await
            .context("touch paper updated_at after question update failed")?;
    }

    // 提交事务
    tx.commit().await.context("commit paper update failed")?;

    // 返回更新后的试卷详情
    fetch_paper_detail(&state, &paper_id)
        .await
        .map(Json)
        .map_err(map_paper_detail_error)
}

// ============================================================
// delete_paper 函数
// ============================================================
/// 删除试卷
///
/// # 说明
/// 使用 SQL 外键级联删除，自动清理关联的 paper_questions
pub(crate) async fn delete_paper(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
) -> ApiResult<PaperDeleteResponse> {
    // 验证试卷 ID 格式
    Uuid::parse_str(&paper_id)
        .map_err(|_| ApiError::bad_request(format!("invalid paper_id: {paper_id}")))?;

    // 执行删除
    let result = query("DELETE FROM papers WHERE paper_id = $1::uuid")
        .bind(&paper_id)
        .execute(&state.pool)
        .await
        .context("delete paper failed")?;

    // 检查是否有行被删除
    if result.rows_affected() == 0 {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("paper not found: {paper_id}"),
        });
    }

    // 返回删除结果
    Ok(Json(PaperDeleteResponse {
        paper_id,
        status: "deleted",
    }))
}

// ============================================================
// get_paper_detail 函数
// ============================================================
/// 获取试卷详情
///
/// # 参数
/// - paper_id: 试卷 UUID（从路径参数提取）
///
/// # 返回
/// - Ok: 试卷详情（含题目列表）
/// - Err: 400（无效 UUID）或 404（试卷不存在）
pub(crate) async fn get_paper_detail(
    AxumPath(paper_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<PaperDetail>, StatusCode> {
    // 验证 UUID 格式
    Uuid::parse_str(&paper_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // 获取试卷详情并转换错误类型
    fetch_paper_detail(&state, &paper_id)
        .await
        .map(Json)
        .map_err(map_paper_detail_status)
}

// ============================================================
// fetch_paper_detail 辅助函数
// ============================================================
/// 获取试卷详情（内部使用）
///
/// # 返回
/// 包含完整信息的 PaperDetail：
/// - 基本信息（ID、描述、标题、作者、审核者）
/// - 题目列表（按 sort_order 排序）
async fn fetch_paper_detail(
    state: &AppState,
    paper_id: &str,
) -> Result<PaperDetail, ApiError> {
    // 步骤 1: 查询试卷基本信息
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

    // 步骤 2: 查询试卷包含的题目（按排序顺序）
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

    // 步骤 3: 为每个题目加载标签
    let mut questions = Vec::with_capacity(question_rows.len());
    for row in question_rows {
        let question_id: String = row.get("question_id");
        let tags = load_question_tags(&state.pool, &question_id)
            .await
            .context("load paper question tags failed")
            .map_err(ApiError::from)?;
        questions.push(map_paper_question_summary(row, tags));
    }

    // 组装并返回 PaperDetail
    Ok(map_paper_detail(paper_row, questions))
}

// ============================================================
// read_text_field 辅助函数
// ============================================================
/// 从 multipart 字段读取文本
async fn read_text_field(
    field: axum::extract::multipart::Field<'_>,
    field_name: &str,
) -> Result<String, ApiError> {
    field
        .text()
        .await
        .map_err(|err| ApiError::bad_request(format!("read {field_name} field failed: {err}")))
}

// ============================================================
// read_json_string_list_field 辅助函数
// ============================================================
/// 从 multipart 字段读取 JSON 字符串数组
///
/// # 说明
/// 前端发送 JSON 数组时需序列化为字符串，如：["a","b"]
async fn read_json_string_list_field(
    field: axum::extract::multipart::Field<'_>,
    field_name: &str,
) -> Result<Vec<String>, ApiError> {
    // 先读取文本
    let text = read_text_field(field, field_name).await?;
    // 解析 JSON 数组
    serde_json::from_str::<Vec<String>>(&text)
        .map_err(|err| ApiError::bad_request(format!("invalid {field_name} field: {err}")))
}

// ============================================================
// validate_question_ids 函数
// ============================================================
/// 验证题目 ID 列表
///
/// # 验证规则
/// - 每个 ID 必须是合法的 UUID
/// - 不能有重复
fn validate_question_ids(question_ids: &[String]) -> Result<(), ApiError> {
    let mut seen_question_ids = HashSet::new();
    for question_id in question_ids {
        // 检查重复
        if !seen_question_ids.insert(question_id.clone()) {
            return Err(ApiError::bad_request(format!(
                "duplicate question_id in question_ids: {question_id}"
            )));
        }
        // 验证 UUID 格式
        Uuid::parse_str(question_id)
            .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;
    }
    Ok(())
}

// ============================================================
// PaperQuestionValidationRow 结构体
// ============================================================
/// 试卷题目验证用的临时数据结构
#[derive(Debug, Clone, PartialEq, Eq)]
struct PaperQuestionValidationRow {
    question_id: String,
    category: String,
    status: String,
}

// ============================================================
// ensure_paper_questions_valid 函数
// ============================================================
/// 验证试卷中的题目是否满足要求
///
/// # 验证规则
/// 1. 所有题目必须存在
/// 2. 所有题目分类必须一致（都是 T 或都是 E）
/// 3. 所有题目状态必须是 reviewed 或 used
async fn ensure_paper_questions_valid<'e, E>(
    executor: E,
    question_ids: &[String],
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    // 加载题目验证数据
    let question_rows = load_paper_question_validation_rows(executor, question_ids).await?;
    // 执行验证
    validate_paper_question_rows(&question_rows)
}

// ============================================================
// load_paper_question_validation_rows 函数
// ============================================================
/// 加载题目验证数据
///
/// # 返回
/// - 题目 ID、分类、状态
async fn load_paper_question_validation_rows<'e, E>(
    executor: E,
    question_ids: &[String],
) -> Result<Vec<PaperQuestionValidationRow>, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    // 处理空列表情况
    if question_ids.is_empty() {
        return Ok(Vec::new());
    }

    // 构建动态 SQL 查询（使用 IN 子句）
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT q.question_id::text AS question_id, q.category, q.status FROM questions q WHERE q.question_id IN (",
    );
    // 添加绑定参数
    for (idx, question_id) in question_ids.iter().enumerate() {
        if idx > 0 {
            builder.push(", ");
        }
        builder.push_bind(question_id).push("::uuid");
    }
    builder.push(')');

    // 执行查询
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

    // 检查是否有题目不存在
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

// ============================================================
// validate_paper_question_rows 函数
// ============================================================
/// 验证题目行数据
///
/// # 验证规则
/// 1. 分类必须是 T 或 E（不能是 none）
/// 2. 所有题目分类必须一致
/// 3. 状态必须是 reviewed 或 used
fn validate_paper_question_rows(
    question_rows: &[PaperQuestionValidationRow],
) -> Result<(), ApiError> {
    let mut expected_category = None;

    for row in question_rows {
        // 验证分类
        match row.category.as_str() {
            "T" | "E" => {}
            other => {
                return Err(ApiError::bad_request(format!(
                    "question {} has category {other}; paper questions must all have category T or all have category E",
                    row.question_id
                )));
            }
        }

        // 验证分类一致性
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

        // 验证状态
        if !matches!(row.status.as_str(), "reviewed" | "used") {
            return Err(ApiError::bad_request(format!(
                "question {} has status {}; paper questions must all have status reviewed or used",
                row.question_id, row.status
            )));
        }
    }

    Ok(())
}

// ============================================================
// 错误映射函数
// ============================================================

/// 将试卷详情错误映射为 ApiError
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

/// 将试卷详情错误映射为 StatusCode
fn map_paper_detail_status(err: ApiError) -> StatusCode {
    err.status
}

/// 将试卷创建错误映射为 ApiError
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

/// 将试卷文件替换错误映射为 ApiError
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

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. multipart/form-data 处理
 *    - Multipart 提取器自动解析请求
 *    - .next_field().await 遍历字段
 *    - 字段名匹配确定字段类型
 *
 * 2. JSON 字符串数组处理
 *    - multipart 中 JSON 数组需序列化为字符串
 *    - 使用 serde_json::from_str 解析
 *
 * 3. 事务处理模式
 *    - pool.begin() 开启事务
 *    - tx.execute() 执行 SQL
 *    - tx.commit() 提交（或自动回滚）
 *
 * 4. 动态 SQL 构建
 *    - QueryBuilder 用于构建 IN 子句
 *    - 避免 SQL 注入
 *    - 循环添加绑定参数
 *
 * 5. 泛型约束
 *    - where E: sqlx::Executor<...>
 *    - 允许函数接受 pool 或 tx
 *
 * ============================================================
 * 试卷 Handler 流程图
 * ============================================================
 *
 * list_papers:
 *   验证参数 → 构建 SQL → 执行 → 映射响应
 *
 * create_paper:
 *   解析 multipart → 验证字段 → 规范化 → 验证题目 → 导入 → 返回结果
 *
 * get_paper_detail:
 *   验证 UUID → 查询基本信息 → 加载题目 → 加载标签 → 返回详情
 *
 * update_paper:
 *   验证 UUID → 开启事务 → 验证题目 → 条件更新 → 提交 → 返回新详情
 *
 * delete_paper:
 *   验证 UUID → 执行 DELETE → 检查影响行数 → 返回结果
 *
 * ============================================================
 * HTTP 端点与 Handler 对应关系
 * ============================================================
 *
 * GET    /papers              → list_papers
 * POST   /papers              → create_paper
 * GET    /papers/:id          → get_paper_detail
 * PATCH  /papers/:id          → update_paper
 * DELETE /papers/:id          → delete_paper
 * PUT    /papers/:id/file     → replace_paper_file
 *
 */
