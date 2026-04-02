// ============================================================
// 文件：src/api/questions/handlers.rs
// 说明：题目管理的 HTTP 请求处理器
// ============================================================

//! 题目管理 API 的请求处理器
//!
//! 实现题目的查询、创建、详情、更新、删除、文件替换等 HTTP 端点

// 导入 anyhow 错误处理库
use anyhow::{anyhow, Context};

// 导入 Axum Web 框架的类型
use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},  // 请求提取器
    http::StatusCode,                                        // HTTP 状态码
    Json,                                                    // JSON 响应
};

// 导入 SQLx 数据库操作
use sqlx::{query, Row};

// 导入 UUID 库
use uuid::Uuid;

// 导入当前模块的子模块
use super::{
    // 导入功能：题目 ZIP 导入和文件替换
    imports::{import_question_zip, replace_question_zip, MAX_UPLOAD_BYTES},
    // 导入数据模型
    models::{
        QuestionDeleteResponse, QuestionDetail, QuestionDifficulty, QuestionFileReplaceResponse,
        QuestionImportResponse, QuestionPaperRef, QuestionSummary, QuestionsParams,
        UpdateQuestionMetadataRequest,
    },
    // 导入查询构建函数
    queries::{
        execute_questions_query, load_question_difficulties, load_question_files,
        load_question_tags, map_question_detail, map_question_paper_ref, map_question_summary,
        validate_question_filters,
    },
};

// 导入 API 共享模块
use crate::api::{
    shared::error::{ApiError, ApiResult},      // 错误类型和结果
    shared::utils::normalize_bundle_description, // 描述规范化
    AppState,                                   // 应用共享状态
};

// ============================================================
// list_questions 函数
// ============================================================
/// 获取题目列表
///
/// # 参数
/// - params: 查询参数（支持分类、标签、难度、搜索等过滤）
/// - state: 应用状态（包含数据库连接池）
///
/// # 返回
/// - Ok: 题目摘要列表
/// - Err: HTTP 状态码错误
///
/// # 查询参数示例
/// ?category=T&difficulty_tag=human&difficulty_min=5&limit=10&offset=0
pub(crate) async fn list_questions(
    // Query 提取器：从 URL 查询字符串解析参数
    Query(params): Query<QuestionsParams>,
    // State 提取器：获取共享的应用状态
    State(state): State<AppState>,
) -> Result<Json<Vec<QuestionSummary>>, StatusCode> {
    // 步骤 1: 验证查询参数
    // 检查分类、难度范围等是否合法
    validate_question_filters(&params).map_err(|_| StatusCode::BAD_REQUEST)?;

    // 步骤 2: 构建查询计划
    // 根据参数动态生成 SQL 查询
    let plan = params.build_query();

    // 步骤 3: 执行查询获取题目行
    let rows = execute_questions_query(&state.pool, &params, &plan)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 步骤 4: 预分配结果向量容量，避免多次内存分配
    let mut questions = Vec::with_capacity(rows.len());

    // 步骤 5: 遍历每道题目，加载关联数据
    for row in rows {
        // 获取题目 ID
        let question_id: String = row.get("question_id");

        // 加载题目标签（一对多关系）
        let tags = load_question_tags(&state.pool, &question_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // 加载题目难度（一对多关系）
        let difficulty = load_question_difficulties(&state.pool, &question_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // 将数据库行映射为 QuestionSummary 结构体
        questions.push(map_question_summary(row, tags, difficulty));
    }

    // 返回 JSON 响应
    Ok(Json(questions))
}

// ============================================================
// get_question_detail 函数
// ============================================================
/// 获取题目详情
///
/// # 参数
/// - question_id: 题目 UUID（从路径参数提取）
/// - state: 应用状态
///
/// # 返回
/// - Ok: 题目详情（含资源文件、关联试卷）
/// - Err: 400（无效 UUID）或 404（题目不存在）
pub(crate) async fn get_question_detail(
    // AxumPath 提取器：从路径 /:question_id 提取值
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<QuestionDetail>, StatusCode> {
    // 验证 UUID 格式
    Uuid::parse_str(&question_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // 获取题目详情并转换错误类型
    fetch_question_detail(&state, &question_id)
        .await
        .map(Json)  // 将 QuestionDetail 包装为 Json
        .map_err(map_question_detail_error)  // 转换错误为 HTTP 状态码
}

// ============================================================
// create_question 函数
// ============================================================
/// 创建新题目
///
/// # 请求格式
/// Content-Type: multipart/form-data
/// - file: ZIP 文件（包含 problem.tex 和 assets/目录）
/// - description: 题目描述（文本）
/// - difficulty: 难度定义（JSON 字符串）
///
/// # 返回
/// 导入结果（题目 ID、资源数量、状态）
pub(crate) async fn create_question(
    State(state): State<AppState>,
    // Multipart 提取器：解析 multipart/form-data 请求
    mut multipart: Multipart,
) -> ApiResult<QuestionImportResponse> {
    // 存储从 multipart 表单中解析的字段
    let mut file_name = None;      // ZIP 文件名
    let mut description = None;    // 题目描述
    let mut difficulty = None;     // 难度定义
    let mut bytes = Vec::new();    // ZIP 文件字节

    // 遍历 multipart 表单的每个字段
    while let Some(field) = multipart
        .next_field()  // 获取下一个字段
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
                // 保存文件名
                file_name = field.file_name().map(str::to_string);
                // 读取文件字节
                bytes = field
                    .bytes()
                    .await
                    .map_err(|err| {
                        ApiError::bad_request(format!("read uploaded file failed: {err}"))
                    })?
                    .to_vec();
            }
            // description 字段：题目描述
            "description" => {
                let value = field.text().await.map_err(|err| {
                    ApiError::bad_request(format!("read description field failed: {err}"))
                })?;
                description = Some(value);
            }
            // difficulty 字段：难度定义（JSON 格式）
            "difficulty" => {
                let value = field.text().await.map_err(|err| {
                    ApiError::bad_request(format!("read difficulty field failed: {err}"))
                })?;
                difficulty = Some(value);
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

    // 验证并规范化 description 字段
    let description = description
        .ok_or_else(|| {
            ApiError::bad_request("multipart form must include a non-empty 'description' field")
        })
        .and_then(|value| {
            normalize_bundle_description("description", &value)
                .map_err(|err| ApiError::bad_request(err.to_string()))
        })?;

    // 验证并规范化 difficulty 字段
    let difficulty = difficulty
        .ok_or_else(|| {
            ApiError::bad_request("multipart form must include a non-empty 'difficulty' field")
        })
        .and_then(|value| {
            // 解析 JSON 为 QuestionDifficulty
            serde_json::from_str::<QuestionDifficulty>(&value)
                .map_err(|err| ApiError::bad_request(format!("invalid difficulty field: {err}")))
                .and_then(|difficulty| {
                    // 规范化难度（验证分数范围、必须有 human 标签等）
                    difficulty
                        .normalize()
                        .map_err(|err| ApiError::bad_request(err.to_string()))
                })
        })?;

    // 验证：ZIP 文件大小不超过 20 MiB
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_request("uploaded zip exceeds 20 MiB limit"));
    }

    // 执行题目导入
    Ok(Json(
        import_question_zip(
            &state.pool,           // 数据库连接池
            file_name.as_deref(),  // 文件名
            &description,          // 描述
            &difficulty,           // 难度
            bytes,                 // ZIP 字节
        )
        .await
        .map_err(ApiError::from)?,  // 转换错误类型
    ))
}

// ============================================================
// replace_question_file 函数
// ============================================================
/// 替换题目文件（重新上传 ZIP）
///
/// # 请求
/// PUT /questions/:question_id/file
/// Content-Type: multipart/form-data
/// - file: 新的 ZIP 文件
pub(crate) async fn replace_question_file(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<QuestionFileReplaceResponse> {
    // 验证题目 ID 是合法的 UUID
    Uuid::parse_str(&question_id)
        .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;

    // 从 multipart 中读取上传的文件
    let (file_name, bytes) = read_uploaded_file_from_multipart(&mut multipart).await?;

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
        replace_question_zip(&state.pool, &question_id, file_name.as_deref(), bytes)
            .await
            .map_err(map_question_file_replace_error)?,  // 转换错误
    ))
}

// ============================================================
// update_question_metadata 函数
// ============================================================
/// 更新题目元数据
///
/// # 请求
/// PATCH /questions/:question_id
/// Content-Type: application/json
///
/// # 可更新字段
/// - category: 分类（none/T/E）
/// - description: 描述
/// - tags: 标签列表
/// - status: 状态（none/reviewed/used）
/// - difficulty: 难度定义
pub(crate) async fn update_question_metadata(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
    // Json 提取器：解析 JSON 请求体
    Json(request): Json<UpdateQuestionMetadataRequest>,
) -> ApiResult<QuestionDetail> {
    // 验证题目 ID 格式
    Uuid::parse_str(&question_id)
        .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;

    // 规范化请求数据（验证分类、状态、标签去重等）
    let update = request
        .normalize()
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    // 开启数据库事务
    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin question metadata update tx failed")?;

    // 检查题目是否存在
    let exists = query("SELECT 1 FROM questions WHERE question_id = $1::uuid")
        .bind(&question_id)
        .fetch_optional(&mut *tx)  // 获取单行或 None
        .await
        .context("check question existence failed")?
        .is_some();
    if !exists {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("question not found: {question_id}"),
        });
    }

    // ========== 更新分类 ==========
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

    // ========== 更新描述 ==========
    if let Some(description) = &update.description {
        query(
            "UPDATE questions SET description = $2, updated_at = NOW() WHERE question_id = $1::uuid",
        )
            .bind(&question_id)
            .bind(description)
            .execute(&mut *tx)
            .await
            .context("update question description failed")?;
    }

    // ========== 更新状态 ==========
    if let Some(status) = &update.status {
        query("UPDATE questions SET status = $2, updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .bind(status)
            .execute(&mut *tx)
            .await
            .context("update question status failed")?;
    }

    // ========== 更新难度（先删除后插入） ==========
    if let Some(difficulty) = &update.difficulty {
        // 删除旧难度
        query("DELETE FROM question_difficulties WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("replace question difficulties failed")?;

        // 插入新难度
        for (algorithm_tag, value) in difficulty {
            query(
                "INSERT INTO question_difficulties (question_id, algorithm_tag, score, notes) VALUES ($1::uuid, $2, $3, $4)",
            )
            .bind(&question_id)
            .bind(algorithm_tag)
            .bind(value.score)
            .bind(value.notes.as_deref())
            .execute(&mut *tx)
            .await
            .with_context(|| format!("insert updated question difficulty failed: {algorithm_tag}"))?;
        }

        // 更新时间戳
        query("UPDATE questions SET updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("touch question updated_at after difficulty update failed")?;
    }

    // ========== 更新标签（先删除后插入） ==========
    if let Some(tags) = &update.tags {
        // 删除旧标签
        query("DELETE FROM question_tags WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("replace question tags failed")?;

        // 插入新标签（带排序索引）
        for (idx, tag) in tags.iter().enumerate() {
            query("INSERT INTO question_tags (question_id, tag, sort_order) VALUES ($1::uuid, $2, $3)")
                .bind(&question_id)
                .bind(tag)
                .bind(i32::try_from(idx).unwrap_or(i32::MAX))  // 索引转 i32
                .execute(&mut *tx)
                .await
                .with_context(|| format!("insert updated question tag failed: {tag}"))?;
        }

        // 更新时间戳
        query("UPDATE questions SET updated_at = NOW() WHERE question_id = $1::uuid")
            .bind(&question_id)
            .execute(&mut *tx)
            .await
            .context("touch question updated_at after tag update failed")?;
    }

    // 提交事务
    tx.commit()
        .await
        .context("commit question metadata update failed")?;

    // 返回更新后的题目详情
    fetch_question_detail(&state, &question_id)
        .await
        .map(Json)
        .map_err(|err| ApiError {
            status: map_question_detail_error(err),
            message: "load updated question detail failed".to_string(),
        })
}

// ============================================================
// delete_question 函数
// ============================================================
/// 删除题目
///
/// # 说明
/// 使用 SQL 外键级联删除，自动清理关联的 question_files、question_tags 等
pub(crate) async fn delete_question(
    AxumPath(question_id): AxumPath<String>,
    State(state): State<AppState>,
) -> ApiResult<QuestionDeleteResponse> {
    // 验证题目 ID 格式
    Uuid::parse_str(&question_id)
        .map_err(|_| ApiError::bad_request(format!("invalid question_id: {question_id}")))?;

    // 执行删除
    let result = query("DELETE FROM questions WHERE question_id = $1::uuid")
        .bind(&question_id)
        .execute(&state.pool)
        .await
        .context("delete question failed")?;

    // 检查是否有行被删除（判断题目是否存在）
    if result.rows_affected() == 0 {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("question not found: {question_id}"),
        });
    }

    // 返回删除结果
    Ok(Json(QuestionDeleteResponse {
        question_id,
        status: "deleted",
    }))
}

// ============================================================
// fetch_question_detail 辅助函数
// ============================================================
/// 获取题目详情（内部使用）
///
/// # 返回
/// 包含完整信息的 QuestionDetail：
/// - 基本信息（ID、分类、状态、描述）
/// - TeX 源文件
/// - 资源文件列表
/// - 标签列表
/// - 难度定义
/// - 关联试卷
async fn fetch_question_detail(
    state: &AppState,
    question_id: &str,
) -> Result<QuestionDetail, anyhow::Error> {
    // 步骤 1: 查询题目基本信息
    let row = query(
        r#"
        SELECT question_id::text AS question_id, source_tex_path, category, status,
               COALESCE(description, '') AS description,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        WHERE question_id = $1::uuid
        "#,
    )
    .bind(question_id)
    .fetch_optional(&state.pool)  // 返回 Option<PgRow>
    .await
    .context("load question detail failed")?
    .ok_or_else(|| anyhow!("question not found: {question_id}"))?;  // None 转为错误

    // 步骤 2: 加载 TeX 源文件（必须存在）
    let tex_files = load_question_files(&state.pool, question_id, "tex")
        .await
        .context("load question tex files failed")?;
    let tex_object_id = tex_files
        .first()
        .map(|file| file.object_id.clone())
        .ok_or_else(|| anyhow!("question is missing a tex object: {question_id}"))?;

    // 步骤 3: 加载资源文件
    let assets = load_question_files(&state.pool, question_id, "asset")
        .await
        .context("load question assets failed")?;

    // 步骤 4: 加载标签
    let tags = load_question_tags(&state.pool, question_id)
        .await
        .context("load question tags failed")?;

    // 步骤 5: 加载难度
    let difficulty = load_question_difficulties(&state.pool, question_id)
        .await
        .context("load question difficulties failed")?;

    // 步骤 6: 加载关联试卷
    let papers = query(
        r#"
        SELECT p.paper_id::text AS paper_id, p.description, p.title, p.subtitle, pq.sort_order
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
    .map(map_question_paper_ref)  // 映射为 QuestionPaperRef
    .collect::<Vec<QuestionPaperRef>>();

    // 组装并返回 QuestionDetail
    Ok(map_question_detail(
        row,
        tex_object_id,
        tags,
        difficulty,
        assets,
        papers,
    ))
}

// ============================================================
// map_question_detail_error 函数
// ============================================================
/// 将题目详情错误映射为 HTTP 状态码
///
/// # 参数
/// - err: anyhow::Error
///
/// # 返回
/// - 404: 题目不存在
/// - 500: 其他服务器错误
fn map_question_detail_error(err: anyhow::Error) -> StatusCode {
    if err.to_string().starts_with("question not found:") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

// ============================================================
// read_uploaded_file_from_multipart 辅助函数
// ============================================================
/// 从 multipart 表单中读取上传的文件
///
/// # 返回
/// - (文件名，字节数据)
async fn read_uploaded_file_from_multipart(
    multipart: &mut Multipart,
) -> Result<(Option<String>, Vec<u8>), ApiError> {
    let mut file_name = None;
    let mut bytes = Vec::new();

    // 遍历所有字段，查找名为"file"的字段
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("read multipart field failed: {err}")))?
    {
        // 跳过非 file 字段
        if field.name() != Some("file") {
            continue;
        }

        // 保存文件名
        file_name = field.file_name().map(str::to_string);
        // 读取文件字节
        bytes = field
            .bytes()
            .await
            .map_err(|err| ApiError::bad_request(format!("read uploaded file failed: {err}")))?
            .to_vec();
    }

    Ok((file_name, bytes))
}

// ============================================================
// map_question_file_replace_error 函数
// ============================================================
/// 将文件替换错误映射为适当的 ApiError
///
/// # 错误分类
/// - 404: 题目不存在
/// - 400: 客户端错误（文件格式、大小、路径安全等）
/// - 500: 服务器内部错误
fn map_question_file_replace_error(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.starts_with("question not found:") {
        // 题目不存在 → 404
        ApiError {
            status: StatusCode::NOT_FOUND,
            message,
        }
    } else if message.contains("uploaded file is empty")
        || message.contains("uploaded zip exceeds")
        || message.contains("open zip archive failed")
        || message.contains("zip expands beyond the allowed uncompressed size")
        || message.contains("zip entry")
        || message.contains("zip root")
        || message.contains("unsafe path")
        || message.contains("all non-root files must be inside")
        || message.contains("unexpected file")
    {
        // 客户端上传的文件有问题 → 400
        ApiError::bad_request(message)
    } else {
        // 其他错误 → 500
        ApiError::from(err)
    }
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. Axum 提取器 (Extractors)
 *    - Query<T>: 解析 URL 查询参数 (?key=value)
 *    - Path<T>: 解析路径参数 (/:id)
 *    - State<T>: 获取共享状态
 *    - Json<T>: 解析/返回 JSON
 *    - Multipart: 解析 multipart/form-data
 *
 * 2. 错误处理模式
 *    .map_err(|_| StatusCode::...)?   // 简单错误
 *    .map_err(ApiError::from)?        // 转换错误类型
 *    return Err(ApiError::...)        // 提前返回
 *
 * 3. 事务处理
 *    pool.begin() → 开启事务
 *    tx.execute() → 执行 SQL
 *    tx.commit()  → 提交事务（或自动回滚）
 *
 * 4. 条件更新模式
 *    if let Some(value) = &update.field {
 *        query(...).bind(value).execute().await?;
 *    }
 *    只更新请求中提供的字段
 *
 * 5. 一对多关系加载
 *    主查询获取基本信息
 *    循环加载关联数据（标签、难度、文件等）
 *
 * ============================================================
 * 题目 API Handler 流程图
 * ============================================================
 *
 * list_questions:
 *   验证参数 → 构建 SQL → 执行 → 加载标签/难度 → 映射响应
 *
 * get_question_detail:
 *   验证 UUID → 查询基本信息 → 加载文件/标签/难度/试卷 → 返回详情
 *
 * create_question:
 *   解析 multipart → 验证字段 → 导入 ZIP → 插入数据库 → 返回结果
 *
 * update_question_metadata:
 *   验证 UUID → 开启事务 → 条件更新各字段 → 提交 → 返回新详情
 *
 * delete_question:
 *   验证 UUID → 执行 DELETE → 检查影响行数 → 返回结果
 *
 * ============================================================
 * HTTP 端点与 Handler 对应关系
 * ============================================================
 *
 * GET    /questions              → list_questions
 * POST   /questions              → create_question
 * GET    /questions/:id          → get_question_detail
 * PATCH  /questions/:id          → update_question_metadata
 * DELETE /questions/:id          → delete_question
 * PUT    /questions/:id/file     → replace_question_file
 *
 */
