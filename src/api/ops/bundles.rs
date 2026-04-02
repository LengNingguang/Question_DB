// ============================================================
// 文件：src/api/ops/bundles.rs
// 说明：题目和试卷打包逻辑
// ============================================================

//! 题目和试卷的 ZIP 打包功能
//!
//! 生成包含元数据、资源文件、渲染后 LaTeX 的 ZIP 包

// 导入标准库类型
use std::{
    fs::File,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

// 导入 anyhow 错误处理
use anyhow::{anyhow, Context, Result};

// 导入 Axum 响应类型
use axum::{
    body::Body,
    http::{header, HeaderValue, Response, StatusCode},
};

// 导入 Serde 序列化
use serde::Serialize;

// 导入 SQLx 数据库操作
use sqlx::{query, PgPool, Row};

// 导入 Tokio 异步文件系统
use tokio::fs;

// 导入 Tokio 工具
use tokio_util::io::ReaderStream;

// 导入 UUID 库
use uuid::Uuid;

// 导入 ZIP 处理库
use zip::{write::SimpleFileOptions, ZipWriter};

// 导入当前模块的类型
use crate::api::{
    // LaTeX 渲染引擎
    ops::paper_render::{
        render_paper_bundle, PaperTemplateKind, RenderPaperInput, RenderQuestionAssetInput,
        RenderQuestionInput,
    },
    // 试卷模型
    papers::models::PaperDetail,
    // 题目模型和查询
    questions::{
        models::{QuestionAssetRef, QuestionDetail, QuestionPaperRef},
        queries::{
            load_question_difficulties, load_question_files, load_question_tags, map_paper_detail,
            map_paper_question_summary, map_question_detail,
        },
    },
    // 共享工具
    shared::utils::bundle_directory_name,
};

// ============================================================
// QuestionBundleManifest 结构体
// ============================================================
/// 题目打包的 manifest.json 结构
#[derive(Debug, Serialize)]
struct QuestionBundleManifest {
    kind: &'static str,               // "question_bundle"
    generated_at_unix: u64,           // 生成时间戳（Unix 秒）
    question_count: usize,            // 题目数量
    questions: Vec<QuestionBundleManifestItem>,
}

// ============================================================
// QuestionBundleManifestItem 结构体
// ============================================================
/// 题目打包中单个题目的元数据
#[derive(Debug, Serialize)]
struct QuestionBundleManifestItem {
    question_id: String,              // 题目 UUID
    directory: String,                // ZIP 中的目录名
    metadata: QuestionDetail,         // 题目完整元数据
    files: Vec<BundleFileEntry>,      // 文件列表
}

// ============================================================
// PaperBundleManifest 结构体
// ============================================================
/// 试卷打包的 manifest.json 结构
#[derive(Debug, Serialize)]
struct PaperBundleManifest {
    kind: &'static str,               // "paper_bundle"
    generated_at_unix: u64,           // 生成时间戳
    paper_count: usize,               // 试卷数量
    papers: Vec<PaperBundleManifestItem>,
}

// ============================================================
// PaperBundleManifestItem 结构体
// ============================================================
/// 试卷打包中单个试卷的元数据
#[derive(Debug, Serialize)]
struct PaperBundleManifestItem {
    paper_id: String,                 // 试卷 UUID
    directory: String,                // ZIP 中的目录名
    metadata: PaperDetail,            // 试卷完整元数据
    template_source: String,          // 使用的 LaTeX 模板路径
    append_file: BundleFileEntry,     // 附录文件
    main_tex_file: BundleFileEntry,   // 渲染后的主 TeX 文件
    assets: Vec<BundleFileEntry>,     // 资源文件列表
    questions: Vec<PaperBundleQuestionManifestItem>,  // 题目列表
}

// ============================================================
// PaperBundleQuestionManifestItem 结构体
// ============================================================
/// 试卷打包中单个题目的元数据
#[derive(Debug, Serialize)]
struct PaperBundleQuestionManifestItem {
    question_id: String,              // 题目 UUID
    sequence: usize,                  // 题目序号
    source_tex_path: String,          // TeX 源路径
    asset_prefix: String,             // 资源文件前缀
    metadata: QuestionDetail,         // 题目元数据
}

// ============================================================
// BundleFileEntry 结构体
// ============================================================
/// ZIP 包中文件的元数据
#[derive(Debug, Clone, Serialize)]
struct BundleFileEntry {
    zip_path: String,                 // ZIP 中的路径
    original_path: String,            // 原始路径
    file_kind: String,                // 文件类型（tex/asset 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    source_question_id: Option<String>,  // 来源题目 ID（如有）
    #[serde(skip_serializing_if = "Option::is_none")]
    object_id: Option<String>,        // 对象 ID
    mime_type: Option<String>,        // MIME 类型
}

// ============================================================
// QuestionBundleData 结构体
// ============================================================
/// 题目打包数据（内部使用）
#[derive(Debug)]
struct QuestionBundleData {
    metadata: QuestionDetail,         // 题目元数据
    files: Vec<QuestionAssetRef>,     // 文件列表
}

// ============================================================
// PaperBundleData 结构体
// ============================================================
/// 试卷打包数据（内部使用）
#[derive(Debug)]
struct PaperBundleData {
    metadata: PaperDetail,            // 试卷元数据
    appendix: PaperAppendixData,      // 附录数据
    questions: Vec<QuestionBundleData>,  // 题目数据列表
}

// ============================================================
// PaperAppendixData 结构体
// ============================================================
/// 试卷附录数据
#[derive(Debug)]
struct PaperAppendixData {
    object_id: String,                // 对象 UUID
    original_file_name: String,       // 原始文件名
    mime_type: Option<String>,        // MIME 类型
}

// ============================================================
// build_question_bundle_response 函数
// ============================================================
/// 构建题目打包响应
///
/// # 参数
/// - pool: 数据库连接池
/// - question_ids: 题目 ID 列表
///
/// # 处理流程
/// 1. 创建临时 ZIP 文件
/// 2. 为每个题目创建目录并写入文件
/// 3. 生成 manifest.json
/// 4. 完成 ZIP 并返回 Response
///
/// # ZIP 结构
/// ```
/// questions_bundle_*.zip
/// ├── manifest.json
/// ├── {desc}_{id}/
/// │   ├── problem.tex
/// │   └── assets/
/// ```
pub(crate) async fn build_question_bundle_response(
    pool: &PgPool,
    question_ids: &[String],
) -> Result<Response<Body>> {
    // 生成 ZIP 文件名和临时路径
    let bundle_name = format!("questions_bundle_{}.zip", timestamp_unix());
    let zip_path = temp_zip_path("questions");

    // 创建 ZIP 文件
    let file = File::create(&zip_path).with_context(|| {
        format!(
            "create question bundle zip failed: {}",
            zip_path.to_string_lossy()
        )
    })?;

    // 创建 ZIP 写入器
    let mut writer = ZipWriter::new(file);
    // 预分配 manifest 条目容量
    let mut manifest_items = Vec::with_capacity(question_ids.len());

    // 遍历每个题目 ID
    for question_id in question_ids {
        // 加载题目数据
        let bundle = load_question_bundle_data(pool, question_id).await?;
        // 生成目录名（描述 + ID）
        let directory = bundle_directory_name(&bundle.metadata.description, question_id);
        // 写入题目文件
        let manifest_files =
            write_question_bundle_files(pool, &mut writer, &bundle.files, &directory).await?;
        // 添加到 manifest
        manifest_items.push(QuestionBundleManifestItem {
            question_id: question_id.clone(),
            directory,
            metadata: bundle.metadata,
            files: manifest_files,
        });
    }

    // 生成 manifest.json
    let manifest = QuestionBundleManifest {
        kind: "question_bundle",
        generated_at_unix: timestamp_unix(),
        question_count: manifest_items.len(),
        questions: manifest_items,
    };
    write_manifest(&mut writer, &manifest)?;

    // 完成 ZIP 并返回响应
    finish_zip_response(writer, zip_path, &bundle_name).await
}

// ============================================================
// build_paper_bundle_response 函数
// ============================================================
/// 构建试卷打包响应
///
/// # 参数
/// - pool: 数据库连接池
/// - paper_ids: 试卷 ID 列表
///
/// # 处理流程
/// 1. 创建临时 ZIP 文件
/// 2. 为每个试卷：
///    - 写入附录 ZIP
///    - 渲染 LaTeX 模板生成 main.tex
///    - 写入资源文件
/// 3. 生成 manifest.json
/// 4. 完成 ZIP 并返回 Response
///
/// # ZIP 结构
/// ```
/// papers_bundle_*.zip
/// ├── manifest.json
/// ├── {desc}_{id}/
/// │   ├── main.tex (渲染后的试卷)
/// │   ├── append.zip (原始附录)
/// │   └── assets/
/// ```
pub(crate) async fn build_paper_bundle_response(
    pool: &PgPool,
    paper_ids: &[String],
) -> Result<Response<Body>> {
    // 生成 ZIP 文件名和临时路径
    let bundle_name = format!("papers_bundle_{}.zip", timestamp_unix());
    let zip_path = temp_zip_path("papers");

    // 创建 ZIP 文件
    let file = File::create(&zip_path).with_context(|| {
        format!(
            "create paper bundle zip failed: {}",
            zip_path.to_string_lossy()
        )
    })?;

    // 创建 ZIP 写入器
    let mut writer = ZipWriter::new(file);
    let mut manifest_items = Vec::with_capacity(paper_ids.len());

    // 遍历每个试卷 ID
    for paper_id in paper_ids {
        // 加载试卷数据
        let bundle = load_paper_bundle_data(pool, paper_id).await?;
        // 生成目录名
        let directory = bundle_directory_name(&bundle.metadata.description, paper_id);

        // 写入附录文件
        let append_file =
            write_paper_appendix_file(pool, &mut writer, &bundle.appendix, &directory).await?;

        // 渲染 LaTeX 模板
        let rendered = render_paper_bundle(build_render_paper_input(pool, &bundle).await?)?;

        // 写入 main.tex
        let main_tex_zip_path = format!("{directory}/main.tex");
        write_bundle_file(
            &mut writer,
            &main_tex_zip_path,
            rendered.main_tex.as_bytes(),
        )?;
        let main_tex_file = BundleFileEntry {
            zip_path: main_tex_zip_path,
            original_path: rendered.template_source_path.to_string(),
            file_kind: "rendered_tex".to_string(),
            source_question_id: None,
            object_id: None,
            mime_type: Some("text/x-tex".to_string()),
        };

        // 写入渲染后的资源文件
        let mut rendered_asset_entries = Vec::with_capacity(rendered.assets.len());
        for asset in &rendered.assets {
            let zip_path = format!("{directory}/{}", asset.output_path);
            write_bundle_file(&mut writer, &zip_path, &asset.bytes)?;
            rendered_asset_entries.push(BundleFileEntry {
                zip_path,
                original_path: asset.original_path.clone(),
                file_kind: "asset".to_string(),
                source_question_id: Some(asset.question_id.clone()),
                object_id: Some(asset.object_id.clone()),
                mime_type: asset.mime_type.clone(),
            });
        }

        // 生成题目 manifest 条目
        let question_entries = bundle
            .questions
            .into_iter()
            .zip(rendered.questions.into_iter())
            .map(
                |(question, rendered_question)| PaperBundleQuestionManifestItem {
                    question_id: rendered_question.question_id,
                    sequence: rendered_question.sequence,
                    source_tex_path: rendered_question.source_tex_path,
                    asset_prefix: rendered_question.asset_prefix,
                    metadata: question.metadata,
                },
            )
            .collect::<Vec<_>>();

        // 添加到 manifest
        manifest_items.push(PaperBundleManifestItem {
            paper_id: paper_id.clone(),
            directory,
            metadata: bundle.metadata,
            template_source: rendered.template_source_path.to_string(),
            append_file,
            main_tex_file,
            assets: rendered_asset_entries,
            questions: question_entries,
        });
    }

    // 生成 manifest.json
    let manifest = PaperBundleManifest {
        kind: "paper_bundle",
        generated_at_unix: timestamp_unix(),
        paper_count: manifest_items.len(),
        papers: manifest_items,
    };
    write_manifest(&mut writer, &manifest)?;

    // 完成 ZIP 并返回响应
    finish_zip_response(writer, zip_path, &bundle_name).await
}

// ============================================================
// write_question_bundle_files 函数
// ============================================================
/// 写入题目文件到 ZIP
///
/// # 参数
/// - pool: 数据库连接池
/// - writer: ZIP 写入器
/// - files: 文件列表
/// - directory: ZIP 中的目录名
async fn write_question_bundle_files(
    pool: &PgPool,
    writer: &mut ZipWriter<File>,
    files: &[QuestionAssetRef],
    directory: &str,
) -> Result<Vec<BundleFileEntry>> {
    let mut manifest_entries = Vec::with_capacity(files.len());

    // 遍历每个文件
    for file in files {
        // 计算 ZIP 路径
        let zip_path = format!("{directory}/{}", file.path);
        // 获取文件内容
        let bytes = fetch_object_bytes(pool, &file.object_id).await?;
        // 写入 ZIP
        write_bundle_file(writer, &zip_path, &bytes)?;

        // 添加到 manifest
        manifest_entries.push(BundleFileEntry {
            zip_path,
            original_path: file.path.clone(),
            file_kind: file.file_kind.clone(),
            source_question_id: None,
            object_id: Some(file.object_id.clone()),
            mime_type: file.mime_type.clone(),
        });
    }

    Ok(manifest_entries)
}

// ============================================================
// write_paper_appendix_file 函数
// ============================================================
/// 写入试卷附录文件到 ZIP
async fn write_paper_appendix_file(
    pool: &PgPool,
    writer: &mut ZipWriter<File>,
    appendix: &PaperAppendixData,
    directory: &str,
) -> Result<BundleFileEntry> {
    // 附录固定命名为 append.zip
    let zip_path = format!("{directory}/append.zip");
    let bytes = fetch_object_bytes(pool, &appendix.object_id).await?;
    write_bundle_file(writer, &zip_path, &bytes)?;

    Ok(BundleFileEntry {
        zip_path,
        original_path: appendix.original_file_name.clone(),
        file_kind: "appendix".to_string(),
        source_question_id: None,
        object_id: Some(appendix.object_id.clone()),
        mime_type: appendix.mime_type.clone(),
    })
}

// ============================================================
// write_bundle_file 函数
// ============================================================
/// 写入单个文件到 ZIP
///
/// # 参数
/// - writer: ZIP 写入器
/// - zip_path: ZIP 中的路径
/// - bytes: 文件字节
fn write_bundle_file(writer: &mut ZipWriter<File>, zip_path: &str, bytes: &[u8]) -> Result<()> {
    // 开始新文件
    writer
        .start_file(zip_path, SimpleFileOptions::default())
        .context("start bundle file entry failed")?;
    // 写入内容
    writer
        .write_all(bytes)
        .with_context(|| format!("write bundle file failed: {zip_path}"))?;
    Ok(())
}

// ============================================================
// write_manifest 函数
// ============================================================
/// 写入 manifest.json 到 ZIP
fn write_manifest<T: Serialize>(writer: &mut ZipWriter<File>, manifest: &T) -> Result<()> {
    writer
        .start_file("manifest.json", SimpleFileOptions::default())
        .context("start manifest.json failed")?;
    // 序列化为美化的 JSON
    let bytes = serde_json::to_vec_pretty(manifest).context("serialize manifest.json failed")?;
    writer
        .write_all(&bytes)
        .context("write manifest.json failed")?;
    Ok(())
}

// ============================================================
// finish_zip_response 函数
// ============================================================
/// 完成 ZIP 并构建 HTTP 响应
///
/// # 参数
/// - writer: ZIP 写入器
/// - zip_path: 临时文件路径
/// - bundle_name: 响应文件名
///
/// # 返回
/// - Response<Body>: HTTP 响应
async fn finish_zip_response(
    writer: ZipWriter<File>,
    zip_path: PathBuf,
    bundle_name: &str,
) -> Result<Response<Body>> {
    // 完成 ZIP 写入
    let file = writer.finish().context("finish zip archive failed")?;
    // 获取文件大小
    let size = file
        .metadata()
        .context("read zip metadata failed")?
        .len()
        .to_string();
    // 删除文件句柄
    drop(file);

    // 打开文件准备流式传输
    let std_file = File::open(&zip_path)
        .with_context(|| format!("open finished zip failed: {}", zip_path.to_string_lossy()))?;
    // 删除临时文件
    std::fs::remove_file(&zip_path).ok();

    // 创建 ReaderStream 实现流式响应
    let stream = ReaderStream::new(fs::File::from_std(std_file));
    let body = Body::from_stream(stream);

    // 设置响应头
    let content_type = HeaderValue::from_static("application/zip");
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{bundle_name}\""))
        .context("build content-disposition header failed")?;
    let content_length =
        HeaderValue::from_str(&size).context("build content-length header failed")?;

    // 构建响应
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, content_length)
        .body(body)
        .context("build zip response failed")
}

// ============================================================
// load_question_bundle_data 函数
// ============================================================
/// 加载题目打包数据
///
/// # 处理流程
/// 1. 查询题目基本信息
/// 2. 加载 TeX 文件
/// 3. 加载资源文件
/// 4. 加载标签和难度
/// 5. 加载关联试卷
async fn load_question_bundle_data(pool: &PgPool, question_id: &str) -> Result<QuestionBundleData> {
    // 查询基本信息
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
    .fetch_optional(pool)
    .await
    .with_context(|| format!("load question detail failed: {question_id}"))?
    .ok_or_else(|| anyhow!("question not found: {question_id}"))?;

    // 加载 TeX 文件
    let tex_files = load_question_files(pool, question_id, "tex")
        .await
        .with_context(|| format!("load question tex files failed: {question_id}"))?;
    let tex_object_id = tex_files
        .first()
        .map(|file| file.object_id.clone())
        .ok_or_else(|| anyhow!("question is missing a tex object: {question_id}"))?;

    // 加载资源文件
    let assets = load_question_files(pool, question_id, "asset")
        .await
        .with_context(|| format!("load question assets failed: {question_id}"))?;

    // 加载标签
    let tags = load_question_tags(pool, question_id)
        .await
        .with_context(|| format!("load question tags failed: {question_id}"))?;

    // 加载难度
    let difficulty = load_question_difficulties(pool, question_id)
        .await
        .with_context(|| format!("load question difficulties failed: {question_id}"))?;

    // 加载关联试卷
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
    .fetch_all(pool)
    .await
    .with_context(|| format!("load question papers failed: {question_id}"))?
    .into_iter()
    .map(|row| QuestionPaperRef {
        paper_id: row.get("paper_id"),
        description: row.get("description"),
        title: row.get("title"),
        subtitle: row.get("subtitle"),
        sort_order: row.get("sort_order"),
    })
    .collect::<Vec<_>>();

    // 合并文件列表
    let mut files = tex_files.clone();
    files.extend(assets.clone());

    Ok(QuestionBundleData {
        metadata: map_question_detail(row, tex_object_id, tags, difficulty, assets, papers),
        files,
    })
}

// ============================================================
// load_paper_bundle_data 函数
// ============================================================
/// 加载试卷打包数据
async fn load_paper_bundle_data(pool: &PgPool, paper_id: &str) -> Result<PaperBundleData> {
    // 查询试卷基本信息（含附录对象）
    let paper_row = query(
        r#"
        SELECT p.paper_id::text AS paper_id, p.description, p.title, p.subtitle,
               p.authors, p.reviewers, p.append_object_id::text AS append_object_id,
               o.file_name AS append_file_name, o.mime_type AS append_mime_type,
               to_char(p.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM papers p
        JOIN objects o ON o.object_id = p.append_object_id
        WHERE p.paper_id = $1::uuid
        "#,
    )
    .bind(paper_id)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("load paper detail failed: {paper_id}"))?
    .ok_or_else(|| anyhow!("paper not found: {paper_id}"))?;

    // 查询题目列表
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
    .fetch_all(pool)
    .await
    .with_context(|| format!("load paper questions failed: {paper_id}"))?;

    // 为每个题目加载数据
    let mut question_summaries = Vec::with_capacity(question_rows.len());
    let mut questions = Vec::with_capacity(question_rows.len());
    for row in question_rows {
        let question_id: String = row.get("question_id");
        let tags = load_question_tags(pool, &question_id)
            .await
            .with_context(|| format!("load paper question tags failed: {question_id}"))?;
        question_summaries.push(map_paper_question_summary(row, tags));
        // 递归加载题目数据
        questions.push(load_question_bundle_data(pool, &question_id).await?);
    }

    // 构建附录数据
    let appendix = PaperAppendixData {
        object_id: paper_row.get("append_object_id"),
        original_file_name: paper_row.get("append_file_name"),
        mime_type: paper_row.get("append_mime_type"),
    };

    Ok(PaperBundleData {
        metadata: map_paper_detail(paper_row, question_summaries),
        appendix,
        questions,
    })
}

// ============================================================
// build_render_paper_input 函数
// ============================================================
/// 构建 LaTeX 渲染输入
///
/// # 处理流程
/// 1. 确定模板类型（T/E）
/// 2. 为每个题目加载 TeX 源码和资源
async fn build_render_paper_input(
    pool: &PgPool,
    bundle: &PaperBundleData,
) -> Result<RenderPaperInput> {
    // 确定模板类型
    let template_kind = determine_paper_template_kind(&bundle.questions)?;
    let mut questions = Vec::with_capacity(bundle.questions.len());

    // 为每个题目准备渲染数据
    for (index, question) in bundle.questions.iter().enumerate() {
        // 获取 TeX 内容
        let tex_bytes = fetch_object_bytes(pool, &question.metadata.tex_object_id).await?;
        let source_tex = String::from_utf8(tex_bytes).with_context(|| {
            format!(
                "question tex object is not valid UTF-8: {}",
                question.metadata.tex_object_id
            )
        })?;

        // 准备资源文件
        let mut assets = Vec::with_capacity(question.metadata.assets.len());
        for asset in &question.metadata.assets {
            assets.push(RenderQuestionAssetInput {
                original_path: asset.path.clone(),
                object_id: asset.object_id.clone(),
                mime_type: asset.mime_type.clone(),
                bytes: fetch_object_bytes(pool, &asset.object_id).await?,
            });
        }

        questions.push(RenderQuestionInput {
            question_id: question.metadata.question_id.clone(),
            sequence: index + 1,
            source_tex_path: question.metadata.source.tex.clone(),
            source_tex,
            assets,
        });
    }

    Ok(RenderPaperInput {
        title: bundle.metadata.title.clone(),
        subtitle: bundle.metadata.subtitle.clone(),
        authors: bundle.metadata.authors.clone(),
        reviewers: bundle.metadata.reviewers.clone(),
        template_kind,
        questions,
    })
}

// ============================================================
// determine_paper_template_kind 函数
// ============================================================
/// 确定试卷模板类型
///
/// # 规则
/// - 所有题目分类必须是 T 或 E
/// - 所有题目分类必须一致
/// - T → Theory 模板，E → Experiment 模板
fn determine_paper_template_kind(questions: &[QuestionBundleData]) -> Result<PaperTemplateKind> {
    // 获取第一题的分类
    let first_question = questions
        .first()
        .ok_or_else(|| anyhow!("paper does not contain any questions"))?;
    let expected_category = first_question.metadata.category.as_str();

    // 确定模板类型
    let template_kind = match expected_category {
        "T" => PaperTemplateKind::Theory,
        "E" => PaperTemplateKind::Experiment,
        other => {
            return Err(anyhow!(
                "paper questions must all be category T or E before rendering, found {other}"
            ));
        }
    };

    // 验证所有题目分类一致
    for question in questions.iter().skip(1) {
        if question.metadata.category != expected_category {
            return Err(anyhow!(
                "paper questions must share one category before rendering, found {} and {}",
                expected_category,
                question.metadata.category
            ));
        }
    }

    Ok(template_kind)
}

// ============================================================
// fetch_object_bytes 函数
// ============================================================
/// 获取对象内容字节
async fn fetch_object_bytes(pool: &PgPool, object_id: &str) -> Result<Vec<u8>> {
    let row = query("SELECT content FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_one(pool)
        .await
        .with_context(|| format!("load object content failed: {object_id}"))?;
    Ok(row.get("content"))
}

// ============================================================
// 辅助函数
// ============================================================

/// 生成临时 ZIP 路径
fn temp_zip_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "qb_{prefix}_bundle_{}_{}.zip",
        timestamp_unix(),
        Uuid::new_v4()
    ))
}

/// 获取 Unix 时间戳（秒）
fn timestamp_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. ZipWriter 用法
 *    - ZipWriter::new(file) 创建写入器
 *    - .start_file(name, options) 开始新文件
 *    - .write_all(bytes) 写入内容
 *    - .finish() 完成 ZIP
 *
 * 2. ReaderStream 流式响应
 *    - 将文件读取转为异步流
 *    - 避免大文件占用内存
 *    - Body::from_stream(stream) 构建响应体
 *
 * 3. Serialize derive
 *    - #[derive(Serialize)] 自动生成序列化
 *    - skip_serializing_if 条件跳过序列化
 *    - serde_json::to_vec_pretty 美化输出
 *
 * 4. 嵌套数据结构
 *    - PaperBundleData 包含 Vec<QuestionBundleData>
 *    - 递归加载数据
 *    - 注意错误上下文传递
 *
 * 5. 临时文件处理
 *    - 使用 temp_dir() 创建临时文件
 *    - 完成后立即删除
 *    - UUID 防止命名冲突
 *
 * ============================================================
 * ZIP 包结构对比
 * ============================================================
 *
 * 题目包 (questions_bundle_*.zip):
 * ├── manifest.json
 * └── {desc}_{id}/
 *     ├── problem.tex
 *     └── assets/
 *         ├── image1.png
 *         └── image2.png
 *
 * 试卷包 (papers_bundle_*.zip):
 * ├── manifest.json
 * └── {desc}_{id}/
 *     ├── main.tex (渲染后)
 *     ├── append.zip
 *     └── assets/
 *         ├── p1-figure1.png (题目 1 的资源)
 *         └── p2-diagram.jpg (题目 2 的资源)
 *
 * ============================================================
 * manifest.json 字段说明
 * ============================================================
 *
 * 题目包 manifest:
 * {
 *   "kind": "question_bundle",
 *   "generated_at_unix": 1234567890,
 *   "question_count": 5,
 *   "questions": [
 *     {
 *       "question_id": "uuid",
 *       "directory": "Desc_id",
 *       "metadata": {...},  // 完整题目元数据
 *       "files": [...]       // 文件列表
 *     }
 *   ]
 * }
 *
 * 试卷包 manifest:
 * {
 *   "kind": "paper_bundle",
 *   "paper_count": 3,
 *   "papers": [
 *     {
 *       "paper_id": "uuid",
 *       "directory": "Desc_id",
 *       "metadata": {...},      // 完整试卷元数据
 *       "template_source": "...", // LaTeX 模板路径
 *       "main_tex_file": {...},   // 渲染后的 main.tex
 *       "questions": [...]        // 题目列表
 *     }
 *   ]
 * }
 *
 */
