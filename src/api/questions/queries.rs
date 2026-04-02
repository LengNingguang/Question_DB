//! Query planning and row-to-response mapping for question endpoints.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder, Row};

use super::models::{
    validate_question_category, QuestionAssetRef, QuestionDetail, QuestionDifficulty,
    QuestionDifficultyValue, QuestionPaperRef, QuestionSourceRef, QuestionSummary, QuestionsParams,
};
use crate::api::papers::models::{PaperDetail, PaperQuestionSummary, PaperSummary};

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
            SELECT q.question_id::text AS question_id,
                   q.source_tex_path,
                   q.category,
                   q.status,
                   COALESCE(q.description, '') AS description,
                   q.score,
                   to_char(q.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                   to_char(q.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
            FROM questions q
            WHERE 1 = 1",
        );
        let mut bind_count = 0;

        if let Some(category) = &self.category {
            builder.push(" AND q.category = ").push_bind(category);
            bind_count += 1;
        }
        if let Some(tag) = &self.tag {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_tags qt WHERE qt.question_id = q.question_id AND qt.tag = ")
                .push_bind(tag)
                .push(")");
            bind_count += 1;
        }
        if let Some(difficulty_tag) = &self.difficulty_tag {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_difficulties qd WHERE qd.question_id = q.question_id AND qd.algorithm_tag = ")
                .push_bind(difficulty_tag);
            bind_count += 1;
            if let Some(difficulty_min) = self.difficulty_min {
                builder.push(" AND qd.score >= ").push_bind(difficulty_min);
                bind_count += 1;
            }
            if let Some(difficulty_max) = self.difficulty_max {
                builder.push(" AND qd.score <= ").push_bind(difficulty_max);
                bind_count += 1;
            }
            builder.push(")");
        }
        if let Some(paper_id) = &self.paper_id {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.question_id = q.question_id AND pq.paper_id = ")
                .push_bind(paper_id)
                .push("::uuid)");
            bind_count += 1;
        }
        if let Some(search) = &self.q {
            let needle = format!("%{search}%");
            builder
                .push(" AND COALESCE(q.description, '') ILIKE ")
                .push_bind(needle);
            bind_count += 1;
        }
        if let Some(score_min) = self.score_min {
            builder.push(" AND q.score >= ").push_bind(score_min);
            bind_count += 1;
        }
        if let Some(score_max) = self.score_max {
            builder.push(" AND q.score <= ").push_bind(score_max);
            bind_count += 1;
        }

        let limit = self.normalized_limit();
        let offset = self.normalized_offset();
        builder
            .push(" ORDER BY q.created_at DESC, q.question_id LIMIT ")
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
    if let Some(category) = &params.category {
        validate_question_category(category)
            .map_err(|_| anyhow!("category must be one of: none, T, E"))?;
    }
    if let Some(difficulty_tag) = &params.difficulty_tag {
        if difficulty_tag.trim().is_empty() {
            return Err(anyhow!("difficulty_tag must not be empty"));
        }
    }
    if (params.difficulty_min.is_some() || params.difficulty_max.is_some())
        && params.difficulty_tag.is_none()
    {
        return Err(anyhow!(
            "difficulty_tag is required when difficulty_min or difficulty_max is provided"
        ));
    }
    if let Some(difficulty_min) = params.difficulty_min {
        if !(1..=10).contains(&difficulty_min) {
            return Err(anyhow!("difficulty_min must be between 1 and 10"));
        }
    }
    if let Some(difficulty_max) = params.difficulty_max {
        if !(1..=10).contains(&difficulty_max) {
            return Err(anyhow!("difficulty_max must be between 1 and 10"));
        }
    }
    if let (Some(difficulty_min), Some(difficulty_max)) =
        (params.difficulty_min, params.difficulty_max)
    {
        if difficulty_min > difficulty_max {
            return Err(anyhow!(
                "difficulty_min must be less than or equal to difficulty_max"
            ));
        }
    }
    if let Some(q) = &params.q {
        if q.trim().is_empty() {
            return Err(anyhow!("q must not be empty"));
        }
    }
    if let Some(score_min) = params.score_min {
        if !(0..=400).contains(&score_min) {
            return Err(anyhow!("score_min must be between 0 and 400"));
        }
    }
    if let Some(score_max) = params.score_max {
        if !(0..=400).contains(&score_max) {
            return Err(anyhow!("score_max must be between 0 and 400"));
        }
    }
    if let (Some(score_min), Some(score_max)) = (params.score_min, params.score_max) {
        if score_min > score_max {
            return Err(anyhow!("score_min must be less than or equal to score_max"));
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
        query = query.bind(tag);
    }
    if let Some(difficulty_tag) = &params.difficulty_tag {
        query = query.bind(difficulty_tag);
    }
    if let Some(difficulty_min) = params.difficulty_min {
        query = query.bind(difficulty_min);
    }
    if let Some(difficulty_max) = params.difficulty_max {
        query = query.bind(difficulty_max);
    }
    if let Some(paper_id) = &params.paper_id {
        query = query.bind(paper_id);
    }
    if let Some(search) = &params.q {
        let needle = format!("%{search}%");
        query = query.bind(needle);
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
        + usize::from(params.difficulty_tag.is_some())
        + params.difficulty_min.as_ref().map(|_| 1).unwrap_or(0)
        + params.difficulty_max.as_ref().map(|_| 1).unwrap_or(0)
        + usize::from(params.paper_id.is_some())
        + params.q.as_ref().map(|_| 1).unwrap_or(0)
        + params.score_min.as_ref().map(|_| 1).unwrap_or(0)
        + params.score_max.as_ref().map(|_| 1).unwrap_or(0)
        + 2
}

pub(crate) async fn load_question_tags(
    pool: &PgPool,
    question_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    query("SELECT tag FROM question_tags WHERE question_id = $1::uuid ORDER BY sort_order, tag")
        .bind(question_id)
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| row.get::<String, _>("tag"))
                .collect()
        })
}

pub(crate) async fn load_question_difficulties(
    pool: &PgPool,
    question_id: &str,
) -> Result<QuestionDifficulty, sqlx::Error> {
    query(
        "SELECT algorithm_tag, score, notes FROM question_difficulties WHERE question_id = $1::uuid ORDER BY algorithm_tag",
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
    .map(|rows| QuestionDifficulty {
        entries: rows
            .into_iter()
            .map(|row| {
                (
                    row.get("algorithm_tag"),
                    QuestionDifficultyValue {
                        score: row.get("score"),
                        notes: row.get("notes"),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    })
}

pub(crate) async fn load_question_files(
    pool: &PgPool,
    question_id: &str,
    file_kind: &str,
) -> Result<Vec<QuestionAssetRef>, sqlx::Error> {
    query(
        r#"
        SELECT qf.file_path, qf.file_kind, qf.mime_type, qf.object_id::text AS object_id
        FROM question_files qf
        WHERE qf.question_id = $1::uuid AND qf.file_kind = $2
        ORDER BY qf.file_path
        "#,
    )
    .bind(question_id)
    .bind(file_kind)
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| QuestionAssetRef {
                path: row.get("file_path"),
                file_kind: row.get("file_kind"),
                object_id: row.get("object_id"),
                mime_type: row.get("mime_type"),
            })
            .collect()
    })
}

pub(crate) fn map_paper_summary(row: PgRow) -> PaperSummary {
    PaperSummary {
        paper_id: row.get("paper_id"),
        description: row.get("description"),
        title: row.get("title"),
        subtitle: row.get("subtitle"),
        authors: row.get("authors"),
        reviewers: row.get("reviewers"),
        question_count: row.get("question_count"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(crate) fn map_paper_question_summary(row: PgRow, tags: Vec<String>) -> PaperQuestionSummary {
    PaperQuestionSummary {
        question_id: row.get("question_id"),
        sort_order: row.get("sort_order"),
        category: row.get("category"),
        status: row.get("status"),
        tags,
    }
}

pub(crate) fn map_question_summary(
    row: PgRow,
    tags: Vec<String>,
    difficulty: QuestionDifficulty,
) -> QuestionSummary {
    QuestionSummary {
        question_id: row.get("question_id"),
        source: QuestionSourceRef {
            tex: row.get("source_tex_path"),
        },
        category: row.get("category"),
        status: row.get("status"),
        description: row.get("description"),
        tags,
        difficulty,
        score: row.get("score"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(crate) fn map_question_paper_ref(row: PgRow) -> QuestionPaperRef {
    QuestionPaperRef {
        paper_id: row.get("paper_id"),
        description: row.get("description"),
        title: row.get("title"),
        subtitle: row.get("subtitle"),
        sort_order: row.get("sort_order"),
    }
}

pub(crate) fn map_question_detail(
    row: PgRow,
    tex_object_id: String,
    tags: Vec<String>,
    difficulty: QuestionDifficulty,
    assets: Vec<QuestionAssetRef>,
    papers: Vec<QuestionPaperRef>,
) -> QuestionDetail {
    QuestionDetail {
        question_id: row.get("question_id"),
        tex_object_id,
        source: QuestionSourceRef {
            tex: row.get("source_tex_path"),
        },
        category: row.get("category"),
        status: row.get("status"),
        description: row.get("description"),
        tags,
        difficulty,
        score: row.get("score"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        assets,
        papers,
    }
}

pub(crate) fn map_paper_detail(row: PgRow, questions: Vec<PaperQuestionSummary>) -> PaperDetail {
    PaperDetail {
        paper_id: row.get("paper_id"),
        description: row.get("description"),
        title: row.get("title"),
        subtitle: row.get("subtitle"),
        authors: row.get("authors"),
        reviewers: row.get("reviewers"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        questions,
    }
}
