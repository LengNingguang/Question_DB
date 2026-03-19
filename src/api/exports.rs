//! Export pipelines for the question-first model.

use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use csv::WriterBuilder;
use serde_json::{json, Value};
use sqlx::{query, PgPool, Row};

use super::{models::ExportFormat, utils::canonical_or_original};

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

pub(crate) async fn fetch_text_object(
    pool: &PgPool,
    object_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(object_id) = object_id else {
        return Ok(None);
    };

    let row = query("SELECT content FROM object_blobs WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("query object blob failed: {object_id}"))?;

    Ok(row.map(|r| {
        let content: Vec<u8> = r.get("content");
        String::from_utf8_lossy(&content).to_string()
    }))
}

pub(crate) async fn export_jsonl(
    pool: &PgPool,
    output_path: &Path,
    include_answers: bool,
) -> Result<usize> {
    let rows = query(
        r#"
        SELECT q.question_id, q.category, q.question_tex_object_id::text AS question_tex_object_id,
               q.answer_tex_object_id::text AS answer_tex_object_id, q.search_text, q.status, q.tags_json
        FROM questions q
        ORDER BY q.question_id
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
        let question_tex_id: Option<String> = row.get("question_tex_object_id");
        let answer_tex_id: Option<String> = row.get("answer_tex_object_id");

        let assets = query(
            r#"
            SELECT asset_id, kind, object_id::text AS object_id, caption, sort_order
            FROM question_assets
            WHERE question_id = $1
            ORDER BY sort_order, asset_id
            "#,
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query assets for export failed: {}", question_id))?
        .into_iter()
        .map(|asset| {
            json!({
                "asset_id": asset.get::<String, _>("asset_id"),
                "kind": asset.get::<String, _>("kind"),
                "object_id": asset.get::<String, _>("object_id"),
                "caption": asset.get::<Option<String>, _>("caption"),
                "sort_order": asset.get::<i32, _>("sort_order"),
            })
        })
        .collect::<Vec<_>>();

        let papers = query(
            r#"
            SELECT p.paper_id, p.edition, p.paper_type, p.title, pq.sort_order, pq.question_label
            FROM paper_questions pq
            JOIN papers p ON p.paper_id = pq.paper_id
            WHERE pq.question_id = $1
            ORDER BY p.paper_id, pq.sort_order
            "#,
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query paper refs for export failed: {}", question_id))?
        .into_iter()
        .map(|paper| {
            json!({
                "paper_id": paper.get::<String, _>("paper_id"),
                "edition": paper.get::<String, _>("edition"),
                "paper_type": paper.get::<String, _>("paper_type"),
                "title": paper.get::<String, _>("title"),
                "sort_order": paper.get::<i32, _>("sort_order"),
                "question_label": paper.get::<Option<String>, _>("question_label"),
            })
        })
        .collect::<Vec<_>>();

        let question_tex_source = fetch_text_object(pool, question_tex_id.as_deref()).await?;
        let answer_tex_source = fetch_text_object(pool, answer_tex_id.as_deref()).await?;

        let mut payload = json!({
            "question_id": question_id,
            "category": row.get::<String, _>("category"),
            "question_tex_object_id": question_tex_id,
            "question_tex_source": question_tex_source,
            "search_text": row.get::<Option<String>, _>("search_text"),
            "status": row.get::<String, _>("status"),
            "tags": row.get::<Value, _>("tags_json"),
            "assets": assets,
            "papers": papers,
        });

        if include_answers {
            payload["answer_tex_object_id"] = Value::String(answer_tex_id.unwrap_or_default());
            payload["answer_tex_source"] =
                answer_tex_source.map(Value::String).unwrap_or(Value::Null);
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
    include_answers: bool,
) -> Result<usize> {
    let rows = query(
        r#"
        SELECT q.question_id, q.category, q.status,
               q.question_tex_object_id::text AS question_tex_object_id,
               q.answer_tex_object_id::text AS answer_tex_object_id,
               q.search_text, q.tags_json
        FROM questions q
        ORDER BY q.question_id
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
        "category",
        "status",
        "question_tex_object_id",
        "answer_tex_object_id",
        "search_text",
        "tags",
    ])?;

    for row in &rows {
        let answer_tex: Option<String> = row.get("answer_tex_object_id");
        writer.write_record([
            row.get::<String, _>("question_id"),
            row.get::<String, _>("category"),
            row.get::<String, _>("status"),
            row.get::<Option<String>, _>("question_tex_object_id")
                .unwrap_or_default(),
            if include_answers {
                answer_tex.unwrap_or_default()
            } else {
                String::new()
            },
            row.get::<Option<String>, _>("search_text")
                .unwrap_or_default(),
            row.get::<Value, _>("tags_json").to_string(),
        ])?;
    }

    writer.flush().context("flush csv writer failed")?;
    Ok(rows.len())
}

pub(crate) fn exported_path(path: &Path) -> String {
    canonical_or_original(path)
}
