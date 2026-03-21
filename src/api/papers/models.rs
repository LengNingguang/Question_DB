use std::collections::HashSet;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

pub(crate) const PAPER_TYPES: [&str; 4] = ["regular", "semifinal", "final", "other"];

#[derive(Debug, Serialize)]
pub struct PaperSummary {
    pub(crate) paper_id: String,
    pub(crate) edition: Option<String>,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) notes: Option<String>,
    pub(crate) question_count: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct PaperQuestionSummary {
    pub(crate) question_id: String,
    pub(crate) sort_order: i32,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PaperDetail {
    pub(crate) paper_id: String,
    pub(crate) edition: Option<String>,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) notes: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) questions: Vec<PaperQuestionSummary>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreatePaperRequest {
    pub(crate) edition: Option<String>,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) notes: Option<String>,
    pub(crate) question_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PaperWriteResponse {
    pub(crate) paper_id: String,
    pub(crate) status: &'static str,
    pub(crate) question_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdatePaperRequest {
    #[serde(default)]
    pub(crate) edition: Option<Option<String>>,
    #[serde(default)]
    pub(crate) paper_type: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) notes: Option<Option<String>>,
    #[serde(default)]
    pub(crate) question_ids: Option<Vec<String>>,
}

#[derive(Debug)]
pub(crate) struct NormalizedPaperUpdate {
    pub(crate) edition: Option<Option<String>>,
    pub(crate) paper_type: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) notes: Option<Option<String>>,
    pub(crate) question_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PaperDeleteResponse {
    pub(crate) paper_id: String,
    pub(crate) status: &'static str,
}

pub(crate) fn validate_paper_type(paper_type: &str) -> Result<()> {
    if !PAPER_TYPES.contains(&paper_type) {
        bail!("paper_type must be one of: regular, semifinal, final, other");
    }
    Ok(())
}

impl UpdatePaperRequest {
    pub(crate) fn normalize(self) -> Result<NormalizedPaperUpdate> {
        if self.edition.is_none()
            && self.paper_type.is_none()
            && self.title.is_none()
            && self.notes.is_none()
            && self.question_ids.is_none()
        {
            return Err(anyhow!(
                "request body must include at least one of: edition, paper_type, title, notes, question_ids"
            ));
        }

        let edition = self
            .edition
            .map(|value| value.and_then(normalize_optional_text));
        let paper_type = self
            .paper_type
            .map(|value| normalize_paper_type(&value))
            .transpose()?;
        let title = self
            .title
            .map(|value| normalize_required_text("title", &value))
            .transpose()?;
        let notes = self
            .notes
            .map(|value| value.and_then(normalize_optional_text));
        let question_ids = self.question_ids.map(normalize_question_ids).transpose()?;

        Ok(NormalizedPaperUpdate {
            edition,
            paper_type,
            title,
            notes,
            question_ids,
        })
    }
}

fn normalize_paper_type(value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    validate_paper_type(&normalized)?;
    Ok(normalized)
}

fn normalize_required_text(field: &str, value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(normalized)
}

fn normalize_optional_text(value: String) -> Option<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_question_ids(question_ids: Vec<String>) -> Result<Vec<String>> {
    if question_ids.is_empty() {
        bail!("question_ids must not be empty");
    }

    let mut seen = HashSet::new();
    for question_id in &question_ids {
        if !seen.insert(question_id.clone()) {
            bail!("duplicate question_id in question_ids: {question_id}");
        }
    }

    Ok(question_ids)
}
