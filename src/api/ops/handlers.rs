// ============================================================
// 文件：src/api/ops/handlers.rs
// 说明：运维操作的 HTTP 请求处理器
// ============================================================

//! 运维操作 API 的请求处理器
//!
//! 实现批量打包、数据导出、质量检查等 HTTP 端点

// 导入标准库类型
use std::{fs, path::Path};

// 导入 anyhow 错误处理
use anyhow::Context;

// 导入 Axum Web 框架类型
use axum::{extract::State, response::Response, Json};

// 导入 Serde JSON
use serde_json::json;

// 导入当前模块的子模块
use super::{
    // 打包功能
    bundles::{build_paper_bundle_response, build_question_bundle_response},
    // 导出功能
    exports::{default_export_path, ensure_parent_dir, export_csv, export_jsonl, exported_path},
    // 数据模型
    models::{
        ExportFormat, ExportRequest, ExportResponse, PaperBundleRequest, QualityCheckRequest,
        QuestionBundleRequest,
    },
    // 质量检查
    quality::build_quality_report,
};

// 导入 API 共享模块
use crate::api::{
    shared::{
        error::{ApiError, ApiResult},
        utils::{canonical_or_original, expand_path},
    },
    AppState,  // 应用共享状态
};

// ============================================================
// download_questions_bundle 函数
// ============================================================
/// 批量下载题目包
///
/// # 请求
/// POST /questions/bundles
/// Content-Type: application/json
/// Body: { question_ids: ["uuid1", "uuid2", ...] }
///
/// # 返回
/// - ZIP 文件（application/zip）
/// - 包含 manifest.json 和各题目目录
pub(crate) async fn download_questions_bundle(
    State(state): State<AppState>,
    Json(request): Json<QuestionBundleRequest>,
) -> Result<Response, ApiError> {
    // 规范化并验证请求（UUID 格式、去重等）
    let question_ids = request
        .normalize()
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    // 构建题目打包响应
    build_question_bundle_response(&state.pool, &question_ids)
        .await
        .map_err(map_bundle_error)  // 转换错误类型
}

// ============================================================
// download_papers_bundle 函数
// ============================================================
/// 批量下载试卷包
///
/// # 请求
/// POST /papers/bundles
/// Content-Type: application/json
/// Body: { paper_ids: ["uuid1", "uuid2", ...] }
///
/// # 返回
/// - ZIP 文件（application/zip）
/// - 包含 manifest.json、渲染后的 main.tex、资源文件、附录
pub(crate) async fn download_papers_bundle(
    State(state): State<AppState>,
    Json(request): Json<PaperBundleRequest>,
) -> Result<Response, ApiError> {
    // 规范化并验证请求
    let paper_ids = request
        .normalize()
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    // 构建试卷打包响应
    build_paper_bundle_response(&state.pool, &paper_ids)
        .await
        .map_err(map_bundle_error)
}

// ============================================================
// run_export 函数
// ============================================================
/// 运行数据导出
///
/// # 请求
/// POST /exports/run
/// Content-Type: application/json
/// Body: { format: "jsonl"|"csv", public: bool, output_path: string }
///
/// # 导出格式
/// - jsonl: 每行一个 JSON 对象，适合程序处理
/// - csv: 逗号分隔值，适合 Excel 打开
///
/// # public 参数
/// - true: 不包含 TeX 源码（公开版本）
/// - false: 包含 TeX 源码（内部版本）
pub(crate) async fn run_export(
    State(state): State<AppState>,
    Json(request): Json<ExportRequest>,
) -> ApiResult<ExportResponse> {
    // 确定输出路径
    let output_path = request
        .output_path
        .as_deref()
        .map(expand_path)  // 展开 ~ 为家目录
        .unwrap_or_else(|| default_export_path(request.format, request.public));

    // 确保父目录存在
    ensure_parent_dir(&output_path, "export")?;

    // 根据格式执行导出
    let exported_count = match request.format {
        ExportFormat::Jsonl => export_jsonl(&state.pool, &output_path, request.public).await?,
        ExportFormat::Csv => export_csv(&state.pool, &output_path, request.public).await?,
    };

    // 返回导出响应
    Ok(Json(ExportResponse {
        format: match request.format {
            ExportFormat::Jsonl => "jsonl",
            ExportFormat::Csv => "csv",
        },
        public: request.public,
        output_path: exported_path(&output_path),  // 返回规范化路径
        exported_questions: exported_count,
    }))
}

// ============================================================
// run_quality_check 函数
// ============================================================
/// 运行数据质量检查
///
/// # 请求
/// POST /quality-checks/run
/// Content-Type: application/json
/// Body: { output_path: string }  (可选)
///
/// # 检查项目
/// - missing_tex_object: 缺少 TeX 对象的题目
/// - missing_tex_source: TeX 内容为空的题目
/// - missing_asset_objects: 缺失的资源文件
/// - empty_papers: 不包含任何题目的试卷
pub(crate) async fn run_quality_check(
    State(state): State<AppState>,
    Json(request): Json<QualityCheckRequest>,
) -> ApiResult<serde_json::Value> {
    // 确定报告输出路径
    let output_path = request
        .output_path
        .as_deref()
        .map(expand_path)
        .unwrap_or_else(|| std::path::PathBuf::from("exports/quality_report.json"));

    // 构建质量报告
    let report = build_quality_report(&state.pool).await?;

    // 确保父目录存在
    ensure_parent_dir(&output_path, "quality report")?;

    // 序列化报告为美化的 JSON
    let serialized =
        serde_json::to_string_pretty(&report).context("serialize quality report failed")?;

    // 写入文件
    fs::write(&output_path, serialized).with_context(|| {
        format!(
            "write quality report failed: {}",
            output_path.to_string_lossy()
        )
    })?;

    // 返回 JSON 响应
    Ok(Json(json!({
        "output_path": canonical_or_original(Path::new(&output_path)),
        "report": report,
    })))
}

// ============================================================
// map_bundle_error 函数
// ============================================================
/// 将打包错误映射为适当的 ApiError
///
/// # 错误分类
/// - 400: 客户端错误（无效 ID、重复 ID、不存在等）
/// - 500: 服务器内部错误
fn map_bundle_error(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.starts_with("question not found:")
        || message.starts_with("paper not found:")
        || message.starts_with("question_ids")
        || message.starts_with("paper_ids")
        || message.starts_with("invalid question_ids entry:")
        || message.starts_with("invalid paper_ids entry:")
        || message.starts_with("duplicate question_ids entry:")
        || message.starts_with("duplicate paper_ids entry:")
    {
        ApiError::bad_request(message)
    } else {
        ApiError::internal(message)
    }
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. State 提取器
 *    - 获取共享的应用状态 (AppState)
 *    - 通常包含数据库连接池
 *    - 在所有请求间共享
 *
 * 2. Json 提取器
 *    - 自动解析 JSON 请求体
 *    - 自动设置响应 Content-Type
 *    - 使用 serde 序列化/反序列化
 *
 * 3. Response 类型
 *    - 用于返回自定义响应
 *    - 可设置状态码、Header、Body
 *    - ZIP 下载等二进制响应使用
 *
 * 4. json! 宏
 *    - 快速构建 JSON 对象
 *    - 支持插值：json!({ "key": value })
 *    - 返回 serde_json::Value
 *
 * 5. 错误映射模式
 *    fn map_error(err: anyhow::Error) -> ApiError {
 *        if err.to_string().starts_with("xxx") {
 *            ApiError::bad_request(message)
 *        } else {
 *            ApiError::internal(message)
 *        }
 *    }
 *
 * ============================================================
 * 运维 API 端点一览
 * ============================================================
 *
 * POST /questions/bundles
 *   用途：批量下载题目
 *   请求：{ question_ids: [...] }
 *   响应：application/zip
 *
 * POST /papers/bundles
 *   用途：批量下载试卷
 *   请求：{ paper_ids: [...] }
 *   响应：application/zip（含渲染后的 LaTeX）
 *
 * POST /exports/run
 *   用途：导出题库数据
 *   请求：{ format: "jsonl", public: false, output_path: "/x" }
 *   响应：{ format, public, output_path, exported_questions }
 *
 * POST /quality-checks/run
 *   用途：数据质量检查
 *   请求：{ output_path: "/x" }
 *   响应：{ output_path, report }
 *
 * ============================================================
 * 导出格式对比
 * ============================================================
 *
 * JSONL (推荐用于程序处理):
 * {"question_id":"...", "category":"T", ...}
 * {"question_id":"...", "category":"E", ...}
 *
 * CSV (推荐用于 Excel 查看):
 * question_id,category,status,description,...
 * uuid1,T,reviewed,题目描述 1,...
 * uuid2,E,none,题目描述 2,...
 *
 */
