//! ZIP-based question import helpers.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    path::{Component, Path},
};

use anyhow::{bail, Context, Result};
use mime_guess::MimeGuess;
use sqlx::{query, PgPool, Postgres, QueryBuilder, Row, Transaction};
use uuid::Uuid;
use zip::ZipArchive;

use super::models::{
    NormalizedQuestionDifficulty, QuestionFileReplaceResponse, QuestionImportResponse,
};

pub(crate) const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
struct ArchiveFile {
    path: String,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct LoadedQuestionZip {
    tex_file: ArchiveFile,
    asset_files: Vec<ArchiveFile>,
}

pub(crate) async fn import_question_zip(
    pool: &PgPool,
    file_name: Option<&str>,
    description: &str,
    difficulty: &NormalizedQuestionDifficulty,
    zip_bytes: Vec<u8>,
) -> Result<QuestionImportResponse> {
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    let loaded = load_question_zip(&zip_bytes)?;
    let question_id = Uuid::new_v4().to_string();
    let mut tx = pool
        .begin()
        .await
        .context("begin question import tx failed")?;

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

    insert_loaded_question_files_tx(&mut tx, &question_id, &loaded).await?;

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

    tx.commit().await.context("commit question import failed")?;

    Ok(QuestionImportResponse {
        question_id,
        file_name: normalize_upload_file_name(file_name),
        imported_assets: loaded.asset_files.len(),
        status: "imported",
    })
}

pub(crate) async fn replace_question_zip(
    pool: &PgPool,
    question_id: &str,
    file_name: Option<&str>,
    zip_bytes: Vec<u8>,
) -> Result<QuestionFileReplaceResponse> {
    if zip_bytes.is_empty() {
        bail!("uploaded file is empty");
    }
    if zip_bytes.len() > MAX_UPLOAD_BYTES {
        bail!("uploaded zip exceeds 20 MiB limit");
    }

    let loaded = load_question_zip(&zip_bytes)?;
    let normalized_file_name = normalize_upload_file_name(file_name);
    let mut tx = pool
        .begin()
        .await
        .context("begin question file replace tx failed")?;

    let exists = query("SELECT 1 FROM questions WHERE question_id = $1::uuid")
        .bind(question_id)
        .fetch_optional(&mut *tx)
        .await
        .context("check question existence failed")?
        .is_some();
    if !exists {
        bail!("question not found: {question_id}");
    }

    replace_question_files_tx(&mut tx, question_id, &loaded).await?;

    query(
        "UPDATE questions SET source_tex_path = $2, updated_at = NOW() WHERE question_id = $1::uuid",
    )
    .bind(question_id)
    .bind(&loaded.tex_file.path)
    .execute(&mut *tx)
    .await
    .context("update question source tex path failed")?;

    tx.commit()
        .await
        .context("commit question file replace failed")?;

    Ok(QuestionFileReplaceResponse {
        question_id: question_id.to_string(),
        file_name: normalized_file_name,
        source_tex_path: loaded.tex_file.path,
        imported_assets: loaded.asset_files.len(),
        status: "replaced",
    })
}

fn load_question_zip(zip_bytes: &[u8]) -> Result<LoadedQuestionZip> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).context("open zip archive failed")?;
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut total_uncompressed = 0usize;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .with_context(|| format!("read zip entry #{idx} failed"))?;
        let raw_name = entry.name().to_string();
        let path = sanitize_archive_path(&raw_name)?;
        if raw_name.ends_with('/') {
            directories.insert(path);
            continue;
        }
        let size_hint = usize::try_from(entry.size()).unwrap_or(usize::MAX);
        total_uncompressed = total_uncompressed.saturating_add(size_hint);
        if total_uncompressed > MAX_TOTAL_UNCOMPRESSED_BYTES {
            bail!("zip expands beyond the allowed uncompressed size");
        }

        let mut bytes = Vec::with_capacity(size_hint.min(1024 * 1024));
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("read zip entry bytes failed: {path}"))?;
        register_parent_directories(&mut directories, &path);
        files.insert(path.clone(), ArchiveFile { path, bytes });
    }

    let (tex_file, asset_files) = validate_standard_layout(&files, &directories)?;

    Ok(LoadedQuestionZip {
        tex_file,
        asset_files,
    })
}

fn sanitize_archive_path(path: &str) -> Result<String> {
    let normalized = path.replace('\\', "/");
    let candidate = Path::new(&normalized);
    if candidate.is_absolute() {
        bail!("zip entry must be relative: {path}");
    }

    let mut cleaned = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => cleaned.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("zip entry has unsafe path: {path}");
            }
        }
    }

    let joined = cleaned.join("/");
    if joined.is_empty() {
        bail!("zip entry has empty path");
    }
    Ok(joined)
}

fn validate_standard_layout(
    files: &BTreeMap<String, ArchiveFile>,
    directories: &BTreeSet<String>,
) -> Result<(ArchiveFile, Vec<ArchiveFile>)> {
    let mut root_tex_files = Vec::new();
    let mut asset_files = Vec::new();
    let root_directories = directories
        .iter()
        .filter(|dir| !dir.contains('/'))
        .cloned()
        .collect::<BTreeSet<_>>();

    for file in files.values() {
        let components = file.path.split('/').collect::<Vec<_>>();
        match components.as_slice() {
            [file_name] => {
                if is_tex_file(file_name) {
                    root_tex_files.push(file.clone());
                } else {
                    bail!(
                        "zip root may only contain one .tex file and one assets/ directory, found unexpected file: {}",
                        file.path
                    );
                }
            }
            [root_dir, ..] => {
                if *root_dir != "assets" {
                    bail!(
                        "all non-root files must be inside the root assets/ directory, found: {}",
                        file.path
                    );
                }
                asset_files.push(file.clone());
            }
            [] => bail!("zip entry has empty path"),
        }
    }

    if root_tex_files.len() != 1 {
        bail!(
            "zip root must contain exactly one .tex file, found {}",
            root_tex_files.len()
        );
    }

    if !root_directories.iter().any(|dir| dir == "assets") {
        bail!("zip root must contain exactly one assets/ directory");
    }
    if root_directories.len() != 1 {
        bail!("zip root must contain exactly one directory named assets/");
    }

    Ok((root_tex_files.remove(0), asset_files))
}

fn is_tex_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("tex"))
        .unwrap_or(false)
}

fn register_parent_directories(directories: &mut BTreeSet<String>, path: &str) {
    let components = path.split('/').collect::<Vec<_>>();
    if components.len() <= 1 {
        return;
    }

    for idx in 1..components.len() {
        directories.insert(components[..idx].join("/"));
    }
}

fn normalize_upload_file_name(file_name: Option<&str>) -> String {
    file_name
        .and_then(|value| Path::new(value).file_name())
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "question.zip".to_string())
}

async fn insert_loaded_question_files_tx(
    tx: &mut Transaction<'_, Postgres>,
    question_id: &str,
    loaded: &LoadedQuestionZip,
) -> Result<()> {
    let tex_object_id = insert_object_tx(
        tx,
        Path::new(&loaded.tex_file.path),
        &loaded.tex_file.bytes,
        Some("text/x-tex"),
    )
    .await?;
    insert_question_file_tx(
        tx,
        question_id,
        &tex_object_id,
        "tex",
        &loaded.tex_file.path,
        Some("text/x-tex"),
    )
    .await?;

    for asset in &loaded.asset_files {
        let mime = MimeGuess::from_path(&asset.path)
            .first_raw()
            .map(str::to_string);
        let object_id =
            insert_object_tx(tx, Path::new(&asset.path), &asset.bytes, mime.as_deref()).await?;
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

async fn replace_question_files_tx(
    tx: &mut Transaction<'_, Postgres>,
    question_id: &str,
    loaded: &LoadedQuestionZip,
) -> Result<()> {
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

    query("DELETE FROM question_files WHERE question_id = $1::uuid")
        .bind(question_id)
        .execute(&mut **tx)
        .await
        .context("delete existing question files failed")?;

    if !old_object_ids.is_empty() {
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

    insert_loaded_question_files_tx(tx, question_id, loaded).await
}

async fn insert_object_tx(
    tx: &mut Transaction<'_, Postgres>,
    source_path: &Path,
    bytes: &[u8],
    mime_type: Option<&str>,
) -> Result<String> {
    let object_id = Uuid::new_v4().to_string();
    let file_name = source_path
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
    .bind(&file_name)
    .bind(mime_type)
    .bind(i64::try_from(bytes.len()).context("object bytes exceed i64 range")?)
    .bind(bytes)
    .execute(&mut **tx)
    .await
    .context("insert object failed")?;

    Ok(object_id)
}

async fn insert_question_file_tx(
    tx: &mut Transaction<'_, Postgres>,
    question_id: &str,
    object_id: &str,
    file_kind: &str,
    file_path: &str,
    mime_type: Option<&str>,
) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::{load_question_zip, MAX_UPLOAD_BYTES};
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn build_zip() -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        writer.start_file("problem.tex", options).unwrap();
        writer.write_all(br"\section{Demo}").unwrap();
        writer.start_file("assets/fig1.png", options).unwrap();
        writer.write_all(b"png").unwrap();

        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn load_question_zip_reads_standard_layout() {
        let loaded = load_question_zip(&build_zip()).expect("zip should parse");
        assert_eq!(loaded.tex_file.path, "problem.tex");
        assert_eq!(loaded.asset_files.len(), 1);
    }

    #[test]
    fn load_question_zip_rejects_extra_root_file() {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        writer.start_file("problem.tex", options).unwrap();
        writer.write_all(br"\section{Demo}").unwrap();
        writer.start_file("readme.txt", options).unwrap();
        writer.write_all(b"nope").unwrap();
        writer.start_file("assets/fig1.png", options).unwrap();
        writer.write_all(b"png").unwrap();

        let zip = writer.finish().unwrap().into_inner();
        let err = load_question_zip(&zip).expect_err("zip should be rejected");
        assert!(err.to_string().contains("unexpected file"));
    }

    #[test]
    fn upload_limit_constant_matches_requirement() {
        assert_eq!(MAX_UPLOAD_BYTES, 20 * 1024 * 1024);
    }
}
