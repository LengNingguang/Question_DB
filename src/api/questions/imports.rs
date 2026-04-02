// ============================================================
// 文件：src/api/questions/imports.rs
// 说明：题目 ZIP 导入和文件替换逻辑
// ============================================================

//! ZIP 格式的题目导入辅助函数
//!
//! 本文件负责：
//! 1. 解析用户上传的 ZIP 文件
//! 2. 验证 ZIP 目录结构是否符合规范
//! 3. 将题目文件和资源存储到数据库
//! 4. 支持完整替换已有题目的文件

// 导入标准库类型
use std::{
    collections::{BTreeMap, BTreeSet},  // 有序集合
    io::{Cursor, Read},                  // IO 操作
    path::{Component, Path},             // 路径处理
};

// 导入 anyhow 错误处理库
use anyhow::{bail, Context, Result};

// 导入 MIME 类型猜测库
use mime_guess::MimeGuess;

// 导入 SQLx 数据库操作类型
use sqlx::{query, PgPool, Postgres, QueryBuilder, Row, Transaction};

// 导入 UUID 库
use uuid::Uuid;

// 导入 ZIP 文件处理库
use zip::ZipArchive;

// 导入当前模块的模型
use super::models::{
    NormalizedQuestionDifficulty, QuestionFileReplaceResponse, QuestionImportResponse,
};

// ============================================================
// 常量定义
// ============================================================
/// 上传文件大小限制：20 MiB
///
/// # 设计说明
/// - 防止用户上传过大的文件导致内存耗尽
/// - 20 MiB 对于题目 ZIP（TeX + 图片）来说足够大
/// - 可根据实际需求调整
pub(crate) const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

/// ZIP 解压后总大小限制：64 MiB
///
/// # 设计说明
/// - 防止 ZIP 炸弹攻击（小 ZIP 解压后巨大）
/// - 解压过程中累计检查
const MAX_TOTAL_UNCOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

// ============================================================
// ArchiveFile 结构体
// ============================================================
/// ZIP 中的单个文件
///
/// # 字段说明
/// - `path`: 文件在 ZIP 中的路径（规范化后）
/// - `bytes`: 文件内容（已解压）
#[derive(Debug, Clone)]
struct ArchiveFile {
    /// 文件路径
    path: String,
    /// 文件内容
    bytes: Vec<u8>,
}

// ============================================================
// LoadedQuestionZip 结构体
// ============================================================
/// 已加载并验证的题目 ZIP
///
/// # 字段说明
/// - `tex_file`: TeX 源文件（必须有且仅有 1 个）
/// - `asset_files`: 资源文件列表（图片、数据文件等）
#[derive(Debug)]
struct LoadedQuestionZip {
    /// TeX 源文件
    tex_file: ArchiveFile,
    /// 资源文件列表
    asset_files: Vec<ArchiveFile>,
}

// ============================================================
// import_question_zip 函数
// ============================================================
/// 导入新的题目 ZIP
///
/// # 参数
/// - `pool`: 数据库连接池
/// - `file_name`: 原始上传文件名（用于响应）
/// - `description`: 题目描述
/// - `difficulty`: 难度评估（已规范化）
/// - `zip_bytes`: ZIP 文件的二进制内容
///
/// # 返回值
/// - Ok: 导入成功，返回题目 ID 和导入信息
/// - Err: 验证失败或数据库错误
///
/// # 处理流程
/// 1. 验证 ZIP 大小
/// 2. 解析并验证 ZIP 结构
/// 3. 生成新的 UUID 作为题目 ID
/// 4. 开启数据库事务
/// 5. 插入题目记录
/// 6. 存储文件到 objects 表
/// 7. 插入 question_files 关联记录
/// 8. 插入难度评估
/// 9. 提交事务
///
/// # 事务安全
/// 所有数据库操作都在事务中执行
/// 任何步骤失败都会自动回滚，保证数据一致性
pub(crate) async fn import_question_zip(
    pool: &PgPool,
    file_name: Option<&str>,
    description: &str,
    difficulty: &NormalizedQuestionDifficulty,
    zip_bytes: Vec<u8>,
) -> Result<QuestionImportResponse> {
    // 验证：ZIP 不能为空
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }

    // 验证：ZIP 不能超过大小限制
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    // 解析并验证 ZIP 文件结构
    let loaded = load_question_zip(&zip_bytes)?;

    // 生成新的题目 UUID
    let question_id = Uuid::new_v4().to_string();

    // 开启数据库事务
    let mut tx = pool
        .begin()
        .await
        .context("begin question import tx failed")?;

    // 插入题目记录到 questions 表
    // 初始分类和状态都设为 'none'，等待后续元数据更新
    query(
        r#"
        INSERT INTO questions (
            question_id, source_tex_path, category, status, description, created_at, updated_at
        )
        VALUES (
            $1::uuid, $2, 'none', 'none', $3, NOW(), NOW()
        )
        "#,
    )
    .bind(&question_id)
    .bind(&loaded.tex_file.path)
    .bind(description)
    .execute(&mut *tx)
    .await
    .context("insert uploaded question failed")?;

    // 存储文件并建立关联
    insert_loaded_question_files_tx(&mut tx, &question_id, &loaded).await?;

    // 插入难度评估记录
    // 遍历已规范化的难度条目
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
        .with_context(|| format!("insert question difficulty failed: {algorithm_tag}"))?;
    }

    // 提交事务
    tx.commit().await.context("commit question import failed")?;

    // 返回导入响应
    Ok(QuestionImportResponse {
        question_id,
        file_name: normalize_upload_file_name(file_name),
        imported_assets: loaded.asset_files.len(),
        status: "imported",
    })
}

// ============================================================
// replace_question_zip 函数
// ============================================================
/// 替换已有题目的 ZIP 文件
///
/// # 参数
/// - `pool`: 数据库连接池
/// - `question_id`: 要替换的题目 ID
/// - `file_name`: 原始上传文件名（用于响应）
/// - `zip_bytes`: ZIP 文件的二进制内容
///
/// # 返回值
/// - Ok: 替换成功，返回题目 ID 和替换信息
/// - Err: 验证失败、题目不存在或数据库错误
///
/// # 处理流程
/// 1. 验证 ZIP 大小和结构
/// 2. 开启数据库事务
/// 3. 检查题目是否存在
/// 4. 删除旧的文件记录
/// 5. 删除旧的对象记录（级联清理）
/// 6. 插入新文件
/// 7. 更新 source_tex_path
/// 8. 提交事务
///
/// # 与 import 的区别
/// - 不生成新 UUID，使用传入的 question_id
/// - 不修改难度评估（保留原有）
/// - 会删除旧文件并清理对象存储
pub(crate) async fn replace_question_zip(
    pool: &PgPool,
    question_id: &str,
    file_name: Option<&str>,
    zip_bytes: Vec<u8>,
) -> Result<QuestionFileReplaceResponse> {
    // 验证：ZIP 不能为空
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }

    // 验证：ZIP 不能超过大小限制
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    // 解析并验证 ZIP 文件结构
    let loaded = load_question_zip(&zip_bytes)?;

    // 规范化文件名
    let normalized_file_name = normalize_upload_file_name(file_name);

    // 开启数据库事务
    let mut tx = pool
        .begin()
        .await
        .context("begin question file replace tx failed")?;

    // 检查题目是否存在
    let exists = query("SELECT 1 FROM questions WHERE question_id = $1::uuid")
        .bind(question_id)
        .fetch_optional(&mut *tx)
        .await
        .context("check question existence failed")?
        .is_some();
    if !exists {
        bail!("question not found: {question_id}");
    }

    // 替换文件（删除旧的，插入新的）
    replace_question_files_tx(&mut tx, question_id, &loaded).await?;

    // 更新题目的 source_tex_path
    query(
        "UPDATE questions SET source_tex_path = $2, updated_at = NOW() WHERE question_id = $1::uuid",
    )
    .bind(question_id)
    .bind(&loaded.tex_file.path)
    .execute(&mut *tx)
    .await
    .context("update question source_tex_path failed")?;

    // 提交事务
    tx.commit()
        .await
        .context("commit question file replace failed")?;

    // 返回替换响应
    Ok(QuestionFileReplaceResponse {
        question_id: question_id.to_string(),
        file_name: normalized_file_name,
        source_tex_path: loaded.tex_file.path,
        imported_assets: loaded.asset_files.len(),
        status: "replaced",
    })
}

// ============================================================
// load_question_zip 函数
// ============================================================
/// 加载并验证 ZIP 文件
///
/// # 参数
/// - `zip_bytes`: ZIP 文件的二进制内容
///
/// # 返回值
/// - Ok: 解析成功，返回 LoadedQuestionZip
/// - Err: ZIP 格式错误或结构不符合规范
///
/// # 验证步骤
/// 1. 打开 ZIP 归档
/// 2. 遍历每个条目
/// 3. 规范化并安全检查路径（防止路径遍历攻击）
/// 4. 累计解压后大小（防止 ZIP 炸弹）
/// 5. 读取文件内容到内存
/// 6. 记录目录结构
/// 7. 验证标准布局（1 个 TeX + assets/ 目录）
pub(crate) fn load_question_zip(zip_bytes: &[u8]) -> Result<LoadedQuestionZip> {
    // 创建内存游标
    let cursor = Cursor::new(zip_bytes);

    // 打开 ZIP 归档
    let mut archive = ZipArchive::new(cursor).context("open zip archive failed")?;

    // 存储文件：path -> ArchiveFile
    let mut files = BTreeMap::new();
    // 存储目录路径
    let mut directories = BTreeSet::new();
    // 累计解压后总大小
    let mut total_uncompressed = 0usize;

    // 遍历 ZIP 中的每个条目
    for idx in 0..archive.len() {
        // 读取条目
        let mut entry = archive
            .by_index(idx)
            .with_context(|| format!("read zip entry #{idx} failed"))?;

        // 获取原始路径名
        let raw_name = entry.name().to_string();

        // 安全检查并规范化路径
        let path = sanitize_archive_path(&raw_name)?;

        // 处理目录条目（以 / 结尾）
        if raw_name.ends_with('/') {
            directories.insert(path);
            continue;
        }

        // 计算解压后大小
        let size_hint = usize::try_from(entry.size()).unwrap_or(usize::MAX);
        total_uncompressed = total_uncompressed.saturating_add(size_hint);

        // 检查是否超过解压大小限制
        if total_uncompressed > MAX_TOTAL_UNCOMPRESSED_BYTES {
            bail!("zip expands beyond the allowed uncompressed size");
        }

        // 读取文件内容
        let mut bytes = Vec::with_capacity(size_hint.min(1024 * 1024));
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("read zip entry bytes failed: {path}"))?;

        // 注册父目录
        register_parent_directories(&mut directories, &path);

        // 存储文件
        files.insert(path.clone(), ArchiveFile { path, bytes });
    }

    // 验证标准目录布局
    let (tex_file, asset_files) = validate_standard_layout(&files, &directories)?;

    Ok(LoadedQuestionZip {
        tex_file,
        asset_files,
    })
}

// ============================================================
// sanitize_archive_path 函数
// ============================================================
/// 规范化并检查 ZIP 条目路径的安全性
///
/// # 参数
/// - `path`: 原始路径字符串
///
/// # 返回值
/// - Ok(String): 规范化后的路径
/// - Err: 路径不安全
///
/// # 安全检查
/// 1. 将反斜杠替换为正斜杠（Windows 兼容）
/// 2. 不能是绝对路径
/// 3. 不能包含 `..`（父目录引用）
/// 4. 不能是空路径
/// 5. 只能包含 Normal 和 CurDir 组件
///
/// # 设计说明
/// 防止 ZIP 路径遍历攻击（如 `../../../etc/passwd`）
/// 确保所有文件都在归档内部
pub(crate) fn sanitize_archive_path(path: &str) -> Result<String> {
    // 将 Windows 风格路径转换为 Unix 风格
    let normalized = path.replace('\\', "/");
    let candidate = Path::new(&normalized);

    // 不能是绝对路径
    if candidate.is_absolute() {
        bail!("zip entry must be relative: {path}");
    }

    // 遍历路径组件进行检查
    let mut cleaned = Vec::new();
    for component in candidate.components() {
        match component {
            // 正常组件：保留
            Component::Normal(part) => cleaned.push(part.to_string_lossy().to_string()),
            // 当前目录（.）：忽略
            Component::CurDir => {}
            // 危险组件：拒绝
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("zip entry has unsafe path: {path}");
            }
        }
    }

    // 拼接规范化后的路径
    let joined = cleaned.join("/");

    // 不能是空路径
    if joined.is_empty() {
        bail!("zip entry has empty path");
    }

    Ok(joined)
}

// ============================================================
// validate_standard_layout 函数
// ============================================================
/// 验证 ZIP 目录结构是否符合标准布局
///
/// # 标准布局规则
/// ```
/// archive.zip
/// ├── problem.tex          # 恰好 1 个 .tex 文件（根目录）
/// └── assets/              # 恰好 1 个 assets 目录
///     ├── fig1.png
///     ├── fig2.svg
///     └── data.csv
/// ```
///
/// # 参数
/// - `files`: 文件映射
/// - `directories`: 目录集合
///
/// # 返回值
/// - Ok: (tex_file, asset_files)
/// - Err: 结构不符合规范
///
/// # 验证规则
/// 1. 根目录只能包含 1 个 .tex 文件
/// 2. 根目录只能包含 1 个名为 `assets/` 的目录
/// 3. 非根文件必须在 `assets/` 目录下
pub(crate) fn validate_standard_layout(
    files: &BTreeMap<String, ArchiveFile>,
    directories: &BTreeSet<String>,
) -> Result<(ArchiveFile, Vec<ArchiveFile>)> {
    // 收集根目录下的 TeX 文件和资源文件
    let mut root_tex_files = Vec::new();
    let mut asset_files = Vec::new();

    // 收集根目录下的所有一级目录
    let root_directories = directories
        .iter()
        .filter(|dir| !dir.contains('/'))
        .cloned()
        .collect::<BTreeSet<_>>();

    // 遍历所有文件进行验证
    for file in files.values() {
        // 按 / 分割路径组件
        let components = file.path.split('/').collect::<Vec<_>>();

        match components.as_slice() {
            // 情况 1: 根目录文件（只有文件名，没有目录）
            [file_name] => {
                if is_tex_file(file_name) {
                    // TeX 文件：收集起来
                    root_tex_files.push(file.clone());
                } else {
                    // 其他文件：不允许
                    bail!(
                        "zip root may only contain one .tex file and one assets/ directory, found unexpected file: {}",
                        file.path
                    );
                }
            }
            // 情况 2: 子目录文件
            [root_dir, ..] => {
                // 必须在 assets/ 目录下
                if *root_dir != "assets" {
                    bail!(
                        "all non-root files must be inside the root assets/ directory, found: {}",
                        file.path
                    );
                }
                asset_files.push(file.clone());
            }
            // 情况 3: 空路径（理论上不应该发生）
            [] => bail!("zip entry has empty path"),
        }
    }

    // 验证：恰好 1 个 TeX 文件
    if root_tex_files.len() != 1 {
        bail!(
            "zip root must contain exactly one .tex file, found {}",
            root_tex_files.len()
        );
    }

    // 验证：必须有 assets/ 目录
    if !root_directories.iter().any(|dir| dir == "assets") {
        bail!("zip root must contain exactly one assets/ directory");
    }

    // 验证：恰好 1 个根目录（即 assets/）
    if root_directories.len() != 1 {
        bail!("zip root must contain exactly one directory named assets/");
    }

    // 返回 TeX 文件和资源文件列表
    Ok((root_tex_files.remove(0), asset_files))
}

// ============================================================
// is_tex_file 函数
// ============================================================
/// 判断文件是否为 TeX 文件
///
/// # 判断规则
/// - 扩展名为 `.tex`（不区分大小写）
fn is_tex_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("tex"))
        .unwrap_or(false)
}

// ============================================================
// register_parent_directories 函数
// ============================================================
/// 注册文件的所有父目录
///
/// # 用途
/// 确保目录集合包含所有层级的父目录
/// 便于后续验证标准布局时正确识别根目录
///
/// # 示例
/// 对于路径 `assets/figures/fig1.png`
/// 会注册 `assets` 和 `assets/figures`
fn register_parent_directories(directories: &mut BTreeSet<String>, path: &str) {
    let components = path.split('/').collect::<Vec<_>>();
    // 如果只有文件名（没有父目录），直接返回
    if components.len() <= 1 {
        return;
    }

    // 注册所有父目录
    for idx in 1..components.len() {
        directories.insert(components[..idx].join("/"));
    }
}

// ============================================================
// normalize_upload_file_name 函数
// ============================================================
/// 规范化上传文件名
///
/// # 处理逻辑
/// 1. 提取基本文件名（去除路径）
/// 2. 转换为字符串
/// 3. 过滤空值
/// 4. 如果没有有效文件名，使用默认值 `question.zip`
///
/// # 安全考虑
/// - 只保留文件名，防止路径泄露
/// - 处理空值情况
fn normalize_upload_file_name(file_name: Option<&str>) -> String {
    file_name
        .and_then(|value| Path::new(value).file_name())
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "question.zip".to_string())
}

// ============================================================
// insert_loaded_question_files_tx 函数
// ============================================================
/// 在事务中插入已加载的文件
///
/// # 参数
/// - `tx`: 数据库事务
/// - `question_id`: 题目 ID
/// - `loaded`: 已加载的 ZIP 内容
///
/// # 处理流程
/// 1. 插入 TeX 文件到 objects 表
/// 2. 插入 question_files 关联记录（kind="tex"）
/// 3. 对每个资源文件：
///    - 猜测 MIME 类型
///    - 插入到 objects 表
///    - 插入 question_files 关联记录（kind="asset"）
///
/// # MIME 类型
/// - TeX 文件：固定为 `text/x-tex`
/// - 资源文件：根据扩展名自动猜测
async fn insert_loaded_question_files_tx(
    tx: &mut Transaction<'_, Postgres>,
    question_id: &str,
    loaded: &LoadedQuestionZip,
) -> Result<()> {
    // 插入 TeX 文件
    let tex_object_id = insert_object_tx(
        tx,
        Path::new(&loaded.tex_file.path),
        &loaded.tex_file.bytes,
        Some("text/x-tex"),
    )
    .await?;

    // 建立 TeX 文件关联
    insert_question_file_tx(
        tx,
        question_id,
        &tex_object_id,
        "tex",
        &loaded.tex_file.path,
        Some("text/x-tex"),
    )
    .await?;

    // 遍历资源文件
    for asset in &loaded.asset_files {
        // 根据文件扩展名猜测 MIME 类型
        let mime = MimeGuess::from_path(&asset.path)
            .first_raw()
            .map(str::to_string);

        // 插入资源文件到 objects 表
        let object_id =
            insert_object_tx(tx, Path::new(&asset.path), &asset.bytes, mime.as_deref()).await?;

        // 建立资源文件关联
        insert_question_file_tx(
            tx,
            question_id,
            &object_id,
            "asset",
            &asset.path,
            mime.as_deref(),
        )
        .await?;
    }

    Ok(())
}

// ============================================================
// replace_question_files_tx 函数
// ============================================================
/// 在事务中替换题目文件
///
/// # 参数
/// - `tx`: 数据库事务
/// - `question_id`: 题目 ID
/// - `loaded`: 已加载的 ZIP 内容
///
/// # 处理流程
/// 1. 查询所有旧文件的 object_id
/// 2. 删除 question_files 记录
/// 3. 删除旧的 objects 记录（级联清理）
/// 4. 插入新文件
///
/// # 为什么需要删除旧 objects
/// - 新文件的 object_id 是重新生成的
/// - 旧的 object_id 不再有引用
/// - 及时清理避免存储空间泄漏
async fn replace_question_files_tx(
    tx: &mut Transaction<'_, Postgres>,
    question_id: &str,
    loaded: &LoadedQuestionZip,
) -> Result<()> {
    // 查询所有旧文件的 object_id
    let old_object_ids = query(
        "SELECT object_id::text AS object_id FROM question_files WHERE question_id = $1::uuid",
    )
    .bind(question_id)
    .fetch_all(&mut **tx)
    .await
    .context("load existing question file objects failed")?
    .into_iter()
    .map(|row| row.get::<String, _>("object_id"))
    .collect::<Vec<_>>();

    // 删除 question_files 关联记录
    query("DELETE FROM question_files WHERE question_id = $1::uuid")
        .bind(question_id)
        .execute(&mut **tx)
        .await
        .context("delete existing question files failed")?;

    // 如果有旧对象，批量删除
    if !old_object_ids.is_empty() {
        // 使用 QueryBuilder 动态构建 IN 子句
        let mut builder = QueryBuilder::<Postgres>::new("DELETE FROM objects WHERE object_id IN (");
        for (idx, object_id) in old_object_ids.iter().enumerate() {
            if idx > 0 {
                builder.push(", ");
            }
            builder.push_bind(object_id).push("::uuid");
        }
        builder.push(')');

        builder
            .build()
            .execute(&mut **tx)
            .await
            .context("delete previous question file objects failed")?;
    }

    // 插入新文件
    insert_loaded_question_files_tx(tx, question_id, loaded).await
}

// ============================================================
// insert_object_tx 函数
// ============================================================
/// 在事务中插入对象到 objects 表
///
/// # 参数
/// - `tx`: 数据库事务
/// - `source_path`: 源文件路径（用于提取文件名）
/// - `bytes`: 文件二进制内容
/// - `mime_type`: MIME 类型（可选）
///
/// # 返回值
/// - Ok(String): 生成的 object_id
/// - Err: 数据库错误
///
/// # objects 表结构
/// | 字段 | 类型 | 说明 |
/// |------|------|------|
/// | object_id | UUID | 主键 |
/// | file_name | TEXT | 文件名 |
/// | mime_type | TEXT | MIME 类型 |
/// | size_bytes | BIGINT | 大小（字节） |
/// | content | BYTEA | 二进制内容 |
/// | created_at | TIMESTAMPTZ | 创建时间 |
async fn insert_object_tx(
    tx: &mut Transaction<'_, Postgres>,
    source_path: &Path,
    bytes: &[u8],
    mime_type: Option<&str>,
) -> Result<String> {
    // 生成新的对象 UUID
    let object_id = Uuid::new_v4().to_string();

    // 从路径提取文件名
    let file_name = source_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "blob.bin".to_string());

    // 插入对象记录
    query(
        r#"
        INSERT INTO objects (object_id, file_name, mime_type, size_bytes, content, created_at)
        VALUES ($1::uuid, $2, $3, $4, $5, NOW())
        "#,
    )
    .bind(&object_id)
    .bind(&file_name)
    .bind(mime_type)
    .bind(i64::try_from(bytes.len()).context("object bytes exceed i64 range")?)
    .bind(bytes)
    .execute(&mut **tx)
    .await
    .context("insert object failed")?;

    Ok(object_id)
}

// ============================================================
// insert_question_file_tx 函数
// ============================================================
/// 在事务中插入题目文件关联记录
///
/// # 参数
/// - `tx`: 数据库事务
/// - `question_id`: 题目 ID
/// - `object_id`: 对象存储 ID
/// - `file_kind`: 文件类型（"tex" 或 "asset"）
/// - `file_path`: 文件在 ZIP 中的路径
/// - `mime_type`: MIME 类型（可选）
///
/// # question_files 表结构
/// | 字段 | 类型 | 说明 |
/// |------|------|------|
/// | question_file_id | UUID | 主键 |
/// | question_id | UUID | 外键（引用 questions） |
/// | object_id | UUID | 外键（引用 objects） |
/// | file_kind | TEXT | 文件类型 |
/// | file_path | TEXT | 文件路径 |
/// | mime_type | TEXT | MIME 类型 |
/// | created_at | TIMESTAMPTZ | 创建时间 |
async fn insert_question_file_tx(
    tx: &mut Transaction<'_, Postgres>,
    question_id: &str,
    object_id: &str,
    file_kind: &str,
    file_path: &str,
    mime_type: Option<&str>,
) -> Result<()> {
    // 插入关联记录
    query(
        r#"
        INSERT INTO question_files (
            question_file_id, question_id, object_id, file_kind, file_path, mime_type, created_at
        )
        VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5, $6, NOW())
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(question_id)
    .bind(object_id)
    .bind(file_kind)
    .bind(file_path)
    .bind(mime_type)
    .execute(&mut **tx)
    .await
    .with_context(|| format!("insert question file failed: {file_path}"))?;

    Ok(())
}

// ============================================================
// 测试模块
// ============================================================
#[cfg(test)]
mod tests {
    use super::{load_question_zip, MAX_UPLOAD_BYTES};
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    // --------------------------------------------------------
    // 辅助函数：构建标准布局的 ZIP
    // --------------------------------------------------------
    /// 创建一个符合标准布局的测试 ZIP
    ///
    /// 结构:
    /// - problem.tex
    /// - assets/fig1.png
    fn build_zip() -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        // 添加 TeX 文件
        writer.start_file("problem.tex", options).unwrap();
        writer.write_all(br"\section{Demo}").unwrap();

        // 添加资源文件
        writer.start_file("assets/fig1.png", options).unwrap();
        writer.write_all(b"png").unwrap();

        writer.finish().unwrap().into_inner()
    }

    // --------------------------------------------------------
    // 测试：标准布局解析
    // --------------------------------------------------------
    #[test]
    fn load_question_zip_reads_standard_layout() {
        // 解析标准 ZIP
        let loaded = load_question_zip(&build_zip()).expect("zip should parse");

        // 验证 TeX 文件路径
        assert_eq!(loaded.tex_file.path, "problem.tex");

        // 验证资源文件数量
        assert_eq!(loaded.asset_files.len(), 1);
    }

    // --------------------------------------------------------
    // 测试：拒绝额外的根目录文件
    // --------------------------------------------------------
    #[test]
    fn load_question_zip_rejects_extra_root_file() {
        // 创建一个包含额外文件的 ZIP
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        writer.start_file("problem.tex", options).unwrap();
        writer.write_all(br"\section{Demo}").unwrap();

        // 额外文件：readme.txt（不允许）
        writer.start_file("readme.txt", options).unwrap();
        writer.write_all(b"nope").unwrap();

        writer.start_file("assets/fig1.png", options).unwrap();
        writer.write_all(b"png").unwrap();

        let zip = writer.finish().unwrap().into_inner();

        // 应该被拒绝
        let err = load_question_zip(&zip).expect_err("zip should be rejected");

        // 验证错误信息包含 "unexpected file"
        assert!(err.to_string().contains("unexpected file"));
    }

    // --------------------------------------------------------
    // 测试：上传限制常量
    // --------------------------------------------------------
    #[test]
    fn upload_limit_constant_matches_requirement() {
        // 验证 MAX_UPLOAD_BYTES 确实是 20 MiB
        assert_eq!(MAX_UPLOAD_BYTES, 20 * 1024 * 1024);
    }
}

// ============================================================
// 知识点讲解 (ZIP 导入和文件处理)
// ============================================================
//
// 1. ZIP 文件格式
//    - 一种常见的压缩归档格式
//    - 支持多个文件和目录
//    - Rust 使用 zip crate 进行读写
//
// 2. 路径安全检查 (sanitize_archive_path)
//    - 防止 ZIP 路径遍历攻击
//    - 攻击示例：../../etc/passwd
//    - 防御：检查路径组件，拒绝 ParentDir 和绝对路径
//
// 3. ZIP 炸弹防护
//    - 小 ZIP 解压后可能非常大
//    - 攻击：耗尽内存或磁盘空间
//    - 防护：累计检查解压后大小
//
// 4. 标准布局验证
//    - 规定 ZIP 必须包含 1 个 TeX + 1 个 assets/
//    - 便于后续处理和渲染
//    - 使用模式匹配验证结构
//
// 5. 事务处理
//    - 所有数据库操作都在事务中执行
//    - 任何步骤失败都会自动回滚
//    - 保证数据一致性
//
// 6. BTreeMap vs BTreeSet
//    - BTreeMap: 有序键值对
//    - BTreeSet: 有序集合
//    - 有序性便于调试和测试
//
// 7. Path 组件分析
//    - Path::components() 返回路径的各个组件
//    - Component::Normal: 正常目录/文件名
//    - Component::CurDir: 当前目录（.）
//    - Component::ParentDir: 父目录（..）
//    - Component::RootDir: 根目录（/）
//    - Component::Prefix: Windows 盘符（C:）
//
// 8. MIME 类型猜测
//    - mime_guess crate 根据扩展名猜测 MIME
//    - 示例：.png -> image/png
//    - 返回 Option，因为可能猜不出
//
// 9. QueryBuilder 动态 SQL
//    - 用于构建 IN 子句（可变长度）
//    - 避免手动拼接 SQL
//    - 支持参数绑定
//
// ============================================================
// 目录布局示例
// ============================================================
//
// 有效的 ZIP 结构：
//
//   archive.zip
//   ├── problem.tex          # 恰好 1 个 .tex 文件（根目录）
//   └── assets/              # 恰好 1 个 assets 目录
//       ├── figures/
//       │   ├── fig1.png
//       │   └── fig2.svg
//       ├── data/
//       │   └── dataset.csv
//       └── solutions/
//           └── answer.tex
//
// 无效的结构示例：
//
// ❌ 两个 TeX 文件:
//   ├── problem1.tex
//   ├── problem2.tex
//   └── assets/
//
// ❌ 根目录有额外文件:
//   ├── problem.tex
//   ├── readme.txt          # 不允许
//   └── assets/
//
// ❌ assets 目录命名错误:
//   ├── problem.tex
//   └── figures/            # 必须是 assets/
//
// ❌ 路径遍历攻击:
//   ├── problem.tex
//   └── ../../etc/passwd    # 被 sanitize_archive_path 拒绝
//
// ============================================================
// 数据库表关系
// ============================================================
//
// questions
// ├── question_id (PK) ──────────────┐
// └── source_tex_path                │
//                                    │
// question_files                     │
// ├── question_file_id (PK)          │
// ├── question_id (FK) ──────────────┘
// ├── object_id (FK) ────────┐
// ├── file_kind              │
// ├── file_path              │
// └── mime_type              │
//                            │
// objects                    │
// ├── object_id (PK) ────────┘
// ├── file_name
// ├── mime_type
// ├── size_bytes
// └── content (BYTEA)
//
// question_difficulties
// ├── question_id (FK) ──────┐
// ├── algorithm_tag          │
// ├── score                  │
// └── notes                  │
//                            │
// (来自 params) ─────────────┘
//
// ============================================================
