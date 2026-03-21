//! Data quality audits for stored objects and paper composition.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{query, PgPool, Row};

#[derive(Debug, Serialize)]
pub(crate) struct QualityReport {
    pub(crate) missing_tex_object: Vec<String>,
    pub(crate) missing_tex_source: Vec<String>,
    pub(crate) missing_asset_objects: Vec<Value>,
    pub(crate) empty_papers: Vec<String>,
}

pub(crate) async fn object_exists(pool: &PgPool, object_id: &str) -> Result<bool> {
    Ok(query("SELECT 1 FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("check object existence failed: {object_id}"))?
        .is_some())
}

pub(crate) async fn object_blob_nonempty(pool: &PgPool, object_id: &str) -> Result<bool> {
    let row = query("SELECT octet_length(content) AS size FROM objects WHERE object_id = $1::uuid")
        .bind(object_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("check object blob failed: {object_id}"))?;

    Ok(row
        .and_then(|r| r.try_get::<Option<i32>, _>("size").ok().flatten())
        .unwrap_or(0)
        > 0)
}

pub(crate) async fn build_quality_report(pool: &PgPool) -> Result<QualityReport> {
    let mut report = QualityReport {
        missing_tex_object: Vec::new(),
        missing_tex_source: Vec::new(),
        missing_asset_objects: Vec::new(),
        empty_papers: Vec::new(),
    };

    let question_rows = query("SELECT question_id::text AS question_id FROM questions")
        .fetch_all(pool)
        .await
        .context("query questions for quality report failed")?;

    for row in question_rows {
        let question_id: String = row.get("question_id");
        let tex_rows = query(
            "SELECT object_id::text AS object_id, file_path FROM question_files WHERE question_id = $1::uuid AND file_kind = 'tex'",
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query tex files failed: {question_id}"))?;

        if tex_rows.is_empty() {
            report.missing_tex_object.push(question_id.clone());
        }

        for tex_row in tex_rows {
            let object_id: String = tex_row.get("object_id");
            if !object_exists(pool, &object_id).await? {
                report.missing_tex_object.push(question_id.clone());
            } else if !object_blob_nonempty(pool, &object_id).await? {
                report.missing_tex_source.push(question_id.clone());
            }
        }

        let asset_rows = query(
            "SELECT object_id::text AS object_id, file_path FROM question_files WHERE question_id = $1::uuid AND file_kind = 'asset'",
        )
        .bind(&question_id)
        .fetch_all(pool)
        .await
        .with_context(|| format!("query asset files failed: {question_id}"))?;

        for asset_row in asset_rows {
            let object_id: String = asset_row.get("object_id");
            if !object_exists(pool, &object_id).await? {
                report.missing_asset_objects.push(json!({
                    "question_id": question_id,
                    "file_path": asset_row.get::<String, _>("file_path"),
                    "object_id": object_id,
                }));
            }
        }
    }

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

    report.empty_papers = paper_rows
        .into_iter()
        .map(|row| row.get::<String, _>("paper_id"))
        .collect();

    Ok(report)
}
