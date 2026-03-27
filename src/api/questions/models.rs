use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::api::shared::utils::normalize_optional_bundle_description;

pub(crate) const QUESTION_CATEGORIES: [&str; 3] = ["none", "T", "E"];
pub(crate) const QUESTION_STATUSES: [&str; 3] = ["none", "reviewed", "used"];

#[derive(Debug, Serialize)]
pub struct QuestionSourceRef {
    pub(crate) tex: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuestionDifficulty {
    #[serde(flatten)]
    pub(crate) entries: BTreeMap<String, QuestionDifficultyValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionDifficultyValue {
    pub(crate) score: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAssetRef {
    pub(crate) path: String,
    pub(crate) file_kind: String,
    pub(crate) object_id: String,
    pub(crate) mime_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QuestionSummary {
    pub(crate) question_id: String,
    pub(crate) source: QuestionSourceRef,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) description: String,
    pub(crate) tags: Vec<String>,
    pub(crate) difficulty: QuestionDifficulty,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct QuestionPaperRef {
    pub(crate) paper_id: String,
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct QuestionDetail {
    pub(crate) question_id: String,
    pub(crate) tex_object_id: String,
    pub(crate) source: QuestionSourceRef,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) description: String,
    pub(crate) tags: Vec<String>,
    pub(crate) difficulty: QuestionDifficulty,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) assets: Vec<QuestionAssetRef>,
    pub(crate) papers: Vec<QuestionPaperRef>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QuestionsParams {
    pub(crate) paper_id: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) difficulty_tag: Option<String>,
    pub(crate) difficulty_min: Option<i32>,
    pub(crate) difficulty_max: Option<i32>,
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateQuestionMetadataRequest {
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<Option<String>>,
    #[serde(default)]
    pub(crate) tags: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) difficulty: Option<QuestionDifficulty>,
}

#[derive(Debug)]
pub(crate) struct NormalizedQuestionMetadataUpdate {
    pub(crate) category: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) status: Option<String>,
    pub(crate) difficulty: Option<NormalizedQuestionDifficulty>,
}

#[derive(Debug, Clone)]
pub(crate) struct NormalizedQuestionDifficultyValue {
    pub(crate) score: i32,
    pub(crate) notes: Option<String>,
}

pub(crate) type NormalizedQuestionDifficulty = BTreeMap<String, NormalizedQuestionDifficultyValue>;

#[derive(Debug, Serialize)]
pub(crate) struct QuestionImportResponse {
    pub(crate) question_id: String,
    pub(crate) file_name: String,
    pub(crate) imported_assets: usize,
    pub(crate) status: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct QuestionFileReplaceResponse {
    pub(crate) question_id: String,
    pub(crate) file_name: String,
    pub(crate) source_tex_path: String,
    pub(crate) imported_assets: usize,
    pub(crate) status: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct QuestionDeleteResponse {
    pub(crate) question_id: String,
    pub(crate) status: &'static str,
}

pub(crate) fn validate_question_category(category: &str) -> Result<()> {
    if !QUESTION_CATEGORIES.contains(&category) {
        bail!("category must be one of: none, T, E");
    }
    Ok(())
}

pub(crate) fn validate_question_status(status: &str) -> Result<()> {
    if !QUESTION_STATUSES.contains(&status) {
        bail!("status must be one of: none, reviewed, used");
    }
    Ok(())
}

impl UpdateQuestionMetadataRequest {
    pub(crate) fn normalize(self) -> Result<NormalizedQuestionMetadataUpdate> {
        if self.category.is_none()
            && self.description.is_none()
            && self.tags.is_none()
            && self.status.is_none()
            && self.difficulty.is_none()
        {
            return Err(anyhow!(
                "request body must include at least one of: category, description, tags, status, difficulty"
            ));
        }

        let category = self
            .category
            .map(|value| normalize_category(&value))
            .transpose()?;
        let description = self
            .description
            .map(|value| normalize_required_plaintext("description", value))
            .transpose()?;
        let tags = self.tags.map(normalize_tags).transpose()?;
        let status = self
            .status
            .map(|value| normalize_status(&value))
            .transpose()?;
        let difficulty = self
            .difficulty
            .map(QuestionDifficulty::normalize)
            .transpose()?;

        Ok(NormalizedQuestionMetadataUpdate {
            category,
            description,
            tags,
            status,
            difficulty,
        })
    }
}

impl QuestionDifficulty {
    pub(crate) fn normalize(self) -> Result<NormalizedQuestionDifficulty> {
        normalize_difficulty_entries(self.entries)
    }
}

fn normalize_category(value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    validate_question_category(&normalized)?;
    Ok(normalized)
}

fn normalize_status(value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    validate_question_status(&normalized)?;
    Ok(normalized)
}

fn normalize_required_plaintext(field: &str, value: Option<String>) -> Result<String> {
    normalize_optional_bundle_description(field, value)
}

fn normalize_optional_plaintext(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn normalize_tags(values: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();

    for value in values {
        let tag = value.trim().to_string();
        if tag.is_empty() {
            bail!("tags must not contain empty strings");
        }
        if seen.insert(tag.clone()) {
            normalized.push(tag);
        }
    }

    Ok(normalized)
}

fn normalize_difficulty_entries(
    values: BTreeMap<String, QuestionDifficultyValue>,
) -> Result<NormalizedQuestionDifficulty> {
    let mut normalized = BTreeMap::new();

    for (name, value) in values {
        let tag = name.trim().to_string();
        if tag.is_empty() {
            bail!("difficulty keys must not be empty");
        }
        if !(1..=10).contains(&value.score) {
            bail!("difficulty.{tag}.score must be between 1 and 10");
        }
        let notes = value.notes.and_then(normalize_optional_plaintext);
        if normalized
            .insert(
                tag.clone(),
                NormalizedQuestionDifficultyValue {
                    score: value.score,
                    notes,
                },
            )
            .is_some()
        {
            bail!("difficulty tags must be unique after trimming");
        }
    }

    if !normalized.contains_key("human") {
        bail!("difficulty must include a human entry");
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::UpdateQuestionMetadataRequest;

    #[test]
    fn update_request_normalizes_and_deduplicates_tags() {
        let request = UpdateQuestionMetadataRequest {
            category: Some(" T ".into()),
            description: Some(Some("  demo note  ".into())),
            tags: Some(vec![" optics ".into(), "mechanics".into(), "optics".into()]),
            status: Some(" reviewed ".into()),
            difficulty: None,
        };

        let normalized = request.normalize().expect("request should normalize");
        assert_eq!(normalized.category.as_deref(), Some("T"));
        assert_eq!(normalized.description.as_deref(), Some("demo note"));
        assert_eq!(
            normalized.tags.expect("tags should be present"),
            vec!["optics".to_string(), "mechanics".to_string()]
        );
        assert_eq!(normalized.status.as_deref(), Some("reviewed"));
    }

    #[test]
    fn update_request_requires_human_difficulty() {
        let request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"difficulty":{"ml":{"score":8}}}"#)
                .expect("json should parse");

        assert!(request.normalize().is_err());
    }

    #[test]
    fn update_request_normalizes_difficulty_notes() {
        let request: UpdateQuestionMetadataRequest = serde_json::from_str(
            r#"{
                "difficulty":{
                    " human ":{"score":7,"notes":"  calibrated  "},
                    "heuristic":{"score":5,"notes":"   "}
                }
            }"#,
        )
        .expect("json should parse");

        let normalized = request.normalize().expect("request should normalize");
        let difficulty = normalized.difficulty.expect("difficulty update");
        assert_eq!(difficulty["human"].score, 7);
        assert_eq!(difficulty["human"].notes.as_deref(), Some("calibrated"));
        assert_eq!(difficulty["heuristic"].score, 5);
        assert_eq!(difficulty["heuristic"].notes, None);
    }

    #[test]
    fn update_request_rejects_empty_or_null_description() {
        let empty_request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"description":""}"#).expect("json should parse");
        let null_request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"description":null}"#).expect("json should parse");

        assert!(empty_request.normalize().is_err());
        assert!(null_request.normalize().is_err());
    }
}
