// ============================================================
// 文件：src/api/papers/imports.rs
// 说明：试卷 ZIP 导入和文件替换逻辑
// ============================================================

//! 试卷 ZIP 文件导入 helpers
//!
//! 处理试卷 ZIP 包的上传、验证、存储等操作

// 导入标准库类型
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

// 导入 anyhow 错误处理
use anyhow::{bail, Context, Result};

// 导入 SQLx 数据库操作
use sqlx::{query, PgPool, Postgres, Row, Transaction};

// 导入 UUID 库
use uuid::Uuid;

// 导入 ZIP 处理库
use zip::ZipArchive;

// 导入当前模块的模型
use super::models::{NormalizedCreatePaperRequest, PaperFileReplaceResponse, PaperImportResponse};

// ============================================================
// 常量定义
// ============================================================
/// 上传文件大小限制：20 MiB
pub(crate) const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

// ============================================================
// import_paper_zip 函数
// ============================================================
/// 导入试卷 ZIP 文件
///
/// # 参数
/// - pool: 数据库连接池
/// - file_name: 上传的文件名
/// - request: 规范化后的创建请求
/// - zip_bytes: ZIP 文件字节
///
/// # 处理流程
/// 1. 验证 ZIP 格式
/// 2. 生成新的 UUID 作为试卷 ID
/// 3. 将 ZIP 作为二进制对象存储到 objects 表
/// 4. 插入试卷元数据到 papers 表
/// 5. 插入题目关联到 paper_questions 表
///
/// # 返回
/// 导入响应（试卷 ID、文件名、题目数量、状态）
pub(crate) async fn import_paper_zip(
    pool: &PgPool,
    file_name: Option<&str>,
    request: &NormalizedCreatePaperRequest,
    zip_bytes: Vec<u8>,
) -> Result<PaperImportResponse> {
    // 验证：文件不能为空
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }
    // 验证：文件大小不超过 20 MiB
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    // 验证 ZIP 格式（只需能打开即可）
    validate_uploaded_zip(&zip_bytes)?;

    // 生成新的试卷 UUID
    let paper_id = Uuid::new_v4().to_string();
    // 规范化文件名
    let normalized_file_name = normalize_upload_file_name(file_name);

    // 开启数据库事务
    let mut tx = pool.begin().await.context("begin paper import tx failed")?;

    // 将 ZIP 文件存储为二进制对象
    let append_object_id = insert_object_tx(
        &mut tx,
        &normalized_file_name,
        &zip_bytes,
        Some("application/zip"),  // MIME 类型
    )
    .await?;

    // 插入试卷元数据
    query(
        r#"
        INSERT INTO papers (
            paper_id, description, title, subtitle, authors, reviewers,
            append_object_id, created_at, updated_at
        )
        VALUES ($1::uuid, $2, $3, $4, $5, $6, $7::uuid, NOW(), NOW())
        "#,
    )
    .bind(&paper_id)
    .bind(&request.description)
    .bind(&request.title)
    .bind(&request.subtitle)
    .bind(&request.authors)
    .bind(&request.reviewers)
    .bind(&append_object_id)
    .execute(&mut *tx)
    .await
    .context("insert paper failed")?;

    // 插入题目关联（设置排序顺序）
    for (idx, question_id) in request.question_ids.iter().enumerate() {
        query(
            r#"
            INSERT INTO paper_questions (paper_id, question_id, sort_order, created_at)
            VALUES ($1::uuid, $2::uuid, $3, NOW())
            "#,
        )
        .bind(&paper_id)
        .bind(question_id)
        .bind(i32::try_from(idx + 1).unwrap_or(i32::MAX))  // 排序从 1 开始
        .execute(&mut *tx)
        .await
        .with_context(|| format!("insert paper question ref failed: {question_id}"))?;
    }

    // 提交事务
    tx.commit().await.context("commit paper import failed")?;

    // 返回导入响应
    Ok(PaperImportResponse {
        paper_id,
        file_name: normalized_file_name,
        question_count: request.question_ids.len(),
        status: "imported",
    })
}

// ============================================================
// replace_paper_zip 函数
// ============================================================
/// 替换试卷 ZIP 文件
///
/// # 参数
/// - pool: 数据库连接池
/// - paper_id: 试卷 UUID
/// - file_name: 上传的文件名
/// - zip_bytes: ZIP 文件字节
///
/// # 处理流程
/// 1. 验证 ZIP 格式
/// 2. 获取旧的 ZIP 对象 ID
/// 3. 存储新的 ZIP 对象
/// 4. 更新 papers 表的引用
/// 5. 删除旧的对象
///
/// # 返回
/// 替换响应（试卷 ID、文件名、状态）
pub(crate) async fn replace_paper_zip(
    pool: &PgPool,
    paper_id: &str,
    file_name: Option<&str>,
    zip_bytes: Vec<u8>,
) -> Result<PaperFileReplaceResponse> {
    // 验证：文件不能为空
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }
    // 验证：文件大小不超过 20 MiB
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    // 验证 ZIP 格式
    validate_uploaded_zip(&zip_bytes)?;

    // 规范化文件名
    let normalized_file_name = normalize_upload_file_name(file_name);

    // 开启数据库事务
    let mut tx = pool
        .begin()
        .await
        .context("begin paper file replace tx failed")?;

    // 查询当前的附录对象 ID
    let previous_object_id = query(
        "SELECT append_object_id::text AS append_object_id FROM papers WHERE paper_id = $1::uuid",
    )
    .bind(paper_id)
    .fetch_optional(&mut *tx)
    .await
    .context("load paper appendix reference failed")?
    .map(|row| row.get::<String, _>("append_object_id"))
    .ok_or_else(|| anyhow::anyhow!("paper not found: {paper_id}"))?;

    // 插入新的 ZIP 对象
    let append_object_id = insert_object_tx(
        &mut tx,
        &normalized_file_name,
        &zip_bytes,
        Some("application/zip"),
    )
    .await?;

    // 更新试卷的附录对象引用
    query("UPDATE papers SET append_object_id = $2::uuid, updated_at = NOW() WHERE paper_id = $1::uuid")
        .bind(paper_id)
        .bind(&append_object_id)
        .execute(&mut *tx)
        .await
        .context("update paper appendix object failed")?;

    // 删除旧的对象（释放存储空间）
    query("DELETE FROM objects WHERE object_id = $1::uuid")
        .bind(&previous_object_id)
        .execute(&mut *tx)
        .await
        .context("delete previous paper appendix object failed")?;

    // 提交事务
    tx.commit()
        .await
        .context("commit paper file replace failed")?;

    // 返回替换响应
    Ok(PaperFileReplaceResponse {
        paper_id: paper_id.to_string(),
        file_name: normalized_file_name,
        status: "replaced",
    })
}

// ============================================================
// validate_uploaded_zip 函数
// ============================================================
/// 验证 ZIP 文件是否可读
///
/// # 参数
/// - zip_bytes: ZIP 文件字节
///
/// # 返回
/// - Ok: ZIP 格式有效
/// - Err: 无法打开 ZIP
fn validate_uploaded_zip(zip_bytes: &[u8]) -> Result<()> {
    let cursor = Cursor::new(zip_bytes);
    ZipArchive::new(cursor).context("open zip archive failed")?;
    Ok(())
}

// ============================================================
// normalize_upload_file_name 函数
// ============================================================
/// 规范化上传文件名
///
/// # 处理逻辑
/// 1. 提取文件名的文件名部分（去除路径）
/// 2. 检查是否为空
/// 3. 提供默认值 "paper.zip"
///
/// # 参数
/// - file_name: 原始文件名
///
/// # 返回
/// 规范化后的文件名
fn normalize_upload_file_name(file_name: Option<&str>) -> String {
    let candidate = file_name
        .and_then(|value| Path::new(value).file_name())  // 提取文件名
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty());

    candidate.unwrap_or_else(|| "paper.zip".to_string())
}

// ============================================================
// insert_object_tx 函数
// ============================================================
/// 在事务中插入二进制对象
///
/// # 参数
/// - tx: 数据库事务
/// - file_name: 文件名
/// - bytes: 文件字节
/// - mime_type: MIME 类型
///
/// # 返回
/// 新对象的 UUID
async fn insert_object_tx(
    tx: &mut Transaction<'_, Postgres>,
    file_name: &str,
    bytes: &[u8],
    mime_type: Option<&str>,
) -> Result<String> {
    // 生成新的对象 UUID
    let object_id = Uuid::new_v4().to_string();

    // 从文件路径提取文件名
    let source_path = PathBuf::from(file_name);
    let normalized_file_name = source_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "blob.bin".to_string());

    // 插入对象到 objects 表
    query(
        r#"
        INSERT INTO objects (object_id, file_name, mime_type, size_bytes, content, created_at)
        VALUES ($1::uuid, $2, $3, $4, $5, NOW())
        "#,
    )
    .bind(&object_id)
    .bind(&normalized_file_name)
    .bind(mime_type)
    .bind(i64::try_from(bytes.len()).context("object bytes exceed i64 range")?)
    .bind(bytes)
    .execute(&mut **tx)
    .await
    .context("insert object failed")?;

    Ok(object_id)
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. ZipArchive::new()
 *    - 从 Cursor 中读取 ZIP 数据
 *    - 只验证格式，不解析内容
 *    - 失败时返回错误
 *
 * 2. Path::file_name()
 *    - 从路径中提取文件名
 *    - 例如："/a/b/c.zip" → Some("c.zip")
 *    - Windows/Linux 路径分隔符都支持
 *
 * 3. to_string_lossy()
 *    - 将 OsStr 转换为 String
 *    - 处理非 UTF-8 字符（替换为 ）
 *    - 避免转换失败
 *
 * 4. Transaction 事务处理
 *    - &mut Transaction<'_, Postgres>
 *    - 保证多个操作的原子性
 *    - 失败时自动回滚
 *
 * 5. &mut **tx 用法
 *    - tx: &mut Transaction
 *    - *tx: Transaction
 *    - **tx: 内部连接
 *    - 用于获取可变引用
 *
 * ============================================================
 * 试卷导入 vs 题目导入
 * ============================================================
 *
 * 相同点:
 * - 都使用 ZIP 上传
 * - 都限制 20 MiB
 * - 都存储到 objects 表
 * - 都使用事务保证一致性
 *
 * 不同点:
 * - 题目：解析 ZIP 内容（tex + assets）
 * - 试卷：只验证 ZIP 格式，整体存储
 * - 题目：插入 question_files
 * - 试卷：只存储 append_object_id
 *
 * ============================================================
 * 数据表关系
 * ============================================================
 *
 * papers 表
 * ├─ paper_id (UUID, PK)
 * ├─ description, title, subtitle
 * ├─ authors[], reviewers[]
 * └─ append_object_id → objects.object_id
 *
 * paper_questions 表
 * ├─ paper_id → papers.paper_id
 * ├─ question_id → questions.question_id
 * └─ sort_order (排序)
 *
 * objects 表
 * ├─ object_id (UUID, PK)
 * ├─ file_name, mime_type, size_bytes
 * └─ content (BYTEA)
 *
 */
