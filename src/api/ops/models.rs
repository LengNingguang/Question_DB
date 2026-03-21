use serde::{Deserialize, Serialize};

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
