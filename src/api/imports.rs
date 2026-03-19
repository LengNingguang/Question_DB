//! Question import helpers for the question-first model.

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use mime_guess::MimeGuess;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{query, PgPool, Postgres, Row, Transaction};
use strsim::normalized_levenshtein;
use uuid::Uuid;

use super::{
    models::{
        QuestionImportCommitResponse, QuestionImportRequest, QuestionImportValidationResponse,
    },
    utils::{
        canonical_or_original, expand_path, join_bundle_path, missing_keys, normalize_search_text,
        read_json_file,
        sha256_hex,
    },
};

const OBJECT_BUCKET: &str = "local";
const SIMILARITY_THRESHOLD: f64 = 0.92;

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct QuestionSourceManifest {
    pub(crate) source_name: String,
    pub(crate) run_label: String,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BundleQuestion {
    pub(crate) question_id: String,
    pub(crate) category: String,
    pub(crate) latex_path: String,
    pub(crate) answer_latex_path: Option<String>,
    #[serde(rename = "latex_anchor")]
    pub(crate) _latex_anchor: Option<String>,
    pub(crate) search_text: Option<String>,
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) assets: Vec<BundleAsset>,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BundleAsset {
    pub(crate) asset_id: String,
    pub(crate) kind: String,
    pub(crate) file_path: String,
    pub(crate) sha256: Option<String>,
    pub(crate) caption: Option<String>,
    pub(crate) sort_order: Option<i32>,
}

#[derive(Debug, Clone)]
pub(crate) struct HydratedQuestion {
    pub(crate) question: BundleQuestion,
    pub(crate) latex_source: String,
    pub(crate) answer_source: Option<String>,
    pub(crate) comparison_text: String,
}

#[derive(Debug)]
pub(crate) struct LoadedQuestionSource {
    pub(crate) manifest: QuestionSourceManifest,
    pub(crate) questions: Vec<BundleQuestion>,
}

#[derive(Debug, Default)]
pub(crate) struct ValidationResult {
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

impl ValidationResult {
    pub(crate) fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub(crate) fn validate_question_import_request(
    request: QuestionImportRequest,
) -> Result<QuestionImportValidationResponse> {
    let source_root = expand_path(&request.source_root);
    let (validation, _) = inspect_question_source(&source_root)?;
    Ok(QuestionImportValidationResponse {
        source_root: canonical_or_original(&source_root),
        ok: validation.ok(),
        warnings: validation.warnings,
        errors: validation.errors,
    })
}

pub(crate) async fn commit_question_import(
    pool: &PgPool,
    request: QuestionImportRequest,
) -> Result<QuestionImportCommitResponse> {
    let source_root = expand_path(&request.source_root);
    let (mut validation, loaded_opt) = inspect_question_source(&source_root)?;
    let run_label_override = loaded_opt.as_ref().map(|v| v.manifest.run_label.clone());

    let mut imported_questions = 0usize;
    let mut imported_assets = 0usize;

    let Some(loaded) = loaded_opt else {
        let response = QuestionImportCommitResponse {
            source_root: canonical_or_original(&source_root),
            status: "failed".to_string(),
            question_count: 0,
            imported_questions,
            imported_assets,
            warnings: validation.warnings.clone(),
            errors: validation.errors.clone(),
        };
        insert_import_run(
            pool,
            source_root.as_path(),
            None,
            false,
            &response.status,
            0,
            &response.warnings,
            &response.errors,
            None,
        )
        .await?;
        return Ok(response);
    };

    let question_count = loaded.questions.len();
    let hydrated_questions = hydrate_bundle_questions(&source_root, &loaded.questions)?;
    let (similarity_warnings, similarity_errors) =
        find_similarity_issues(pool, &hydrated_questions, request.allow_similar).await?;
    validation.warnings.extend(similarity_warnings);
    validation.errors.extend(similarity_errors);

    let status = if validation.errors.is_empty() {
        "committed"
    } else {
        "failed"
    }
    .to_string();

    if validation.errors.is_empty() {
        let mut tx = pool.begin().await.context("begin tx failed")?;

        for hydrated in &hydrated_questions {
            let latex_path = join_bundle_path(&source_root, &hydrated.question.latex_path);
            let question_tex_object_id = upsert_object_tx(
                &mut tx,
                "question_tex",
                &latex_path,
                hydrated.latex_source.as_bytes(),
                Some("text/x-tex"),
                "question_import",
            )
            .await?;

            let answer_tex_object_id =
                if let Some(answer_path_raw) = &hydrated.question.answer_latex_path {
                    let answer_path = join_bundle_path(&source_root, answer_path_raw);
                    if let Some(source) = &hydrated.answer_source {
                        Some(
                            upsert_object_tx(
                                &mut tx,
                                "answer_tex",
                                &answer_path,
                                source.as_bytes(),
                                Some("text/x-tex"),
                                "question_import",
                            )
                            .await?,
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };

            let search_text = hydrated
                .question
                .search_text
                .clone()
                .unwrap_or_else(|| hydrated.comparison_text.clone());

            query(
                r#"
                INSERT INTO questions (
                    question_id, category, question_tex_object_id, answer_tex_object_id,
                    search_text, status, tags_json, notes, created_at, updated_at
                )
                VALUES ($1, $2, $3::uuid, $4::uuid, $5, $6, $7, $8, NOW(), NOW())
                ON CONFLICT (question_id)
                DO UPDATE SET
                    category = EXCLUDED.category,
                    question_tex_object_id = EXCLUDED.question_tex_object_id,
                    answer_tex_object_id = EXCLUDED.answer_tex_object_id,
                    search_text = EXCLUDED.search_text,
                    status = EXCLUDED.status,
                    tags_json = EXCLUDED.tags_json,
                    notes = EXCLUDED.notes,
                    updated_at = NOW()
                "#,
            )
            .bind(&hydrated.question.question_id)
            .bind(&hydrated.question.category)
            .bind(&question_tex_object_id)
            .bind(answer_tex_object_id.as_deref())
            .bind(search_text)
            .bind(&hydrated.question.status)
            .bind(Value::Array(
                hydrated
                    .question
                    .tags
                    .iter()
                    .map(|v| Value::String(v.clone()))
                    .collect(),
            ))
            .bind(hydrated.question.notes.as_deref())
            .execute(&mut *tx)
            .await
            .with_context(|| {
                format!("upsert question failed: {}", hydrated.question.question_id)
            })?;
            imported_questions += 1;

            query("DELETE FROM question_assets WHERE question_id = $1")
                .bind(&hydrated.question.question_id)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!(
                        "delete existing assets failed: {}",
                        hydrated.question.question_id
                    )
                })?;

            for asset in &hydrated.question.assets {
                let asset_path = join_bundle_path(&source_root, &asset.file_path);
                let bytes = fs::read(&asset_path).with_context(|| {
                    format!("read asset failed: {}", asset_path.to_string_lossy())
                })?;
                let mime = MimeGuess::from_path(&asset_path)
                    .first_raw()
                    .map(|item| item.to_string())
                    .unwrap_or_else(|| "application/octet-stream".to_string());

                let object_id = upsert_object_tx(
                    &mut tx,
                    "question_asset",
                    &asset_path,
                    &bytes,
                    Some(&mime),
                    "question_import",
                )
                .await?;

                query(
                    r#"
                    INSERT INTO question_assets (
                        asset_id, question_id, kind, object_id, caption, sort_order, created_at
                    ) VALUES ($1, $2, $3, $4::uuid, $5, $6, NOW())
                    ON CONFLICT (asset_id)
                    DO UPDATE SET
                        question_id = EXCLUDED.question_id,
                        kind = EXCLUDED.kind,
                        object_id = EXCLUDED.object_id,
                        caption = EXCLUDED.caption,
                        sort_order = EXCLUDED.sort_order
                    "#,
                )
                .bind(&asset.asset_id)
                .bind(&hydrated.question.question_id)
                .bind(&asset.kind)
                .bind(&object_id)
                .bind(asset.caption.as_deref())
                .bind(asset.sort_order.unwrap_or(0))
                .execute(&mut *tx)
                .await
                .with_context(|| format!("insert asset failed: {}", asset.asset_id))?;
                imported_assets += 1;
            }
        }

        tx.commit().await.context("commit question import failed")?;
    }

    let response = QuestionImportCommitResponse {
        source_root: canonical_or_original(&source_root),
        status,
        question_count,
        imported_questions,
        imported_assets,
        warnings: validation.warnings.clone(),
        errors: validation.errors.clone(),
    };

    insert_import_run(
        pool,
        source_root.as_path(),
        Some(loaded.manifest.source_name.as_str()),
        false,
        &response.status,
        response.question_count,
        &response.warnings,
        &response.errors,
        run_label_override.as_deref(),
    )
    .await?;

    Ok(response)
}

pub(crate) fn inspect_question_source(
    source_root: &Path,
) -> Result<(ValidationResult, Option<LoadedQuestionSource>)> {
    let mut validation = ValidationResult::default();

    if !source_root.exists() {
        validation.errors.push(format!(
            "source root does not exist: {}",
            source_root.to_string_lossy()
        ));
        return Ok((validation, None));
    }
    if !source_root.is_dir() {
        validation.errors.push(format!(
            "source root must be a directory: {}",
            source_root.to_string_lossy()
        ));
        return Ok((validation, None));
    }

    let manifest_path = source_root.join("manifest.json");
    if !manifest_path.exists() {
        validation.errors.push("manifest.json is missing".to_string());
        return Ok((validation, None));
    }

    let manifest_value: Value = read_json_file(&manifest_path)?;
    let Some(manifest_obj) = manifest_value.as_object() else {
        validation
            .errors
            .push("manifest.json root must be an object".to_string());
        return Ok((validation, None));
    };

    let required_manifest_keys = ["source_name", "run_label"];
    let missing_manifest_keys = missing_keys(manifest_obj, &required_manifest_keys);
    if !missing_manifest_keys.is_empty() {
        validation.errors.push(format!(
            "manifest.json missing fields: {:?}",
            missing_manifest_keys
        ));
    }

    let questions_dir = source_root.join("questions");
    if !questions_dir.exists() {
        validation.errors.push("questions/ directory is missing".to_string());
        return Ok((validation, None));
    }

    let mut question_files = fs::read_dir(&questions_dir)
        .with_context(|| {
            format!(
                "read questions dir failed: {}",
                questions_dir.to_string_lossy()
            )
        })?
        .filter_map(|entry| entry.ok().map(|v| v.path()))
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    question_files.sort();

    if question_files.is_empty() {
        validation
            .errors
            .push("questions/ has no JSON files".to_string());
    }

    let mut parsed_questions = Vec::new();
    let mut seen_ids = HashSet::new();
    let allowed_question_keys: BTreeSet<&str> = BTreeSet::from([
        "question_id",
        "category",
        "latex_path",
        "answer_latex_path",
        "latex_anchor",
        "search_text",
        "status",
        "tags",
        "assets",
        "notes",
    ]);

    for (idx, path) in question_files.iter().enumerate() {
        let label = format!("question #{}", idx + 1);
        let value: Value = read_json_file(path)?;
        let Some(obj) = value.as_object() else {
            validation
                .errors
                .push(format!("{label} must be a JSON object"));
            continue;
        };

        let required_question_keys = [
            "question_id",
            "category",
            "latex_path",
            "status",
            "tags",
            "assets",
        ];
        let missing = missing_keys(obj, &required_question_keys);
        if !missing.is_empty() {
            validation
                .errors
                .push(format!("{label} missing fields: {:?}", missing));
        }

        let unknown = obj
            .keys()
            .filter(|key| !allowed_question_keys.contains(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            validation
                .warnings
                .push(format!("{label} has unknown fields: {:?}", unknown));
        }

        let parsed: BundleQuestion = match serde_json::from_value(value.clone()) {
            Ok(v) => v,
            Err(err) => {
                validation
                    .errors
                    .push(format!("{label} failed to parse: {err}"));
                continue;
            }
        };

        if !seen_ids.insert(parsed.question_id.clone()) {
            validation
                .errors
                .push(format!("duplicate question_id: {}", parsed.question_id));
        }

        if !["theory", "experiment"].contains(&parsed.category.as_str()) {
            validation.errors.push(format!(
                "{} category must be theory/experiment",
                parsed.question_id
            ));
        }
        if !["raw", "reviewed", "published"].contains(&parsed.status.as_str()) {
            validation.errors.push(format!(
                "{} status must be raw/reviewed/published",
                parsed.question_id
            ));
        }

        let latex_path = join_bundle_path(source_root, &parsed.latex_path);
        if !latex_path.exists() {
            validation.errors.push(format!(
                "{} latex_path does not exist: {}",
                parsed.question_id, parsed.latex_path
            ));
        }
        if let Some(answer_path) = &parsed.answer_latex_path {
            let resolved = join_bundle_path(source_root, answer_path);
            if !resolved.exists() {
                validation.errors.push(format!(
                    "{} answer_latex_path does not exist: {}",
                    parsed.question_id, answer_path
                ));
            }
        }

        for asset in &parsed.assets {
            if asset.file_path.trim().is_empty() {
                validation
                    .errors
                    .push(format!("{} asset missing file_path", parsed.question_id));
                continue;
            }
            let asset_path = join_bundle_path(source_root, &asset.file_path);
            if !asset_path.exists() {
                validation.errors.push(format!(
                    "{} asset does not exist: {}",
                    parsed.question_id, asset.file_path
                ));
                continue;
            }
            let bytes = fs::read(&asset_path)
                .with_context(|| format!("read asset failed: {}", asset_path.to_string_lossy()))?;
            let actual = sha256_hex(&bytes);
            if let Some(expected) = &asset.sha256 {
                if expected.to_uppercase() != actual {
                    validation.errors.push(format!(
                        "{} asset checksum mismatch: {}",
                        parsed.question_id, asset.file_path
                    ));
                }
            } else {
                validation.warnings.push(format!(
                    "{} asset has no sha256: {}",
                    parsed.question_id, asset.file_path
                ));
            }
        }

        parsed_questions.push(parsed);
    }

    let manifest = match serde_json::from_value::<QuestionSourceManifest>(manifest_value) {
        Ok(v) => v,
        Err(err) => {
            validation
                .errors
                .push(format!("manifest parse failed: {err}"));
            return Ok((validation, None));
        }
    };

    let loaded = if parsed_questions.is_empty() {
        None
    } else {
        Some(LoadedQuestionSource {
            manifest,
            questions: parsed_questions,
        })
    };

    Ok((validation, loaded))
}

pub(crate) fn hydrate_bundle_questions(
    source_root: &Path,
    questions: &[BundleQuestion],
) -> Result<Vec<HydratedQuestion>> {
    let mut hydrated = Vec::with_capacity(questions.len());
    for question in questions {
        let latex_path = join_bundle_path(source_root, &question.latex_path);
        let latex_bytes = fs::read(&latex_path).with_context(|| {
            format!("read question tex failed: {}", latex_path.to_string_lossy())
        })?;
        let latex_source = String::from_utf8_lossy(&latex_bytes).to_string();

        let answer_source = if let Some(path) = &question.answer_latex_path {
            let answer_path = join_bundle_path(source_root, path);
            if answer_path.exists() {
                let bytes = fs::read(&answer_path).with_context(|| {
                    format!("read answer tex failed: {}", answer_path.to_string_lossy())
                })?;
                Some(String::from_utf8_lossy(&bytes).to_string())
            } else {
                None
            }
        } else {
            None
        };

        let comparison_text = normalize_search_text(
            &[
                question.search_text.as_deref(),
                Some(latex_source.as_str()),
                answer_source.as_deref(),
            ],
            1000,
        );

        hydrated.push(HydratedQuestion {
            question: question.clone(),
            latex_source,
            answer_source,
            comparison_text,
        });
    }
    Ok(hydrated)
}

pub(crate) async fn find_similarity_issues(
    pool: &PgPool,
    questions: &[HydratedQuestion],
    allow_similar: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let rows =
        query("SELECT question_id, COALESCE(search_text, '') AS comparison_text FROM questions")
            .fetch_all(pool)
            .await
            .context("query existing questions for similarity check failed")?;

    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for incoming in questions {
        if incoming.comparison_text.is_empty() {
            continue;
        }
        let mut matches = Vec::new();
        for row in &rows {
            let existing_id: String = row.get("question_id");
            let existing_text: String = row.get("comparison_text");
            if existing_text.is_empty() || existing_id == incoming.question.question_id {
                continue;
            }
            let ratio = normalized_levenshtein(&incoming.comparison_text, &existing_text);
            if ratio >= SIMILARITY_THRESHOLD {
                matches.push(format!("{} ({ratio:.3})", existing_id));
            }
        }

        if !matches.is_empty() {
            let msg = format!(
                "{} is highly similar to existing questions: {}",
                incoming.question.question_id,
                matches.join(", ")
            );
            if allow_similar {
                warnings.push(msg);
            } else {
                errors.push(msg);
            }
        }
    }

    Ok((warnings, errors))
}

pub(crate) async fn upsert_object_tx(
    tx: &mut Transaction<'_, Postgres>,
    kind: &str,
    source_path: &Path,
    bytes: &[u8],
    mime_type: Option<&str>,
    created_by: &str,
) -> Result<String> {
    let size = i64::try_from(bytes.len()).context("object bytes exceed i64 range")?;
    let sha = sha256_hex(bytes).to_lowercase();

    if let Some(existing) = query(
        "SELECT object_id::text AS object_id FROM objects WHERE sha256 = $1 AND size_bytes = $2",
    )
    .bind(&sha)
    .bind(size)
    .fetch_optional(&mut **tx)
    .await
    .context("query existing object by hash failed")?
    {
        let object_id: String = existing.get("object_id");
        query("INSERT INTO object_blobs (object_id, content) VALUES ($1::uuid, $2) ON CONFLICT (object_id) DO NOTHING")
            .bind(&object_id)
            .bind(bytes)
            .execute(&mut **tx)
            .await
            .context("ensure existing object blob failed")?;
        return Ok(object_id);
    }

    let object_id = Uuid::new_v4().to_string();
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "blob.bin".to_string());
    let prefix = sha.get(0..4).unwrap_or("0000");
    let object_key = format!("qb/local/{kind}/{prefix}/{file_name}");

    query(
        r#"
        INSERT INTO objects (
            object_id, bucket, object_key, sha256, size_bytes,
            mime_type, storage_class, created_at, created_by, encryption
        ) VALUES (
            $1::uuid, $2, $3, $4, $5,
            $6, 'hot', NOW(), $7, 'sse'
        )
        "#,
    )
    .bind(&object_id)
    .bind(OBJECT_BUCKET)
    .bind(&object_key)
    .bind(&sha)
    .bind(size)
    .bind(mime_type)
    .bind(created_by)
    .execute(&mut **tx)
    .await
    .context("insert object failed")?;

    query("INSERT INTO object_blobs (object_id, content) VALUES ($1::uuid, $2)")
        .bind(&object_id)
        .bind(bytes)
        .execute(&mut **tx)
        .await
        .context("insert object blob failed")?;

    Ok(object_id)
}

pub(crate) async fn insert_import_run(
    pool: &PgPool,
    source_root: &Path,
    source_name: Option<&str>,
    dry_run: bool,
    status: &str,
    item_count: usize,
    warnings: &[String],
    errors: &[String],
    run_label_override: Option<&str>,
) -> Result<()> {
    let run_label = run_label_override
        .map(str::to_string)
        .or_else(|| {
            source_name.map(str::to_string).or_else(|| {
                source_root
                    .file_name()
                    .map(|v| v.to_string_lossy().to_string())
            })
        })
        .unwrap_or_else(|| "question-import".to_string());

    let details = json!({
        "source_name": source_name,
        "warnings": warnings,
        "errors": errors,
    });

    query(
        r#"
        INSERT INTO import_runs (
            run_label, bundle_path, dry_run, status, item_count,
            warning_count, error_count, details_json, started_at, finished_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())
        "#,
    )
    .bind(run_label)
    .bind(source_root.to_string_lossy().to_string())
    .bind(dry_run)
    .bind(status)
    .bind(i32::try_from(item_count).unwrap_or(i32::MAX))
    .bind(i32::try_from(warnings.len()).unwrap_or(i32::MAX))
    .bind(i32::try_from(errors.len()).unwrap_or(i32::MAX))
    .bind(details)
    .execute(pool)
    .await
    .context("insert import_run failed")?;

    Ok(())
}
