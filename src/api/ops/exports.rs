//! Export pipelines for uploaded question metadata.

use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use csv::WriterBuilder;
use serde_json::json;
use sqlx::{query, PgPool, Row};

use super::models::ExportFormat;
use crate::api::{
    questions::{
        models::{QuestionDifficulty, QuestionSourceRef},
        queries::{load_question_algorithms, load_question_files, load_question_tags},
    },
    shared::utils::canonical_or_original,
};

pub(crate) fn default_export_path(format: ExportFormat, is_public: bool) -> PathBuf {
    let suffix = if is_public { "public" } else { "internal" };
    let ext = match format {
        ExportFormat::Jsonl => "jsonl",
        ExportFormat::Csv => "csv",
    };
    PathBuf::from("exports").join(format!("question_bank_{suffix}.{ext}"))
}

pub(crate) fn ensure_parent_dir(output_path: &Path, label: &str) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create {label} parent directory failed: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    Ok(())
}

pub(crate) async fn fetch_text_object(pool: &PgPool, object_id: &str) -> Result<String> {
    let row = query("SELECT content FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_one(pool)
        .await
        .with_context(|| format!("query object failed: {object_id}"))?;

    let content: Vec<u8> = row.get("content");
    Ok(String::from_utf8_lossy(&content).to_string())
}

pub(crate) async fn export_jsonl(
    pool: &PgPool,
    output_path: &Path,
    include_tex_source: bool,
) -> Result<usize> {
    let rows = query(
        r#"
        SELECT question_id::text AS question_id, source_tex_path, category, status,
               COALESCE(notes, '') AS notes,
               difficulty_human, difficulty_notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        ORDER BY created_at DESC, question_id
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query questions for jsonl export failed")?;

    let file = fs::File::create(output_path).with_context(|| {
        format!(
            "create export file failed: {}",
            output_path.to_string_lossy()
        )
    })?;
    let mut writer = BufWriter::new(file);

    for row in &rows {
        let question_id: String = row.get("question_id");
        let tex_files = load_question_files(pool, &question_id, "tex").await?;
        let assets = load_question_files(pool, &question_id, "asset").await?;
        let tags = load_question_tags(pool, &question_id).await?;
        let algorithms = load_question_algorithms(pool, &question_id).await?;
        let tex_object_id = tex_files
            .first()
            .map(|file| file.object_id.clone())
            .unwrap_or_default();

        let mut payload = json!({
            "question_id": question_id,
            "tex_object_id": tex_object_id,
            "source": QuestionSourceRef {
                tex: row.get("source_tex_path"),
            },
            "category": row.get::<String, _>("category"),
            "status": row.get::<String, _>("status"),
            "notes": row.get::<String, _>("notes"),
            "difficulty": QuestionDifficulty {
                human: row.get("difficulty_human"),
                algorithm: algorithms,
                notes: row.get("difficulty_notes"),
            },
            "tags": tags,
            "assets": assets,
            "created_at": row.get::<String, _>("created_at"),
            "updated_at": row.get::<String, _>("updated_at"),
        });

        if include_tex_source && !tex_object_id.is_empty() {
            payload["tex_source"] =
                serde_json::Value::String(fetch_text_object(pool, &tex_object_id).await?);
        }

        writer
            .write_all(serde_json::to_string(&payload)?.as_bytes())
            .context("write jsonl line failed")?;
        writer.write_all(b"\n").context("write newline failed")?;
    }

    writer.flush().context("flush jsonl writer failed")?;
    Ok(rows.len())
}

pub(crate) async fn export_csv(
    pool: &PgPool,
    output_path: &Path,
    include_tex_source: bool,
) -> Result<usize> {
    let rows = query(
        r#"
        SELECT question_id::text AS question_id, source_tex_path, category, status,
               COALESCE(notes, '') AS notes,
               difficulty_human, difficulty_notes,
               to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
        FROM questions
        ORDER BY created_at DESC, question_id
        "#,
    )
    .fetch_all(pool)
    .await
    .context("query questions for csv export failed")?;

    let file = fs::File::create(output_path).with_context(|| {
        format!(
            "create export csv failed: {}",
            output_path.to_string_lossy()
        )
    })?;
    let mut writer = WriterBuilder::new().from_writer(file);

    writer.write_record([
        "question_id",
        "tex_object_id",
        "source_tex_path",
        "category",
        "status",
        "notes",
        "difficulty_human",
        "difficulty_algorithm",
        "difficulty_notes",
        "tags",
        "created_at",
        "updated_at",
        "tex_source",
    ])?;

    for row in &rows {
        let question_id: String = row.get("question_id");
        let tex_files = load_question_files(pool, &question_id, "tex").await?;
        let tags = load_question_tags(pool, &question_id).await?;
        let algorithms = load_question_algorithms(pool, &question_id).await?;
        let tex_object_id = tex_files
            .first()
            .map(|file| file.object_id.clone())
            .unwrap_or_default();

        writer.write_record([
            question_id,
            tex_object_id.clone(),
            row.get::<String, _>("source_tex_path"),
            row.get::<String, _>("category"),
            row.get::<String, _>("status"),
            row.get::<String, _>("notes"),
            row.get::<Option<i32>, _>("difficulty_human")
                .map(|value| value.to_string())
                .unwrap_or_default(),
            serde_json::to_string(&algorithms)?,
            row.get::<Option<String>, _>("difficulty_notes")
                .unwrap_or_default(),
            serde_json::to_string(&tags)?,
            row.get::<String, _>("created_at"),
            row.get::<String, _>("updated_at"),
            if include_tex_source && !tex_object_id.is_empty() {
                fetch_text_object(pool, &tex_object_id).await?
            } else {
                String::new()
            },
        ])?;
    }

    writer.flush().context("flush csv writer failed")?;
    Ok(rows.len())
}

pub(crate) fn exported_path(path: &Path) -> String {
    canonical_or_original(path)
}
