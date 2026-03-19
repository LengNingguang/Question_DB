//! Query planning and row-to-response mapping for read endpoints.

use anyhow::{anyhow, Result};
use serde_json::Value;
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder, Row};

use super::models::{
    PaperQuestionSummary, PaperSummary, QuestionAsset, QuestionPaperRef, QuestionSummary,
    QuestionsParams,
};

#[derive(Debug)]
pub(crate) struct QuestionsQuery {
    pub(crate) sql: String,
    pub(crate) bind_count: usize,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

impl QuestionsParams {
    pub(crate) fn normalized_limit(&self) -> i64 {
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    pub(crate) fn normalized_offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }

    pub(crate) fn build_query(&self) -> QuestionsQuery {
        let mut builder = QueryBuilder::<Postgres>::new(
            "
            SELECT q.question_id, q.category, q.status, q.search_text,
                   q.question_tex_object_id::text AS question_tex_object_id,
                   q.answer_tex_object_id::text AS answer_tex_object_id, q.tags_json
            FROM questions q
            WHERE 1 = 1",
        );
        let mut bind_count = 0;

        if let Some(category) = &self.category {
            builder.push(" AND q.category = ").push_bind(category);
            bind_count += 1;
        }
        if let Some(has_assets) = self.has_assets {
            if has_assets {
                builder.push(
                    " AND EXISTS (SELECT 1 FROM question_assets qa WHERE qa.question_id = q.question_id)",
                );
            } else {
                builder.push(
                    " AND NOT EXISTS (SELECT 1 FROM question_assets qa WHERE qa.question_id = q.question_id)",
                );
            }
        }
        if let Some(has_answer) = self.has_answer {
            if has_answer {
                builder.push(" AND q.answer_tex_object_id IS NOT NULL");
            } else {
                builder.push(" AND q.answer_tex_object_id IS NULL");
            }
        }
        if let Some(tag) = &self.tag {
            builder
                .push(" AND q.tags_json @> ")
                .push_bind(serde_json::json!([tag]));
            bind_count += 1;
        }
        if let Some(paper_id) = &self.paper_id {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.question_id = q.question_id AND pq.paper_id = ")
                .push_bind(paper_id)
                .push(')');
            bind_count += 1;
        }
        if let Some(edition) = &self.edition {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq JOIN papers p ON p.paper_id = pq.paper_id WHERE pq.question_id = q.question_id AND p.edition = ")
                .push_bind(edition)
                .push(')');
            bind_count += 1;
        }
        if let Some(paper_type) = &self.paper_type {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq JOIN papers p ON p.paper_id = pq.paper_id WHERE pq.question_id = q.question_id AND p.paper_type = ")
                .push_bind(paper_type)
                .push(')');
            bind_count += 1;
        }
        if let Some(search) = &self.q {
            let needle = format!("%{search}%");
            builder
                .push(" AND (q.question_id ILIKE ")
                .push_bind(needle.clone())
                .push(" OR COALESCE(q.search_text, '') ILIKE ")
                .push_bind(needle)
                .push(')');
            bind_count += 2;
        }

        let limit = self.normalized_limit();
        let offset = self.normalized_offset();
        builder
            .push(" ORDER BY q.question_id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        QuestionsQuery {
            sql: builder.sql().to_owned(),
            bind_count: bind_count + 2,
            limit,
            offset,
        }
    }
}

pub(crate) fn validate_question_filters(params: &QuestionsParams) -> Result<()> {
    if let Some(paper_type) = &params.paper_type {
        let valid = ["regular", "semifinal", "final", "other"];
        if !valid.contains(&paper_type.as_str()) {
            return Err(anyhow!(
                "paper_type must be one of: regular, semifinal, final, other"
            ));
        }
    }
    if let Some(category) = &params.category {
        let valid = ["theory", "experiment"];
        if !valid.contains(&category.as_str()) {
            return Err(anyhow!("category must be one of: theory, experiment"));
        }
    }
    Ok(())
}

pub(crate) async fn execute_questions_query(
    pool: &PgPool,
    params: &QuestionsParams,
    plan: &QuestionsQuery,
) -> Result<Vec<PgRow>, sqlx::Error> {
    let mut query = query(&plan.sql);
    if let Some(category) = &params.category {
        query = query.bind(category);
    }
    if let Some(tag) = &params.tag {
        query = query.bind(serde_json::json!([tag]));
    }
    if let Some(paper_id) = &params.paper_id {
        query = query.bind(paper_id);
    }
    if let Some(edition) = &params.edition {
        query = query.bind(edition);
    }
    if let Some(paper_type) = &params.paper_type {
        query = query.bind(paper_type);
    }
    if let Some(search) = &params.q {
        let needle = format!("%{search}%");
        query = query.bind(needle.clone()).bind(needle);
    }
    debug_assert_eq!(plan.bind_count, count_question_binds(params));
    query
        .bind(plan.limit)
        .bind(plan.offset)
        .fetch_all(pool)
        .await
}

pub(crate) fn count_question_binds(params: &QuestionsParams) -> usize {
    usize::from(params.category.is_some())
        + usize::from(params.tag.is_some())
        + usize::from(params.paper_id.is_some())
        + usize::from(params.edition.is_some())
        + usize::from(params.paper_type.is_some())
        + params.q.as_ref().map(|_| 2).unwrap_or(0)
        + 2
}

pub(crate) fn map_paper_summary(row: PgRow) -> PaperSummary {
    PaperSummary {
        paper_id: row.get("paper_id"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
        notes: row.get("notes"),
        question_count: row.get("question_count"),
    }
}

pub(crate) fn map_paper_question_summary(row: PgRow) -> PaperQuestionSummary {
    PaperQuestionSummary {
        question_id: row.get("question_id"),
        sort_order: row.get("sort_order"),
        question_label: row.get("question_label"),
        category: row.get("category"),
        status: row.get("status"),
        tags: row.get::<Value, _>("tags_json"),
    }
}

pub(crate) fn map_question_summary(row: PgRow) -> QuestionSummary {
    QuestionSummary {
        question_id: row.get("question_id"),
        category: row.get("category"),
        status: row.get("status"),
        search_text: row.get("search_text"),
        question_tex_object_id: row.get("question_tex_object_id"),
        answer_tex_object_id: row.get("answer_tex_object_id"),
        tags: row.get::<Value, _>("tags_json"),
    }
}

pub(crate) fn map_question_asset(row: PgRow) -> QuestionAsset {
    QuestionAsset {
        asset_id: row.get("asset_id"),
        kind: row.get("kind"),
        object_id: row.get("object_id"),
        caption: row.get("caption"),
        sort_order: row.get("sort_order"),
    }
}

pub(crate) fn map_question_paper_ref(row: PgRow) -> QuestionPaperRef {
    QuestionPaperRef {
        paper_id: row.get("paper_id"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
        sort_order: row.get("sort_order"),
        question_label: row.get("question_label"),
    }
}
