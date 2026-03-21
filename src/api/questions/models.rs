use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

pub(crate) const QUESTION_CATEGORIES: [&str; 3] = ["none", "T", "E"];
pub(crate) const QUESTION_STATUSES: [&str; 3] = ["none", "reviewed", "used"];

#[derive(Debug, Serialize)]
pub struct QuestionSourceRef {
    pub(crate) tex: String,
}

#[derive(Debug, Default, Serialize)]
pub struct QuestionDifficulty {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) human: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) algorithm: Vec<QuestionAlgorithmScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QuestionAlgorithmScore {
    pub(crate) tag: String,
    pub(crate) score: i32,
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub(crate) notes: String,
    pub(crate) tags: Vec<String>,
    pub(crate) difficulty: QuestionDifficulty,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct QuestionPaperRef {
    pub(crate) paper_id: String,
    pub(crate) edition: Option<String>,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct QuestionDetail {
    pub(crate) question_id: String,
    pub(crate) tex_object_id: String,
    pub(crate) source: QuestionSourceRef,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) notes: String,
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
    pub(crate) paper_type: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) tag: Option<String>,
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
    pub(crate) notes: Option<Option<String>>,
    #[serde(default)]
    pub(crate) tags: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) difficulty: Option<QuestionDifficultyPatch>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionDifficultyPatch {
    #[serde(default)]
    pub(crate) human: Option<Option<i32>>,
    #[serde(default)]
    pub(crate) algorithm: Option<Option<BTreeMap<String, i32>>>,
    #[serde(default)]
    pub(crate) notes: Option<Option<String>>,
}

#[derive(Debug)]
pub(crate) struct NormalizedQuestionMetadataUpdate {
    pub(crate) category: Option<String>,
    pub(crate) notes: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) status: Option<String>,
    pub(crate) difficulty: Option<NormalizedQuestionDifficultyUpdate>,
}

#[derive(Debug)]
pub(crate) struct NormalizedQuestionDifficultyUpdate {
    pub(crate) clear_all: bool,
    pub(crate) human: Option<Option<i32>>,
    pub(crate) algorithm: Option<BTreeMap<String, i32>>,
    pub(crate) notes: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct QuestionImportResponse {
    pub(crate) question_id: String,
    pub(crate) file_name: String,
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
            && self.notes.is_none()
            && self.tags.is_none()
            && self.status.is_none()
            && self.difficulty.is_none()
        {
            return Err(anyhow!(
                "request body must include at least one of: category, notes, tags, status, difficulty"
            ));
        }

        let category = self
            .category
            .map(|value| normalize_category(&value))
            .transpose()?;
        let notes = self.notes.map(normalize_plaintext_to_string);
        let tags = self.tags.map(normalize_tags).transpose()?;
        let status = self
            .status
            .map(|value| normalize_status(&value))
            .transpose()?;
        let difficulty = self
            .difficulty
            .map(QuestionDifficultyPatch::normalize)
            .transpose()?;

        Ok(NormalizedQuestionMetadataUpdate {
            category,
            notes,
            tags,
            status,
            difficulty,
        })
    }
}

impl QuestionDifficultyPatch {
    fn normalize(self) -> Result<NormalizedQuestionDifficultyUpdate> {
        let clear_all = self.human.is_none() && self.algorithm.is_none() && self.notes.is_none();

        let human = self
            .human
            .map(|value| {
                value
                    .map(|score| {
                        if !(1..=10).contains(&score) {
                            bail!("difficulty.human must be an integer between 1 and 10");
                        }
                        Ok(score)
                    })
                    .transpose()
            })
            .transpose()?;

        let algorithm = self
            .algorithm
            .map(|value| {
                value
                    .map(normalize_algorithm_scores)
                    .transpose()
                    .map(|scores| scores.unwrap_or_default())
            })
            .transpose()?;

        let notes = self
            .notes
            .map(|value| value.and_then(normalize_optional_plaintext));

        Ok(NormalizedQuestionDifficultyUpdate {
            clear_all,
            human,
            algorithm,
            notes,
        })
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

fn normalize_plaintext_to_string(value: Option<String>) -> String {
    value
        .and_then(|text| {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or_default()
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

fn normalize_algorithm_scores(values: BTreeMap<String, i32>) -> Result<BTreeMap<String, i32>> {
    let mut normalized = BTreeMap::new();

    for (name, score) in values {
        let tag = name.trim().to_string();
        if tag.is_empty() {
            bail!("difficulty.algorithm keys must not be empty");
        }
        if !(1..=10).contains(&score) {
            bail!("difficulty.algorithm.{tag} must be between 1 and 10");
        }
        normalized.insert(tag, score);
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
            notes: Some(Some("  demo note  ".into())),
            tags: Some(vec![" optics ".into(), "mechanics".into(), "optics".into()]),
            status: Some(" reviewed ".into()),
            difficulty: None,
        };

        let normalized = request.normalize().expect("request should normalize");
        assert_eq!(normalized.category.as_deref(), Some("T"));
        assert_eq!(normalized.notes.as_deref(), Some("demo note"));
        assert_eq!(
            normalized.tags.expect("tags should be present"),
            vec!["optics".to_string(), "mechanics".to_string()]
        );
        assert_eq!(normalized.status.as_deref(), Some("reviewed"));
    }

    #[test]
    fn update_request_allows_clearing_difficulty_with_empty_object() {
        let request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"difficulty":{}}"#).expect("json should parse");

        let normalized = request.normalize().expect("request should normalize");
        assert!(normalized.difficulty.expect("difficulty update").clear_all);
    }
}
