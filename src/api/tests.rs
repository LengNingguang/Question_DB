#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command, time::{SystemTime, UNIX_EPOCH}};

use super::super::{
    imports::inspect_question_source, queries::count_question_binds, utils::normalize_search_text,
};
    use crate::api::models::QuestionsParams;

    #[test]
    fn question_query_normalizes_limit_offset_and_counts_binds() {
        let params = QuestionsParams {
            edition: Some("18".into()),
            paper_id: Some("CPHOS-18-REGULAR".into()),
            paper_type: Some("regular".into()),
            category: Some("theory".into()),
            has_assets: Some(true),
            has_answer: Some(false),
            tag: Some("mechanics".into()),
            q: Some("pendulum".into()),
            limit: Some(999),
            offset: Some(-10),
        };

        let query = params.build_query();
        assert_eq!(query.limit, 100);
        assert_eq!(query.offset, 0);
        assert_eq!(query.bind_count, count_question_binds(&params));
        assert!(query.sql.contains("q.tags_json @>"));
        assert!(query.sql.contains("EXISTS (SELECT 1 FROM question_assets"));
        assert!(query.sql.contains("q.answer_tex_object_id IS NULL"));
        assert!(query.sql.contains("FROM paper_questions pq"));
    }

    #[test]
    fn normalize_search_text_removes_latex_noise() {
        let normalized = normalize_search_text(
            &[Some("\\alpha + x_{1} with  spaces"), Some("line\\beta")],
            1000,
        );
        assert!(!normalized.contains("\\beta"));
        assert!(!normalized.contains("\\alpha"));
        assert!(normalized.contains("with spaces"));
    }

    #[test]
    fn inspect_question_source_accepts_generated_layout() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let out_dir = std::env::temp_dir().join(format!("qb_generated_samples_{unique}"));
        let status = Command::new("bash")
            .arg(root.join("scripts/generate_samples.sh"))
            .arg(&out_dir)
            .status()
            .expect("run sample generator");
        assert!(status.success());

        let (validation, loaded) =
            inspect_question_source(&out_dir).expect("inspect question source");
        assert!(validation.errors.is_empty());
        assert!(loaded.is_some());
        let loaded = loaded.expect("loaded bundle");
        assert_eq!(loaded.manifest.source_name, "generated-cphos-latex");
        assert_eq!(loaded.questions.len(), 4);
    }
}
