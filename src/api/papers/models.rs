use std::collections::HashSet;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::api::questions::models::validate_question_category;
use crate::api::shared::utils::{
    normalize_bundle_description, normalize_optional_bundle_description,
};

#[derive(Debug, Serialize)]
pub struct PaperSummary {
    pub(crate) paper_id: String,
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
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
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) questions: Vec<PaperQuestionSummary>,
}

#[derive(Debug)]
pub(crate) struct CreatePaperRequest {
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) question_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PapersParams {
    pub(crate) question_id: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PaperImportResponse {
    pub(crate) paper_id: String,
    pub(crate) file_name: String,
    pub(crate) status: &'static str,
    pub(crate) question_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct PaperFileReplaceResponse {
    pub(crate) paper_id: String,
    pub(crate) file_name: String,
    pub(crate) status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdatePaperRequest {
    #[serde(default)]
    pub(crate) description: Option<Option<String>>,
    #[serde(default)]
    pub(crate) title: Option<Option<String>>,
    #[serde(default)]
    pub(crate) subtitle: Option<Option<String>>,
    #[serde(default)]
    pub(crate) authors: Option<Option<Vec<String>>>,
    #[serde(default)]
    pub(crate) reviewers: Option<Option<Vec<String>>>,
    #[serde(default)]
    pub(crate) question_ids: Option<Option<Vec<String>>>,
}

#[derive(Debug)]
pub(crate) struct NormalizedCreatePaperRequest {
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) question_ids: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct NormalizedPaperUpdate {
    pub(crate) description: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) subtitle: Option<String>,
    pub(crate) authors: Option<Vec<String>>,
    pub(crate) reviewers: Option<Vec<String>>,
    pub(crate) question_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PaperDeleteResponse {
    pub(crate) paper_id: String,
    pub(crate) status: &'static str,
}

impl CreatePaperRequest {
    pub(crate) fn normalize(self) -> Result<NormalizedCreatePaperRequest> {
        let description = normalize_required_description("description", &self.description)?;
        let title = normalize_required_metadata_text("title", &self.title)?;
        let subtitle = normalize_required_metadata_text("subtitle", &self.subtitle)?;
        let authors = normalize_text_list("authors", self.authors)?;
        let reviewers = normalize_text_list("reviewers", self.reviewers)?;
        let question_ids = normalize_question_ids(self.question_ids)?;

        Ok(NormalizedCreatePaperRequest {
            description,
            title,
            subtitle,
            authors,
            reviewers,
            question_ids,
        })
    }
}

impl UpdatePaperRequest {
    pub(crate) fn normalize(self) -> Result<NormalizedPaperUpdate> {
        if self.description.is_none()
            && self.title.is_none()
            && self.subtitle.is_none()
            && self.authors.is_none()
            && self.reviewers.is_none()
            && self.question_ids.is_none()
        {
            return Err(anyhow!(
                "request body must include at least one of: description, title, subtitle, authors, reviewers, question_ids"
            ));
        }

        let description = self
            .description
            .map(|value| normalize_required_plaintext("description", value))
            .transpose()?;
        let title = self
            .title
            .map(|value| normalize_required_metadata_option("title", value))
            .transpose()?;
        let subtitle = self
            .subtitle
            .map(|value| normalize_required_metadata_option("subtitle", value))
            .transpose()?;
        let authors = self
            .authors
            .map(|value| normalize_required_text_list("authors", value))
            .transpose()?;
        let reviewers = self
            .reviewers
            .map(|value| normalize_required_text_list("reviewers", value))
            .transpose()?;
        let question_ids = self
            .question_ids
            .map(|value| normalize_required_question_ids("question_ids", value))
            .transpose()?;

        Ok(NormalizedPaperUpdate {
            description,
            title,
            subtitle,
            authors,
            reviewers,
            question_ids,
        })
    }
}

impl PapersParams {
    pub(crate) fn normalized_limit(&self) -> i64 {
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    pub(crate) fn normalized_offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

fn normalize_required_description(field: &str, value: &str) -> Result<String> {
    normalize_bundle_description(field, value)
}

fn normalize_required_plaintext(field: &str, value: Option<String>) -> Result<String> {
    normalize_optional_bundle_description(field, value)
}

fn normalize_required_metadata_option(field: &str, value: Option<String>) -> Result<String> {
    let Some(text) = value else {
        bail!("{field} must not be null");
    };
    normalize_required_metadata_text(field, &text)
}

fn normalize_required_metadata_text(field: &str, value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        bail!("{field} must not be empty");
    }
    if normalized.chars().any(char::is_control) {
        bail!("{field} must not contain control characters");
    }
    Ok(normalized)
}

fn normalize_required_text_list(field: &str, value: Option<Vec<String>>) -> Result<Vec<String>> {
    let Some(items) = value else {
        bail!("{field} must not be null");
    };
    normalize_text_list(field, items)
}

fn normalize_text_list(field: &str, values: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();

    for value in values {
        let item = normalize_required_metadata_text(field, &value)?;
        if seen.insert(item.clone()) {
            normalized.push(item);
        }
    }

    Ok(normalized)
}

fn normalize_required_question_ids(field: &str, value: Option<Vec<String>>) -> Result<Vec<String>> {
    let Some(items) = value else {
        bail!("{field} must not be null");
    };
    normalize_question_ids(items)
}

fn normalize_question_ids(question_ids: Vec<String>) -> Result<Vec<String>> {
    if question_ids.is_empty() {
        bail!("question_ids must not be empty");
    }

    let mut normalized = Vec::with_capacity(question_ids.len());
    let mut seen = HashSet::new();
    for question_id in question_ids {
        let candidate = question_id.trim().to_string();
        if candidate.is_empty() {
            bail!("question_ids must not contain empty strings");
        }
        if !seen.insert(candidate.clone()) {
            bail!("duplicate question_id in question_ids: {candidate}");
        }
        normalized.push(candidate);
    }

    Ok(normalized)
}

pub(crate) fn validate_paper_filters(params: &PapersParams) -> Result<()> {
    if let Some(question_id) = &params.question_id {
        uuid::Uuid::parse_str(question_id)
            .map_err(|_| anyhow!("question_id must be a valid UUID"))?;
    }
    if let Some(category) = &params.category {
        validate_question_category(category)
            .map_err(|_| anyhow!("category must be one of: none, T, E"))?;
    }
    if let Some(q) = &params.q {
        if q.trim().is_empty() {
            bail!("q must not be empty");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CreatePaperRequest, UpdatePaperRequest};

    #[test]
    fn create_request_normalizes_metadata_and_lists() {
        let request = CreatePaperRequest {
            description: "  综合训练试卷  ".into(),
            title: "  2026 校内选拔  ".into(),
            subtitle: "  第一场  ".into(),
            authors: vec![" Alice ".into(), "Bob".into(), "Alice".into()],
            reviewers: vec![" Carol ".into()],
            question_ids: vec![" q1 ".into()],
        };

        let normalized = request.normalize().expect("request should normalize");
        assert_eq!(normalized.description, "综合训练试卷");
        assert_eq!(normalized.title, "2026 校内选拔");
        assert_eq!(normalized.subtitle, "第一场");
        assert_eq!(
            normalized.authors,
            vec!["Alice".to_string(), "Bob".to_string()]
        );
        assert_eq!(normalized.reviewers, vec!["Carol".to_string()]);
        assert_eq!(normalized.question_ids, vec!["q1".to_string()]);
    }

    #[test]
    fn create_request_requires_non_empty_question_ids() {
        let request = CreatePaperRequest {
            description: "demo".into(),
            title: "title".into(),
            subtitle: "subtitle".into(),
            authors: vec![],
            reviewers: vec![],
            question_ids: vec![],
        };

        assert!(request.normalize().is_err());
    }

    #[test]
    fn update_request_rejects_empty_or_null_fields() {
        let empty_request: UpdatePaperRequest =
            serde_json::from_str(r#"{"title":""}"#).expect("json should parse");
        let null_request: UpdatePaperRequest =
            serde_json::from_str(r#"{"question_ids":null}"#).expect("json should parse");

        assert!(empty_request.normalize().is_err());
        assert!(null_request.normalize().is_err());
    }

    #[test]
    fn update_request_normalizes_lists() {
        let request: UpdatePaperRequest = serde_json::from_str(
            r#"{
                "description":"  综合训练重排卷  ",
                "authors":[" Alice ","Bob","Alice"],
                "reviewers":[" Carol "],
                "question_ids":[" q1 ","q2 "]
            }"#,
        )
        .expect("json should parse");

        let normalized = request.normalize().expect("request should normalize");
        assert_eq!(normalized.description.as_deref(), Some("综合训练重排卷"));
        assert_eq!(
            normalized.authors.expect("authors should be present"),
            vec!["Alice".to_string(), "Bob".to_string()]
        );
        assert_eq!(
            normalized.reviewers.expect("reviewers should be present"),
            vec!["Carol".to_string()]
        );
        assert_eq!(
            normalized
                .question_ids
                .expect("question_ids should be present"),
            vec!["q1".to_string(), "q2".to_string()]
        );
    }
}
