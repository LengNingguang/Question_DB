//! Query planning and row-to-response mapping for question endpoints.

use anyhow::{anyhow, Result};
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder, Row};

use super::models::{
    validate_question_category, QuestionAlgorithmScore, QuestionAssetRef, QuestionDetail,
    QuestionDifficulty, QuestionPaperRef, QuestionSourceRef, QuestionSummary, QuestionsParams,
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
                   COALESCE(q.notes, '') AS notes,
                   q.difficulty_human,
                   q.difficulty_notes,
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
        if let Some(paper_id) = &self.paper_id {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.question_id = q.question_id AND pq.paper_id = ")
                .push_bind(paper_id)
                .push("::uuid)");
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
                .push(" AND (q.question_id::text ILIKE ")
                .push_bind(needle.clone())
                .push(" OR COALESCE(q.notes, '') ILIKE ")
                .push_bind(needle.clone())
                .push(" OR COALESCE(q.source_tex_path, '') ILIKE ")
                .push_bind(needle)
                .push(')');
            bind_count += 3;
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
    if let Some(paper_type) = &params.paper_type {
        let valid = ["regular", "semifinal", "final", "other"];
        if !valid.contains(&paper_type.as_str()) {
            return Err(anyhow!(
                "paper_type must be one of: regular, semifinal, final, other"
            ));
        }
    }
    if let Some(category) = &params.category {
        validate_question_category(category)
            .map_err(|_| anyhow!("category must be one of: none, T, E"))?;
    }
    if let Some(q) = &params.q {
        if q.trim().is_empty() {
            return Err(anyhow!("q must not be empty"));
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
    if let Some(paper_id) = &params.paper_id {
        query = query.bind(paper_id);
    }
    if let Some(paper_type) = &params.paper_type {
        query = query.bind(paper_type);
    }
    if let Some(search) = &params.q {
        let needle = format!("%{search}%");
        query = query.bind(needle.clone()).bind(needle.clone()).bind(needle);
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
        + usize::from(params.paper_type.is_some())
        + params.q.as_ref().map(|_| 3).unwrap_or(0)
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

pub(crate) async fn load_question_algorithms(
    pool: &PgPool,
    question_id: &str,
) -> Result<Vec<QuestionAlgorithmScore>, sqlx::Error> {
    query(
        "SELECT algorithm_tag, score FROM question_difficulty_algorithms WHERE question_id = $1::uuid ORDER BY algorithm_tag",
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| QuestionAlgorithmScore {
                tag: row.get("algorithm_tag"),
                score: row.get("score"),
            })
            .collect()
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
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
        notes: row.get("notes"),
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
    algorithms: Vec<QuestionAlgorithmScore>,
) -> QuestionSummary {
    QuestionSummary {
        question_id: row.get("question_id"),
        source: QuestionSourceRef {
            tex: row.get("source_tex_path"),
        },
        category: row.get("category"),
        status: row.get("status"),
        notes: row.get("notes"),
        tags,
        difficulty: QuestionDifficulty {
            human: row.get("difficulty_human"),
            algorithm: algorithms,
            notes: row.get("difficulty_notes"),
        },
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(crate) fn map_question_paper_ref(row: PgRow) -> QuestionPaperRef {
    QuestionPaperRef {
        paper_id: row.get("paper_id"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
        sort_order: row.get("sort_order"),
    }
}

pub(crate) fn map_question_detail(
    row: PgRow,
    tex_object_id: String,
    tags: Vec<String>,
    algorithms: Vec<QuestionAlgorithmScore>,
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
        notes: row.get("notes"),
        tags,
        difficulty: QuestionDifficulty {
            human: row.get("difficulty_human"),
            algorithm: algorithms,
            notes: row.get("difficulty_notes"),
        },
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        assets,
        papers,
    }
}

pub(crate) fn map_paper_detail(row: PgRow, questions: Vec<PaperQuestionSummary>) -> PaperDetail {
    PaperDetail {
        paper_id: row.get("paper_id"),
        edition: row.get("edition"),
        paper_type: row.get("paper_type"),
        title: row.get("title"),
        notes: row.get("notes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        questions,
    }
}
