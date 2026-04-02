// ============================================================
// 文件：src/api/ops/exports.rs
// 说明：数据导出功能（JSONL/CSV）
// ============================================================

//! 题目数据的导出管道
//!
//! 支持将题库数据导出为 JSONL 或 CSV 格式

// 导入标准库类型
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

// 导入 anyhow 错误处理
use anyhow::{Context, Result};

// 导入 CSV 处理库
use csv::WriterBuilder;

// 导入 Serde JSON
use serde_json::json;

// 导入 SQLx 数据库操作
use sqlx::{query, PgPool, Row};

// 导入当前模块的类型
use super::models::ExportFormat;

// 导入题目模块的类型和查询
use crate::api::{
    questions::{
        models::QuestionSourceRef,
        queries::{load_question_difficulties, load_question_files, load_question_tags},
    },
    shared::utils::canonical_or_original,
};

// ============================================================
// default_export_path 函数
// ============================================================
/// 生成默认的导出文件路径
///
/// # 参数
/// - format: 导出格式（Jsonl/Csv）
/// - is_public: 是否公开版本（不包含 TeX 源码）
///
/// # 返回
/// 默认路径：exports/question_bank_{public|internal}.{ext}
pub(crate) fn default_export_path(format: ExportFormat, is_public: bool) -> PathBuf {
    // 根据 public 参数确定后缀
    let suffix = if is_public { "public" } else { "internal" };
    // 确定扩展名
    let ext = match format {
        ExportFormat::Jsonl => "jsonl",
        ExportFormat::Csv => "csv",
    };
    // 返回完整路径
    PathBuf::from("exports").join(format!("question_bank_{suffix}.{ext}"))
}

// ============================================================
// ensure_parent_dir 函数
// ============================================================
/// 确保父目录存在
///
/// # 参数
/// - output_path: 输出文件路径
/// - label: 错误消息标签（用于区分导出/报告）
pub(crate) fn ensure_parent_dir(output_path: &Path, label: &str) -> Result<()> {
    // 如果路径有父目录
    if let Some(parent) = output_path.parent() {
        // 创建所有父目录（不存在则创建）
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create {label} parent directory failed: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    Ok(())
}

// ============================================================
// fetch_text_object 函数
// ============================================================
/// 获取文本对象的内容
///
/// # 参数
/// - pool: 数据库连接池
/// - object_id: 对象 UUID
///
/// # 返回
/// UTF-8 解码后的文本
pub(crate) async fn fetch_text_object(pool: &PgPool, object_id: &str) -> Result<String> {
    // 查询对象内容
    let row = query("SELECT content FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_one(pool)
        .await
        .with_context(|| format!("query object failed: {object_id}"))?;

    // 获取二进制内容
    let content: Vec<u8> = row.get("content");
    // 转换为 UTF-8 字符串（无效字符替换为 ）
    Ok(String::from_utf8_lossy(&content).to_string())
}

// ============================================================
// export_jsonl 函数
// ============================================================
/// 导出为 JSONL 格式
///
/// # 参数
/// - pool: 数据库连接池
/// - output_path: 输出文件路径
/// - include_tex_source: 是否包含 TeX 源码
///
/// # JSONL 格式
/// 每行一个 JSON 对象：
/// {"question_id":"...", "category":"T", "tex_source":"..."}
/// {"question_id":"...", "category":"E", ...}
///
/// # 返回
/// 导出的题目数量
pub(crate) async fn export_jsonl(
    pool: &PgPool,
    output_path: &Path,
    include_tex_source: bool,
) -> Result<usize> {
    // 查询所有题目
    let rows = query(
        r#"
        SELECT question_id::text AS question_id, source_tex_path, category, status,
               COALESCE(description, '') AS description,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        ORDER BY created_at DESC, question_id
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query questions for jsonl export failed")?;

    // 创建输出文件
    let file = fs::File::create(output_path).with_context(|| {
        format!(
            "create export file failed: {}",
            output_path.to_string_lossy()
        )
    })?;
    // 包装为 BufWriter 提高性能
    let mut writer = BufWriter::new(file);

    // 遍历每个题目
    for row in &rows {
        // 获取题目 ID
        let question_id: String = row.get("question_id");

        // 加载关联数据
        let tex_files = load_question_files(pool, &question_id, "tex").await?;
        let assets = load_question_files(pool, &question_id, "asset").await?;
        let tags = load_question_tags(pool, &question_id).await?;
        let difficulty = load_question_difficulties(pool, &question_id).await?;

        // 获取 TeX 对象 ID
        let tex_object_id = tex_files
            .first()
            .map(|file| file.object_id.clone())
            .unwrap_or_default();

        // 构建 JSON 对象
        let mut payload = json!({
            "question_id": question_id,
            "tex_object_id": tex_object_id,
            "source": QuestionSourceRef {
                tex: row.get("source_tex_path"),
            },
            "category": row.get::<String, _>("category"),
            "status": row.get::<String, _>("status"),
            "description": row.get::<String, _>("description"),
            "difficulty": difficulty,
            "tags": tags,
            "assets": assets,
            "created_at": row.get::<String, _>("created_at"),
            "updated_at": row.get::<String, _>("updated_at"),
        });

        // 如果是内部版本且 TeX 存在，添加源码
        if include_tex_source && !tex_object_id.is_empty() {
            payload["tex_source"] =
                serde_json::Value::String(fetch_text_object(pool, &tex_object_id).await?);
        }

        // 写入一行
        writer
            .write_all(serde_json::to_string(&payload)?.as_bytes())
            .context("write jsonl line failed")?;
        // 添加换行
        writer.write_all(b"\n").context("write newline failed")?;
    }

    // 刷新缓冲区
    writer.flush().context("flush jsonl writer failed")?;
    // 返回题目数量
    Ok(rows.len())
}

// ============================================================
// export_csv 函数
// ============================================================
/// 导出为 CSV 格式
///
/// # 参数
/// - pool: 数据库连接池
/// - output_path: 输出文件路径
/// - include_tex_source: 是否包含 TeX 源码
///
/// # CSV 列
/// question_id, tex_object_id, source_tex_path, category, status,
/// description, difficulty, tags, created_at, updated_at, tex_source
///
/// # 返回
/// 导出的题目数量
pub(crate) async fn export_csv(
    pool: &PgPool,
    output_path: &Path,
    include_tex_source: bool,
) -> Result<usize> {
    // 查询所有题目
    let rows = query(
        r#"
        SELECT question_id::text AS question_id, source_tex_path, category, status,
               COALESCE(description, '') AS description,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        ORDER BY created_at DESC, question_id
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query questions for csv export failed")?;

    // 创建输出文件
    let file = fs::File::create(output_path).with_context(|| {
        format!(
            "create export csv failed: {}",
            output_path.to_string_lossy()
        )
    })?;
    // 创建 CSV Writer
    let mut writer = WriterBuilder::new().from_writer(file);

    // 写入表头
    writer.write_record([
        "question_id",
        "tex_object_id",
        "source_tex_path",
        "category",
        "status",
        "description",
        "difficulty",
        "tags",
        "created_at",
        "updated_at",
        "tex_source",
    ])?;

    // 遍历每个题目
    for row in &rows {
        let question_id: String = row.get("question_id");
        let tex_files = load_question_files(pool, &question_id, "tex").await?;
        let tags = load_question_tags(pool, &question_id).await?;
        let difficulty = load_question_difficulties(pool, &question_id).await?;
        let tex_object_id = tex_files
            .first()
            .map(|file| file.object_id.clone())
            .unwrap_or_default();

        // 构建记录行
        writer.write_record([
            question_id,
            tex_object_id.clone(),
            row.get::<String, _>("source_tex_path"),
            row.get::<String, _>("category"),
            row.get::<String, _>("status"),
            row.get::<String, _>("description"),
            serde_json::to_string(&difficulty)?,  // JSON 字符串
            serde_json::to_string(&tags)?,         // JSON 字符串
            row.get::<String, _>("created_at"),
            row.get::<String, _>("updated_at"),
            if include_tex_source && !tex_object_id.is_empty() {
                fetch_text_object(pool, &tex_object_id).await?
            } else {
                String::new()  // 公开版本留空
            },
        ])?;
    }

    // 刷新输出
    writer.flush().context("flush csv writer failed")?;
    Ok(rows.len())
}

// ============================================================
// exported_path 函数
// ============================================================
/// 获取规范化的导出路径
pub(crate) fn exported_path(path: &Path) -> String {
    canonical_or_original(path)
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. BufWriter 缓冲写入
 *    - BufWriter::new(file) 包装文件
 *    - 减少系统调用次数
 *    - 提高写入性能
 *    - 用完需要.flush()
 *
 * 2. CSV WriterBuilder
 *    - WriterBuilder::new() 创建构建器
 *    - .from_writer(file) 包装写入器
 *    - .write_record([fields]) 写入一行
 *    - 自动处理转义和引号
 *
 * 3. json! 宏
 *    - 快速构建 JSON 对象
 *    - 支持 Rust 类型自动转换
 *    - payload["key"] = value 添加字段
 *
 * 4. from_utf8_lossy
 *    - 处理可能无效的 UTF-8
 *    - 无效字节替换为
 *    - 返回 Cow<str> 类型
 *
 * 5. 条件序列化
 *    - if include_tex_source { ... }
 *    - 公开版本不包含源码
 *    - 内部版本包含完整数据
 *
 * ============================================================
 * JSONL vs CSV 格式对比
 * ============================================================
 *
 * JSONL (推荐):
 * - 每行一个完整的 JSON 对象
 * - 支持嵌套结构（difficulty、tags 数组）
 * - 适合程序处理
 * - 示例:
 *   {"question_id":"uuid","category":"T","difficulty":{"human":{"score":5}}}
 *
 * CSV (适合 Excel):
 * - 逗号分隔的扁平结构
 * - 复杂数据存为 JSON 字符串
 * - 适合人工查看
 * - 示例:
 *   uuid,tex_id,path,T,reviewed,描述，{...},["tag1"],2024-01-01,...,
 *
 * ============================================================
 * 导出字段说明
 * ============================================================
 *
 * | 字段名          | 说明                     |
 * |----------------|--------------------------|
 * | question_id    | 题目 UUID                |
 * | tex_object_id  | TeX 源文件的对象 ID      |
 * | source_tex_path| TeX 源文件路径           |
 * | category       | 分类（none/T/E）         |
 * | status         | 状态（none/reviewed/used）|
 * | description    | 题目描述                 |
 * | difficulty     | 难度定义（JSON）         |
 * | tags           | 标签列表（JSON 数组）    |
 * | assets         | 资源文件列表（JSON 数组） |
 * | created_at     | 创建时间（ISO 8601）     |
 * | updated_at     | 更新时间（ISO 8601）     |
 * | tex_source     | TeX 源码（仅内部版本）   |
 *
 */
