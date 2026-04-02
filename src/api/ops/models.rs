use std::collections::HashSet;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub(crate) struct ExportRequest {
    pub(crate) format: ExportFormat,
    #[serde(default)]
    pub(crate) public: bool,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ExportFormat {
    Jsonl,
    Csv,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QualityCheckRequest {
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExportResponse {
    pub(crate) format: &'static str,
    pub(crate) public: bool,
    pub(crate) output_path: String,
    pub(crate) exported_questions: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QuestionBundleRequest {
    pub(crate) question_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PaperBundleRequest {
    pub(crate) paper_ids: Vec<String>,
}

impl QuestionBundleRequest {
    pub(crate) fn normalize(self) -> Result<Vec<String>> {
        normalize_ids("question_ids", self.question_ids)
    }
}

impl PaperBundleRequest {
    pub(crate) fn normalize(self) -> Result<Vec<String>> {
        normalize_ids("paper_ids", self.paper_ids)
    }
}

fn normalize_ids(field_name: &str, ids: Vec<String>) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Err(anyhow!("{field_name} must not be empty"));
    }

    let mut normalized = Vec::with_capacity(ids.len());
    let mut seen = HashSet::new();

    for raw_id in ids {
        let id = raw_id.trim().to_string();
        if id.is_empty() {
            bail!("{field_name} must not contain empty values");
        }
        Uuid::parse_str(&id).map_err(|_| anyhow!("invalid {field_name} entry: {id}"))?;
        if !seen.insert(id.clone()) {
            bail!("duplicate {field_name} entry: {id}");
        }
        normalized.push(id);
    }

    Ok(normalized)
}
