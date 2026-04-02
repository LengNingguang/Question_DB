use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use sqlx::{query, PgPool, Postgres, Row, Transaction};
use uuid::Uuid;
use zip::ZipArchive;

use super::models::{NormalizedCreatePaperRequest, PaperFileReplaceResponse, PaperImportResponse};

pub(crate) const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

pub(crate) async fn import_paper_zip(
    pool: &PgPool,
    file_name: Option<&str>,
    request: &NormalizedCreatePaperRequest,
    zip_bytes: Vec<u8>,
) -> Result<PaperImportResponse> {
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    validate_uploaded_zip(&zip_bytes)?;

    let paper_id = Uuid::new_v4().to_string();
    let normalized_file_name = normalize_upload_file_name(file_name);
    let mut tx = pool.begin().await.context("begin paper import tx failed")?;
    let append_object_id = insert_object_tx(
        &mut tx,
        &normalized_file_name,
        &zip_bytes,
        Some("application/zip"),
    )
    .await?;

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

    for (idx, question_id) in request.question_ids.iter().enumerate() {
        query(
            r#"
            INSERT INTO paper_questions (paper_id, question_id, sort_order, created_at)
            VALUES ($1::uuid, $2::uuid, $3, NOW())
            "#,
        )
        .bind(&paper_id)
        .bind(question_id)
        .bind(i32::try_from(idx + 1).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await
        .with_context(|| format!("insert paper question ref failed: {question_id}"))?;
    }

    tx.commit().await.context("commit paper import failed")?;

    Ok(PaperImportResponse {
        paper_id,
        file_name: normalized_file_name,
        question_count: request.question_ids.len(),
        status: "imported",
    })
}

pub(crate) async fn replace_paper_zip(
    pool: &PgPool,
    paper_id: &str,
    file_name: Option<&str>,
    zip_bytes: Vec<u8>,
) -> Result<PaperFileReplaceResponse> {
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    validate_uploaded_zip(&zip_bytes)?;

    let normalized_file_name = normalize_upload_file_name(file_name);
    let mut tx = pool
        .begin()
        .await
        .context("begin paper file replace tx failed")?;

    let previous_object_id = query(
        "SELECT append_object_id::text AS append_object_id FROM papers WHERE paper_id = $1::uuid",
    )
    .bind(paper_id)
    .fetch_optional(&mut *tx)
    .await
    .context("load paper appendix reference failed")?
    .map(|row| row.get::<String, _>("append_object_id"))
    .ok_or_else(|| anyhow::anyhow!("paper not found: {paper_id}"))?;

    let append_object_id = insert_object_tx(
        &mut tx,
        &normalized_file_name,
        &zip_bytes,
        Some("application/zip"),
    )
    .await?;

    query("UPDATE papers SET append_object_id = $2::uuid, updated_at = NOW() WHERE paper_id = $1::uuid")
        .bind(paper_id)
        .bind(&append_object_id)
        .execute(&mut *tx)
        .await
        .context("update paper appendix object failed")?;

    query("DELETE FROM objects WHERE object_id = $1::uuid")
        .bind(&previous_object_id)
        .execute(&mut *tx)
        .await
        .context("delete previous paper appendix object failed")?;

    tx.commit()
        .await
        .context("commit paper file replace failed")?;

    Ok(PaperFileReplaceResponse {
        paper_id: paper_id.to_string(),
        file_name: normalized_file_name,
        status: "replaced",
    })
}

fn validate_uploaded_zip(zip_bytes: &[u8]) -> Result<()> {
    let cursor = Cursor::new(zip_bytes);
    ZipArchive::new(cursor).context("open zip archive failed")?;
    Ok(())
}

fn normalize_upload_file_name(file_name: Option<&str>) -> String {
    let candidate = file_name
        .and_then(|value| Path::new(value).file_name())
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty());

    candidate.unwrap_or_else(|| "paper.zip".to_string())
}

async fn insert_object_tx(
    tx: &mut Transaction<'_, Postgres>,
    file_name: &str,
    bytes: &[u8],
    mime_type: Option<&str>,
) -> Result<String> {
    let object_id = Uuid::new_v4().to_string();
    let source_path = PathBuf::from(file_name);
    let normalized_file_name = source_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "blob.bin".to_string());

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

#[cfg(test)]
mod tests {
    use super::{validate_uploaded_zip, MAX_UPLOAD_BYTES};
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn build_zip() -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        writer.start_file("meta/info.json", options).unwrap();
        writer.write_all(br#"{"kind":"paper"}"#).unwrap();
        writer.start_file("appendices/raw.bin", options).unwrap();
        writer.write_all(b"payload").unwrap();

        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn validate_uploaded_zip_accepts_any_non_empty_layout() {
        validate_uploaded_zip(&build_zip()).expect("zip should parse");
    }

    #[test]
    fn validate_uploaded_zip_rejects_invalid_zip_bytes() {
        let err = validate_uploaded_zip(b"not-a-zip").expect_err("should reject");
        assert!(err.to_string().contains("open zip archive failed"));
    }

    #[test]
    fn upload_limit_constant_matches_requirement() {
        assert_eq!(MAX_UPLOAD_BYTES, 20 * 1024 * 1024);
    }
}
