#[cfg(test)]
mod tests {
    use crate::api::questions::{models::QuestionsParams, queries::count_question_binds};

    #[test]
    fn question_query_normalizes_limit_offset_and_counts_binds() {
        let params = QuestionsParams {
            paper_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            paper_type: Some("regular".into()),
            category: Some("none".into()),
            tag: Some("mechanics".into()),
            q: Some("pendulum".into()),
            limit: Some(999),
            offset: Some(-10),
        };

        let query = params.build_query();
        assert_eq!(query.limit, 100);
        assert_eq!(query.offset, 0);
        assert_eq!(query.bind_count, count_question_binds(&params));
        assert!(query.sql.contains("FROM question_tags qt"));
        assert!(query.sql.contains("FROM paper_questions pq"));
        assert!(query.sql.contains("COALESCE(q.notes, '') ILIKE"));
    }
}
