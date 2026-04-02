use std::{
    fs::File,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    http::{header, HeaderValue, Response, StatusCode},
};
use serde::Serialize;
use sqlx::{query, PgPool, Row};
use tokio::fs;
use tokio_util::io::ReaderStream;
use uuid::Uuid;
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::api::{
    ops::paper_render::{
        render_paper_bundle, PaperTemplateKind, RenderPaperInput, RenderQuestionAssetInput,
        RenderQuestionInput,
    },
    papers::models::PaperDetail,
    questions::{
        models::{QuestionAssetRef, QuestionDetail, QuestionPaperRef},
        queries::{
            load_question_difficulties, load_question_files, load_question_tags, map_paper_detail,
            map_paper_question_summary, map_question_detail,
        },
    },
    shared::utils::bundle_directory_name,
};

#[derive(Debug, Serialize)]
struct QuestionBundleManifest {
    kind: &'static str,
    generated_at_unix: u64,
    question_count: usize,
    questions: Vec<QuestionBundleManifestItem>,
}

#[derive(Debug, Serialize)]
struct QuestionBundleManifestItem {
    question_id: String,
    directory: String,
    metadata: QuestionDetail,
    files: Vec<BundleFileEntry>,
}

#[derive(Debug, Serialize)]
struct PaperBundleManifest {
    kind: &'static str,
    generated_at_unix: u64,
    paper_count: usize,
    papers: Vec<PaperBundleManifestItem>,
}

#[derive(Debug, Serialize)]
struct PaperBundleManifestItem {
    paper_id: String,
    directory: String,
    metadata: PaperDetail,
    template_source: String,
    append_file: BundleFileEntry,
    main_tex_file: BundleFileEntry,
    assets: Vec<BundleFileEntry>,
    questions: Vec<PaperBundleQuestionManifestItem>,
}

#[derive(Debug, Serialize)]
struct PaperBundleQuestionManifestItem {
    question_id: String,
    sequence: usize,
    source_tex_path: String,
    asset_prefix: String,
    metadata: QuestionDetail,
}

#[derive(Debug, Clone, Serialize)]
struct BundleFileEntry {
    zip_path: String,
    original_path: String,
    file_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_question_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_id: Option<String>,
    mime_type: Option<String>,
}

#[derive(Debug)]
struct QuestionBundleData {
    metadata: QuestionDetail,
    files: Vec<QuestionAssetRef>,
}

#[derive(Debug)]
struct PaperBundleData {
    metadata: PaperDetail,
    appendix: PaperAppendixData,
    questions: Vec<QuestionBundleData>,
}

#[derive(Debug)]
struct PaperAppendixData {
    object_id: String,
    original_file_name: String,
    mime_type: Option<String>,
}

pub(crate) async fn build_question_bundle_response(
    pool: &PgPool,
    question_ids: &[String],
) -> Result<Response<Body>> {
    let bundle_name = format!("questions_bundle_{}.zip", timestamp_unix());
    let zip_path = temp_zip_path("questions");
    let file = File::create(&zip_path).with_context(|| {
        format!(
            "create question bundle zip failed: {}",
            zip_path.to_string_lossy()
        )
    })?;
    let mut writer = ZipWriter::new(file);
    let mut manifest_items = Vec::with_capacity(question_ids.len());

    for question_id in question_ids {
        let bundle = load_question_bundle_data(pool, question_id).await?;
        let directory = bundle_directory_name(&bundle.metadata.description, question_id);
        let manifest_files =
            write_question_bundle_files(pool, &mut writer, &bundle.files, &directory).await?;
        manifest_items.push(QuestionBundleManifestItem {
            question_id: question_id.clone(),
            directory,
            metadata: bundle.metadata,
            files: manifest_files,
        });
    }

    let manifest = QuestionBundleManifest {
        kind: "question_bundle",
        generated_at_unix: timestamp_unix(),
        question_count: manifest_items.len(),
        questions: manifest_items,
    };
    write_manifest(&mut writer, &manifest)?;
    finish_zip_response(writer, zip_path, &bundle_name).await
}

pub(crate) async fn build_paper_bundle_response(
    pool: &PgPool,
    paper_ids: &[String],
) -> Result<Response<Body>> {
    let bundle_name = format!("papers_bundle_{}.zip", timestamp_unix());
    let zip_path = temp_zip_path("papers");
    let file = File::create(&zip_path).with_context(|| {
        format!(
            "create paper bundle zip failed: {}",
            zip_path.to_string_lossy()
        )
    })?;
    let mut writer = ZipWriter::new(file);
    let mut manifest_items = Vec::with_capacity(paper_ids.len());

    for paper_id in paper_ids {
        let bundle = load_paper_bundle_data(pool, paper_id).await?;
        let directory = bundle_directory_name(&bundle.metadata.description, paper_id);
        let append_file =
            write_paper_appendix_file(pool, &mut writer, &bundle.appendix, &directory).await?;
        let rendered = render_paper_bundle(build_render_paper_input(pool, &bundle).await?)?;

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

    let manifest = PaperBundleManifest {
        kind: "paper_bundle",
        generated_at_unix: timestamp_unix(),
        paper_count: manifest_items.len(),
        papers: manifest_items,
    };
    write_manifest(&mut writer, &manifest)?;
    finish_zip_response(writer, zip_path, &bundle_name).await
}

async fn write_question_bundle_files(
    pool: &PgPool,
    writer: &mut ZipWriter<File>,
    files: &[QuestionAssetRef],
    directory: &str,
) -> Result<Vec<BundleFileEntry>> {
    let mut manifest_entries = Vec::with_capacity(files.len());

    for file in files {
        let zip_path = format!("{directory}/{}", file.path);
        let bytes = fetch_object_bytes(pool, &file.object_id).await?;
        write_bundle_file(writer, &zip_path, &bytes)?;

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

async fn write_paper_appendix_file(
    pool: &PgPool,
    writer: &mut ZipWriter<File>,
    appendix: &PaperAppendixData,
    directory: &str,
) -> Result<BundleFileEntry> {
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

fn write_bundle_file(writer: &mut ZipWriter<File>, zip_path: &str, bytes: &[u8]) -> Result<()> {
    writer
        .start_file(zip_path, SimpleFileOptions::default())
        .context("start bundle file entry failed")?;
    writer
        .write_all(bytes)
        .with_context(|| format!("write bundle file failed: {zip_path}"))?;
    Ok(())
}

fn write_manifest<T: Serialize>(writer: &mut ZipWriter<File>, manifest: &T) -> Result<()> {
    writer
        .start_file("manifest.json", SimpleFileOptions::default())
        .context("start manifest.json failed")?;
    let bytes = serde_json::to_vec_pretty(manifest).context("serialize manifest.json failed")?;
    writer
        .write_all(&bytes)
        .context("write manifest.json failed")?;
    Ok(())
}

async fn finish_zip_response(
    writer: ZipWriter<File>,
    zip_path: PathBuf,
    bundle_name: &str,
) -> Result<Response<Body>> {
    let file = writer.finish().context("finish zip archive failed")?;
    let size = file
        .metadata()
        .context("read zip metadata failed")?
        .len()
        .to_string();
    drop(file);

    let std_file = File::open(&zip_path)
        .with_context(|| format!("open finished zip failed: {}", zip_path.to_string_lossy()))?;
    std::fs::remove_file(&zip_path).ok();
    let stream = ReaderStream::new(fs::File::from_std(std_file));
    let body = Body::from_stream(stream);

    let content_type = HeaderValue::from_static("application/zip");
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{bundle_name}\""))
        .context("build content-disposition header failed")?;
    let content_length =
        HeaderValue::from_str(&size).context("build content-length header failed")?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, content_length)
        .body(body)
        .context("build zip response failed")
}

async fn load_question_bundle_data(pool: &PgPool, question_id: &str) -> Result<QuestionBundleData> {
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

    let tex_files = load_question_files(pool, question_id, "tex")
        .await
        .with_context(|| format!("load question tex files failed: {question_id}"))?;
    let tex_object_id = tex_files
        .first()
        .map(|file| file.object_id.clone())
        .ok_or_else(|| anyhow!("question is missing a tex object: {question_id}"))?;
    let assets = load_question_files(pool, question_id, "asset")
        .await
        .with_context(|| format!("load question assets failed: {question_id}"))?;
    let tags = load_question_tags(pool, question_id)
        .await
        .with_context(|| format!("load question tags failed: {question_id}"))?;
    let difficulty = load_question_difficulties(pool, question_id)
        .await
        .with_context(|| format!("load question difficulties failed: {question_id}"))?;

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

    let mut files = tex_files.clone();
    files.extend(assets.clone());

    Ok(QuestionBundleData {
        metadata: map_question_detail(row, tex_object_id, tags, difficulty, assets, papers),
        files,
    })
}

async fn load_paper_bundle_data(pool: &PgPool, paper_id: &str) -> Result<PaperBundleData> {
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

    let mut question_summaries = Vec::with_capacity(question_rows.len());
    let mut questions = Vec::with_capacity(question_rows.len());
    for row in question_rows {
        let question_id: String = row.get("question_id");
        let tags = load_question_tags(pool, &question_id)
            .await
            .with_context(|| format!("load paper question tags failed: {question_id}"))?;
        question_summaries.push(map_paper_question_summary(row, tags));
        questions.push(load_question_bundle_data(pool, &question_id).await?);
    }

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

async fn build_render_paper_input(
    pool: &PgPool,
    bundle: &PaperBundleData,
) -> Result<RenderPaperInput> {
    let template_kind = determine_paper_template_kind(&bundle.questions)?;
    let mut questions = Vec::with_capacity(bundle.questions.len());

    for (index, question) in bundle.questions.iter().enumerate() {
        let tex_bytes = fetch_object_bytes(pool, &question.metadata.tex_object_id).await?;
        let source_tex = String::from_utf8(tex_bytes).with_context(|| {
            format!(
                "question tex object is not valid UTF-8: {}",
                question.metadata.tex_object_id
            )
        })?;

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

fn determine_paper_template_kind(questions: &[QuestionBundleData]) -> Result<PaperTemplateKind> {
    let first_question = questions
        .first()
        .ok_or_else(|| anyhow!("paper does not contain any questions"))?;
    let expected_category = first_question.metadata.category.as_str();
    let template_kind = match expected_category {
        "T" => PaperTemplateKind::Theory,
        "E" => PaperTemplateKind::Experiment,
        other => {
            return Err(anyhow!(
                "paper questions must all be category T or E before rendering, found {other}"
            ));
        }
    };

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

async fn fetch_object_bytes(pool: &PgPool, object_id: &str) -> Result<Vec<u8>> {
    let row = query("SELECT content FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_one(pool)
        .await
        .with_context(|| format!("load object content failed: {object_id}"))?;
    Ok(row.get("content"))
}

fn temp_zip_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "qb_{prefix}_bundle_{}_{}.zip",
        timestamp_unix(),
        Uuid::new_v4()
    ))
}

fn timestamp_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
