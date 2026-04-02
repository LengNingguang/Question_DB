// ============================================================
// 文件：src/api/ops/quality.rs
// 说明：数据质量检查功能
// ============================================================

//! 题库数据质量审计
//!
//! 检查缺失的对象、空内容、空试卷等数据完整性问题

// 导入 anyhow 错误处理
use anyhow::{Context, Result};

// 导入 Serde 序列化
use serde::Serialize;

// 导入 Serde JSON
use serde_json::{json, Value};

// 导入 SQLx 数据库操作
use sqlx::{query, PgPool, Row};

// ============================================================
// QualityReport 结构体
// ============================================================
/// 质量检查报告
#[derive(Debug, Serialize)]
pub(crate) struct QualityReport {
    /// 缺少 TeX 对象的题目 ID 列表
    pub(crate) missing_tex_object: Vec<String>,
    /// TeX 源内容为空的题目 ID 列表
    pub(crate) missing_tex_source: Vec<String>,
    /// 缺失的资源文件详情列表
    pub(crate) missing_asset_objects: Vec<Value>,
    /// 不包含任何题目的试卷 ID 列表
    pub(crate) empty_papers: Vec<String>,
}

// ============================================================
// object_exists 函数
// ============================================================
/// 检查对象是否存在
///
/// # 参数
/// - pool: 数据库连接池
/// - object_id: 对象 UUID
///
/// # 返回
/// - true: 对象存在
/// - false: 对象不存在
pub(crate) async fn object_exists(pool: &PgPool, object_id: &str) -> Result<bool> {
    // 查询是否存在
    Ok(query("SELECT 1 FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)  // 返回 Option<PgRow>
        .await
        .with_context(|| format!("check object existence failed: {object_id}"))?
        .is_some())  // Some → true, None → false
}

// ============================================================
// object_blob_nonempty 函数
// ============================================================
/// 检查对象内容是否非空
///
/// # 参数
/// - pool: 数据库连接池
/// - object_id: 对象 UUID
///
/// # 返回
/// - true: 内容长度 > 0
/// - false: 内容长度为 0 或对象不存在
pub(crate) async fn object_blob_nonempty(pool: &PgPool, object_id: &str) -> Result<bool> {
    // 查询 content 字节的长度
    let row = query("SELECT octet_length(content) AS size FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("check object blob failed: {object_id}"))?;

    // 获取大小并判断
    Ok(row
        .and_then(|r| r.try_get::<Option<i32>, _>("size").ok().flatten())
        .unwrap_or(0)  // None 或错误 → 0
        > 0)
}

// ============================================================
// build_quality_report 函数
// ============================================================
/// 构建完整的质量检查报告
///
/// # 检查项目
/// 1. missing_tex_object: 没有 TeX 文件记录或对象不存在的题目
/// 2. missing_tex_source: TeX 对象内容为空的题目
/// 3. missing_asset_objects: 资源文件对象不存在
/// 4. empty_papers: 不包含任何题目的试卷
///
/// # 处理流程
/// 1. 遍历所有题目
/// 2. 检查每个题目的 TeX 文件
/// 3. 检查每个题目的资源文件
/// 4. 查询所有空试卷
pub(crate) async fn build_quality_report(pool: &PgPool) -> Result<QualityReport> {
    // 初始化报告
    let mut report = QualityReport {
        missing_tex_object: Vec::new(),
        missing_tex_source: Vec::new(),
        missing_asset_objects: Vec::new(),
        empty_papers: Vec::new(),
    };

    // ========== 检查所有题目 ==========
    // 获取所有题目 ID
    let question_rows = query("SELECT question_id::text AS question_id FROM questions")
        .fetch_all(pool)
        .await
        .context("query questions for quality report failed")?;

    // 遍历每个题目
    for row in question_rows {
        let question_id: String = row.get("question_id");

        // ----- 检查 TeX 文件 -----
        let tex_rows = query(
            "SELECT object_id::text AS object_id, file_path FROM question_files WHERE question_id = $1::uuid AND file_kind = 'tex'",
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query tex files failed: {question_id}"))?;

        // 如果没有 TeX 文件记录
        if tex_rows.is_empty() {
            report.missing_tex_object.push(question_id.clone());
        }

        // 检查每个 TeX 文件
        for tex_row in tex_rows {
            let object_id: String = tex_row.get("object_id");
            // 检查对象是否存在
            if !object_exists(pool, &object_id).await? {
                report.missing_tex_object.push(question_id.clone());
            // 检查内容是否非空
            } else if !object_blob_nonempty(pool, &object_id).await? {
                report.missing_tex_source.push(question_id.clone());
            }
        }

        // ----- 检查资源文件 -----
        let asset_rows = query(
            "SELECT object_id::text AS object_id, file_path FROM question_files WHERE question_id = $1::uuid AND file_kind = 'asset'",
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query asset files failed: {question_id}"))?;

        // 检查每个资源文件
        for asset_row in asset_rows {
            let object_id: String = asset_row.get("object_id");
            // 检查对象是否存在
            if !object_exists(pool, &object_id).await? {
                // 记录详细信息
                report.missing_asset_objects.push(json!({
                    "question_id": question_id,
                    "file_path": asset_row.get::<String, _>("file_path"),
                    "object_id": object_id,
                }));
            }
        }
    }

    // ========== 检查空试卷 ==========
    let paper_rows = query(
        r#"
        SELECT p.paper_id::text AS paper_id
        FROM papers p
        LEFT JOIN paper_questions pq ON pq.paper_id = p.paper_id
        WHERE pq.paper_id IS NULL
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query empty papers for quality report failed")?;

    // 收集空试卷 ID
    report.empty_papers = paper_rows
        .into_iter()
        .map(|row| row.get::<String, _>("paper_id"))
        .collect();

    Ok(report)
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. LEFT JOIN 检测空关系
 *    LEFT JOIN paper_questions ON ...
 *    WHERE paper_questions.paper_id IS NULL
 *    → 查找没有关联题目的试卷
 *
 * 2. octet_length 函数
 *    PostgreSQL 函数，返回 BYTEA 的字节长度
 *    octet_length(content) → i32
 *
 * 3. Option 链式处理
 *    row
 *       .and_then(|r| r.try_get(...).ok().flatten())
 *       .unwrap_or(0)
 *    - and_then: 有值则处理，无值则跳过
 *    - ok(): Result → Option
 *    - flatten(): Option<Option<T>> → Option<T>
 *
 * 4. json! 宏构建动态对象
 *    json!({
 *        "key": value,
 *        "nested": {...}
 *    })
 *    → serde_json::Value
 *
 * 5. 可变累加模式
 *    let mut report = Report { list: vec![] };
 *    for item in items {
 *        if check_failed(item) {
 *            report.list.push(item.id);
 *        }
 *    }
 *
 * ============================================================
 * 质量检查 SQL 详解
 * ============================================================
 *
 * 1. 查询 TeX 文件:
 *    SELECT object_id, file_path
 *    FROM question_files
 *    WHERE question_id = ? AND file_kind = 'tex'
 *
 * 2. 查询资源文件:
 *    SELECT object_id, file_path
 *    FROM question_files
 *    WHERE question_id = ? AND file_kind = 'asset'
 *
 * 3. 查询空试卷:
 *    SELECT p.paper_id
 *    FROM papers p
 *    LEFT JOIN paper_questions pq ON pq.paper_id = p.paper_id
 *    WHERE pq.paper_id IS NULL
 *
 *    LEFT JOIN 返回所有 papers
 *    没有关联题目的试卷：pq.paper_id 为 NULL
 *
 * ============================================================
 * 质量报告示例输出
 * ============================================================
 *
 * {
 *   "missing_tex_object": [
 *     "uuid-1",  // 没有 TeX 文件或对象不存在
 *     "uuid-2"
 *   ],
 *   "missing_tex_source": [
 *     "uuid-3"   // TeX 对象内容为空
 *   ],
 *   "missing_asset_objects": [
 *     {
 *       "question_id": "uuid-1",
 *       "file_path": "assets/fig1.png",
 *       "object_id": "obj-xxx"
 *     }
 *   ],
 *   "empty_papers": [
 *     "uuid-paper-1"  // 不包含任何题目的试卷
 *   ]
 * }
 *
 * ============================================================
 * 检查逻辑流程图
 * ============================================================
 *
 * build_quality_report
 *     │
 *     ├─ 遍历所有题目
 *     │   ├─ 查询 TeX 文件
 *     │   │   ├─ 无记录 → missing_tex_object
 *     │   │   └─ 有记录
 *     │   │       ├─ 对象不存在 → missing_tex_object
 *     │   │       └─ 内容为空 → missing_tex_source
 *     │   │
 *     │   └─ 查询资源文件
 *     │       └─ 对象不存在 → missing_asset_objects
 *     │
 *     └─ 查询空试卷 → empty_papers
 *
 */
