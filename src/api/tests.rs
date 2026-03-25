#[cfg(test)]
mod tests {
    use crate::api::{
        papers::{models::PapersParams, queries::count_paper_binds},
        questions::{models::QuestionsParams, queries::count_question_binds},
    };

    #[test]
    fn question_query_normalizes_limit_offset_and_counts_binds() {
        let params = QuestionsParams {
            paper_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            paper_type: Some("regular".into()),
            category: Some("none".into()),
            tag: Some("mechanics".into()),
            difficulty_tag: Some("human".into()),
            difficulty_min: Some(3),
            difficulty_max: Some(6),
            q: Some("pendulum".into()),
            limit: Some(999),
            offset: Some(-10),
        };

        let query = params.build_query();
        assert_eq!(query.limit, 100);
        assert_eq!(query.offset, 0);
        assert_eq!(query.bind_count, count_question_binds(&params));
        assert!(query.sql.contains("FROM question_tags qt"));
        assert!(query.sql.contains("FROM question_difficulties qd"));
        assert!(query.sql.contains("qd.algorithm_tag = "));
        assert!(query.sql.contains("qd.score >= "));
        assert!(query.sql.contains("qd.score <= "));
        assert!(query.sql.contains("FROM paper_questions pq"));
        assert!(query.sql.contains("COALESCE(q.description, '') ILIKE"));
        assert!(!query.sql.contains("q.question_id::text ILIKE"));
        assert!(!query.sql.contains("q.source_tex_path, '') ILIKE"));
    }

    #[test]
    fn paper_query_normalizes_limit_offset_and_counts_binds() {
        let params = PapersParams {
            question_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            paper_type: Some("final".into()),
            category: Some("E".into()),
            tag: Some("optics".into()),
            q: Some("thermal".into()),
            limit: Some(999),
            offset: Some(-10),
        };

        let query = params.build_query();
        assert_eq!(query.limit, 100);
        assert_eq!(query.offset, 0);
        assert_eq!(query.bind_count, count_paper_binds(&params));
        assert!(query.sql.contains("FROM paper_questions pq"));
        assert!(query.sql.contains("JOIN question_tags qt"));
        assert!(query.sql.contains("p.description ILIKE"));
    }
}
