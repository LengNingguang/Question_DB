use anyhow::Result;
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder};

use super::models::{validate_paper_filters, PapersParams};

#[derive(Debug)]
pub(crate) struct PapersQuery {
    pub(crate) sql: String,
    pub(crate) bind_count: usize,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

impl PapersParams {
    pub(crate) fn build_query(&self) -> PapersQuery {
        let mut builder = QueryBuilder::<Postgres>::new(
            "
            SELECT p.paper_id::text AS paper_id,
                   p.description,
                   p.title,
                   p.subtitle,
                   p.authors,
                   p.reviewers,
                   COUNT(pq_count.question_id) AS question_count,
                   to_char(p.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                   to_char(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
            FROM papers p
            LEFT JOIN paper_questions pq_count ON pq_count.paper_id = p.paper_id
            WHERE 1 = 1",
        );
        let mut bind_count = 0;

        if let Some(question_id) = &self.question_id {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.paper_id = p.paper_id AND pq.question_id = ")
                .push_bind(question_id)
                .push("::uuid)");
            bind_count += 1;
        }
        if let Some(category) = &self.category {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq JOIN questions q ON q.question_id = pq.question_id WHERE pq.paper_id = p.paper_id AND q.category = ")
                .push_bind(category)
                .push(')');
            bind_count += 1;
        }
        if let Some(tag) = &self.tag {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq JOIN question_tags qt ON qt.question_id = pq.question_id WHERE pq.paper_id = p.paper_id AND qt.tag = ")
                .push_bind(tag)
                .push(')');
            bind_count += 1;
        }
        if let Some(search) = &self.q {
            let needle = format!("%{search}%");
            builder
                .push(" AND CONCAT_WS(' ', p.description, p.title, p.subtitle, array_to_string(p.authors, ' '), array_to_string(p.reviewers, ' ')) ILIKE ")
                .push_bind(needle);
            bind_count += 1;
        }

        let limit = self.normalized_limit();
        let offset = self.normalized_offset();
        builder
            .push(
                " GROUP BY p.paper_id, p.description, p.title, p.subtitle, p.authors, p.reviewers, p.created_at, p.updated_at",
            )
            .push(" ORDER BY p.created_at DESC, p.paper_id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        PapersQuery {
            sql: builder.sql().to_owned(),
            bind_count: bind_count + 2,
            limit,
            offset,
        }
    }
}

pub(crate) async fn execute_papers_query(
    pool: &PgPool,
    params: &PapersParams,
    plan: &PapersQuery,
) -> Result<Vec<PgRow>, sqlx::Error> {
    let mut query = query(&plan.sql);
    if let Some(question_id) = &params.question_id {
        query = query.bind(question_id);
    }
    if let Some(category) = &params.category {
        query = query.bind(category);
    }
    if let Some(tag) = &params.tag {
        query = query.bind(tag);
    }
    if let Some(search) = &params.q {
        let needle = format!("%{search}%");
        query = query.bind(needle);
    }
    debug_assert_eq!(plan.bind_count, count_paper_binds(params));
    query
        .bind(plan.limit)
        .bind(plan.offset)
        .fetch_all(pool)
        .await
}

pub(crate) fn count_paper_binds(params: &PapersParams) -> usize {
    usize::from(params.question_id.is_some())
        + usize::from(params.category.is_some())
        + usize::from(params.tag.is_some())
        + usize::from(params.q.is_some())
        + 2
}

pub(crate) fn validate_and_build_papers_query(params: &PapersParams) -> Result<PapersQuery> {
    validate_paper_filters(params)?;
    Ok(params.build_query())
}
