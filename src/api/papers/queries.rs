// ============================================================
// 文件：src/api/papers/queries.rs
// 说明：试卷查询构建和行映射
// ============================================================

//! 试卷查询构建和数据库行映射
//!
//! 提供动态 SQL 构建、参数验证、行转响应对象等功能

// 导入 anyhow 错误处理
use anyhow::Result;

// 导入 SQLx 数据库操作
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder};

// 导入当前模块的模型和验证函数
use super::models::{validate_paper_filters, PapersParams};

// ============================================================
// PapersQuery 结构体
// ============================================================
/// 试卷查询计划
///
/// # 字段
/// - sql: 生成的 SQL 语句
/// - bind_count: 绑定参数数量
/// - limit: 分页限制
/// - offset: 分页偏移
#[derive(Debug)]
pub(crate) struct PapersQuery {
    pub(crate) sql: String,
    pub(crate) bind_count: usize,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

// ============================================================
// PapersParams 实现
// ============================================================
impl PapersParams {
    /// 构建查询计划
    ///
    /// # 查询字段
    /// - paper_id: 试卷 ID
    /// - description: 描述
    /// - title: 标题
    /// - subtitle: 子标题
    /// - authors: 作者数组
    /// - reviewers: 审核者数组
    /// - question_count: 题目数量（通过 COUNT 计算）
    /// - created_at/updated_at: 时间戳
    ///
    /// # 支持的过滤条件
    /// - question_id: 按包含的题目 ID 过滤
    /// - category: 按题目分类过滤
    /// - tag: 按题目标签过滤
    /// - q: 搜索关键词（匹配描述、标题、作者等）
    pub(crate) fn build_query(&self) -> PapersQuery {
        // 使用 QueryBuilder 构建动态 SQL
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
            WHERE 1 = 1",  // 恒真条件，便于后续添加 AND 子句
        );
        let mut bind_count = 0;

        // ========== 添加 question_id 过滤 ==========
        // EXISTS 子查询：试卷包含指定题目
        if let Some(question_id) = &self.question_id {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.paper_id = p.paper_id AND pq.question_id = ")
                .push_bind(question_id)
                .push("::uuid)");
            bind_count += 1;
        }

        // ========== 添加 category 过滤 ==========
        // EXISTS 子查询：试卷包含指定分类的题目
        if let Some(category) = &self.category {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq JOIN questions q ON q.question_id = pq.question_id WHERE pq.paper_id = p.paper_id AND q.category = ")
                .push_bind(category)
                .push(')');
            bind_count += 1;
        }

        // ========== 添加 tag 过滤 ==========
        // EXISTS 子查询：试卷包含带有指定标签的题目
        if let Some(tag) = &self.tag {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq JOIN question_tags qt ON qt.question_id = pq.question_id WHERE pq.paper_id = p.paper_id AND qt.tag = ")
                .push_bind(tag)
                .push(')');
            bind_count += 1;
        }

        // ========== 添加搜索关键词过滤 ==========
        // 使用 CONCAT_WS 连接多个字段，ILIKE 模糊匹配
        if let Some(search) = &self.q {
            let needle = format!("%{search}%");  // SQL LIKE 模式
            builder
                .push(" AND CONCAT_WS(' ', p.description, p.title, p.subtitle, array_to_string(p.authors, ' '), array_to_string(p.reviewers, ' ')) ILIKE ")
                .push_bind(needle);
            bind_count += 1;
        }

        // ========== 添加分页和排序 ==========
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
            sql: builder.sql().to_owned(),  // 获取生成的 SQL
            bind_count: bind_count + 2,      // +2 是 limit 和 offset
            limit,
            offset,
        }
    }
}

// ============================================================
// execute_papers_query 函数
// ============================================================
/// 执行试卷查询
///
/// # 参数
/// - pool: 数据库连接池
/// - params: 查询参数
/// - plan: 查询计划
///
/// # 返回
/// - Ok: 数据库行列表
/// - Err: SQL 错误
pub(crate) async fn execute_papers_query(
    pool: &PgPool,
    params: &PapersParams,
    plan: &PapersQuery,
) -> Result<Vec<PgRow>, sqlx::Error> {
    // 创建查询对象
    let mut query = query(&plan.sql);

    // 按顺序绑定参数（必须与 build_query 中的顺序一致）
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

    // 调试断言：验证绑定参数数量
    debug_assert_eq!(plan.bind_count, count_paper_binds(params));

    // 绑定 limit 和 offset，执行查询
    query
        .bind(plan.limit)
        .bind(plan.offset)
        .fetch_all(pool)
        .await
}

// ============================================================
// count_paper_binds 函数
// ============================================================
/// 计算查询参数中的绑定数量
///
/// # 说明
/// 用于调试断言，验证 build_query 和 execute_papers_query 的一致性
pub(crate) fn count_paper_binds(params: &PapersParams) -> usize {
    usize::from(params.question_id.is_some())
        + usize::from(params.category.is_some())
        + usize::from(params.tag.is_some())
        + usize::from(params.q.is_some())
        + 2  // limit 和 offset
}

// ============================================================
// validate_and_build_papers_query 函数
// ============================================================
/// 验证参数并构建查询计划
///
/// # 参数
/// - params: 查询参数
///
/// # 返回
/// - Ok: 查询计划
/// - Err: 验证失败错误
pub(crate) fn validate_and_build_papers_query(params: &PapersParams) -> Result<PapersQuery> {
    // 先验证参数（UUID 格式、分类合法性等）
    validate_paper_filters(params)?;
    // 构建查询计划
    Ok(params.build_query())
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. QueryBuilder 动态 SQL 构建
 *    - 链式调用 push() 和 push_bind()
 *    - 避免 SQL 注入（使用绑定参数）
 *    - 最终用 .sql() 获取完整 SQL
 *
 * 2. EXISTS 子查询
 *    - 用于一对多关系过滤
 *    - 比 JOIN + DISTINCT 更高效
 *    - 示例：查找包含某题目的试卷
 *
 * 3. CONCAT_WS 函数
 *    - CONCAT_WS(separator, ...) 连接字符串
 *    - WS = With Separator（带分隔符）
 *    - NULL 值会被跳过
 *
 * 4. ILIKE 操作符
 *    - 不区分大小写的 LIKE
 *    - % 是通配符（匹配任意字符）
 *    - 示例：%keyword% 包含 keyword
 *
 * 5. LEFT JOIN + COUNT
 *    - LEFT JOIN 保留所有试卷（即使没有题目）
 *    - COUNT 计算题目数量
 *    - GROUP BY 按试卷分组
 *
 * ============================================================
 * 查询条件对应关系
 * ============================================================
 *
 * 参数              →  SQL 条件
 * question_id       →  EXISTS (SELECT 1 FROM paper_questions WHERE question_id = ?)
 * category          →  EXISTS (SELECT 1 FROM paper_questions JOIN questions WHERE category = ?)
 * tag               →  EXISTS (SELECT 1 FROM paper_questions JOIN question_tags WHERE tag = ?)
 * q (search)        →  CONCAT_WS(...) ILIKE '%keyword%'
 *
 * ============================================================
 * SQL 生成示例
 * ============================================================
 *
 * 输入参数:
 *   category=T, limit=10, offset=0
 *
 * 生成的 SQL:
 *   SELECT p.paper_id, p.description, ..., COUNT(pq.question_id) AS question_count
 *   FROM papers p
 *   LEFT JOIN paper_questions pq_count ON pq_count.paper_id = p.paper_id
 *   WHERE 1 = 1
 *     AND EXISTS (SELECT 1 FROM paper_questions pq
 *                   JOIN questions q ON q.question_id = pq.question_id
 *                   WHERE pq.paper_id = p.paper_id AND q.category = $1)
 *   GROUP BY p.paper_id, ...
 *   ORDER BY p.created_at DESC, p.paper_id
 *   LIMIT $2 OFFSET $3
 *
 */
