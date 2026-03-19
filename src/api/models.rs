//! Request and response models exposed by the HTTP API.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct PaperSummary {
    pub(crate) paper_id: String,
    pub(crate) edition: String,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) notes: Option<String>,
    pub(crate) question_count: i64,
}

#[derive(Debug, Serialize)]
pub struct PaperQuestionSummary {
    pub(crate) question_id: String,
    pub(crate) sort_order: i32,
    pub(crate) question_label: Option<String>,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) tags: Value,
}

#[derive(Debug, Serialize)]
pub struct PaperDetail {
    pub(crate) paper_id: String,
    pub(crate) edition: String,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) notes: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) questions: Vec<PaperQuestionSummary>,
}

#[derive(Debug, Serialize)]
pub struct QuestionSummary {
    pub(crate) question_id: String,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) search_text: Option<String>,
    pub(crate) question_tex_object_id: Option<String>,
    pub(crate) answer_tex_object_id: Option<String>,
    pub(crate) tags: Value,
}

#[derive(Debug, Serialize)]
pub struct QuestionAsset {
    pub(crate) asset_id: String,
    pub(crate) kind: String,
    pub(crate) object_id: String,
    pub(crate) caption: Option<String>,
    pub(crate) sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct QuestionPaperRef {
    pub(crate) paper_id: String,
    pub(crate) edition: String,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) sort_order: i32,
    pub(crate) question_label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QuestionDetail {
    pub(crate) question_id: String,
    pub(crate) category: String,
    pub(crate) question_tex_object_id: Option<String>,
    pub(crate) answer_tex_object_id: Option<String>,
    pub(crate) search_text: Option<String>,
    pub(crate) status: String,
    pub(crate) tags: Value,
    pub(crate) notes: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) assets: Vec<QuestionAsset>,
    pub(crate) papers: Vec<QuestionPaperRef>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QuestionsParams {
    pub(crate) edition: Option<String>,
    pub(crate) paper_id: Option<String>,
    pub(crate) paper_type: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) has_assets: Option<bool>,
    pub(crate) has_answer: Option<bool>,
    pub(crate) tag: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchParams {
    pub(crate) q: String,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QuestionImportRequest {
    pub(crate) source_root: String,
    #[serde(default)]
    pub(crate) allow_similar: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreatePaperRequest {
    pub(crate) paper_id: String,
    pub(crate) edition: String,
    pub(crate) paper_type: String,
    pub(crate) title: String,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReplacePaperQuestionsRequest {
    pub(crate) question_refs: Vec<PaperQuestionRefInput>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PaperQuestionRefInput {
    pub(crate) question_id: String,
    pub(crate) sort_order: i32,
    pub(crate) question_label: Option<String>,
}

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
pub(crate) struct QuestionImportValidationResponse {
    pub(crate) source_root: String,
    pub(crate) ok: bool,
    pub(crate) warnings: Vec<String>,
    pub(crate) errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct QuestionImportCommitResponse {
    pub(crate) source_root: String,
    pub(crate) status: String,
    pub(crate) question_count: usize,
    pub(crate) imported_questions: usize,
    pub(crate) imported_assets: usize,
    pub(crate) warnings: Vec<String>,
    pub(crate) errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PaperWriteResponse {
    pub(crate) paper_id: String,
    pub(crate) status: &'static str,
    pub(crate) question_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExportResponse {
    pub(crate) format: &'static str,
    pub(crate) public: bool,
    pub(crate) output_path: String,
    pub(crate) exported_questions: usize,
}
