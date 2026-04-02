// ============================================================
// 文件：src/api/questions/queries.rs
// 说明：题目查询构建和行到响应映射
// ============================================================

//! 查询构建器和行到响应的映射
//!
//! 本文件负责：
//! 1. 根据查询参数动态构建 SQL 查询
//! 2. 验证查询参数的合法性
//! 3. 执行数据库查询
//! 4. 将数据库行映射为 API 响应类型

// 导入标准库的 BTreeMap（有序映射）
use std::collections::BTreeMap;

// 导入 anyhow 错误处理库
use anyhow::{anyhow, Result};

// 导入 SQLx 数据库操作相关类型
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder, Row};

// 导入当前模块的模型定义
use super::models::{
    validate_question_category, QuestionAssetRef, QuestionDetail, QuestionDifficulty,
    QuestionDifficultyValue, QuestionPaperRef, QuestionSourceRef, QuestionSummary, QuestionsParams,
};

// 导入试卷模块的模型（用于试卷相关的映射）
use crate::api::papers::models::{PaperDetail, PaperQuestionSummary, PaperSummary};

// ============================================================
// QuestionsQuery 结构体
// ============================================================
/// 题目查询计划
///
/// 保存构建好的 SQL 查询和相关的元数据
///
/// # 字段说明
/// - `sql`: 构建好的 SQL 查询字符串
/// - `bind_count`: 参数绑定数量（用于调试验证）
/// - `limit`: 分页限制（默认 20，最大 100）
/// - `offset`: 分页偏移（默认 0）
#[derive(Debug)]
pub(crate) struct QuestionsQuery {
    /// SQL 查询字符串
    pub(crate) sql: String,
    /// 参数绑定数量
    pub(crate) bind_count: usize,
    /// 分页限制
    pub(crate) limit: i64,
    /// 分页偏移
    pub(crate) offset: i64,
}

// ============================================================
// QuestionsParams 实现
// ============================================================
impl QuestionsParams {
    // --------------------------------------------------------
    // normalized_limit 函数
    // --------------------------------------------------------
    /// 获取规范化后的分页限制
    ///
    /// # 返回值
    /// - 如果有 `limit` 参数：在 [1, 100] 范围内钳制
    /// - 如果没有 `limit` 参数：返回默认值 20
    ///
    /// # 设计说明
    /// - 最小限制为 1（避免无意义的查询）
    /// - 最大限制为 100（防止过度负载）
    /// - 默认值 20 是合理的页面大小
    pub(crate) fn normalized_limit(&self) -> i64 {
        // unwrap_or(20): 没有 limit 时使用默认值 20
        // clamp(1, 100): 限制在 [1, 100] 范围内
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    // --------------------------------------------------------
    // normalized_offset 函数
    // --------------------------------------------------------
    /// 获取规范化后的分页偏移
    ///
    /// # 返回值
    /// - 如果有 `offset` 参数：确保非负
    /// - 如果没有 `offset` 参数：返回默认值 0
    ///
    /// # 设计说明
    /// - 偏移不能为负数（SQL 不允许）
    /// - 默认从第一页开始
    pub(crate) fn normalized_offset(&self) -> i64 {
        // unwrap_or(0): 没有 offset 时使用默认值 0
        // max(0): 确保非负
        self.offset.unwrap_or(0).max(0)
    }

    // --------------------------------------------------------
    // build_query 函数
    // --------------------------------------------------------
    /// 构建查询计划
    ///
    /// 根据查询参数动态生成 SQL 查询语句
    ///
    /// # 返回
    /// 包含 SQL 语句、绑定数量、limit 和 offset 的 QuestionsQuery
    ///
    /// # 支持的过滤条件
    /// | 参数 | SQL 条件 | 说明 |
    /// |------|----------|------|
    /// | category | `q.category = ?` | 题目分类过滤 |
    /// | tag | `EXISTS (SELECT 1 FROM question_tags ...)` | 标签过滤 |
    /// | difficulty_tag | `EXISTS (SELECT 1 FROM question_difficulties ...)` | 难度标签过滤 |
    /// | difficulty_min | `qd.score >= ?` | 最小难度分数 |
    /// | difficulty_max | `qd.score <= ?` | 最大难度分数 |
    /// | paper_id | `EXISTS (SELECT 1 FROM paper_questions ...)` | 所属试卷过滤 |
    /// | q (search) | `description ILIKE ?` | 全文搜索（不区分大小写） |
    ///
    /// # SQL 注入防护
    /// 使用 QueryBuilder 的参数绑定机制，所有用户输入都通过 `push_bind()` 传递
    /// 这确保了输入会被正确转义，防止 SQL 注入攻击
    ///
    /// # 示例 SQL
    /// ```sql
    /// SELECT q.question_id::text AS question_id,
    ///        q.source_tex_path,
    ///        q.category,
    ///        q.status,
    ///        COALESCE(q.description, '') AS description,
    ///        to_char(q.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS created_at,
    ///        to_char(q.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS updated_at
    /// FROM questions q
    /// WHERE 1 = 1
    ///   AND q.category = $1
    ///   AND EXISTS (SELECT 1 FROM question_tags qt WHERE qt.question_id = q.question_id AND qt.tag = $2)
    /// ORDER BY q.created_at DESC, q.question_id
    /// LIMIT $3 OFFSET $4
    /// ```
    pub(crate) fn build_query(&self) -> QuestionsQuery {
        // 初始化 QueryBuilder，设置基础 SQL 查询
        // WHERE 1 = 1 是一个常用技巧，便于后续动态添加 AND 条件
        let mut builder = QueryBuilder::<Postgres>::new(
            "
            SELECT q.question_id::text AS question_id,
                   q.source_tex_path,
                   q.category,
                   q.status,
                   COALESCE(q.description, '') AS description,
                   to_char(q.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                   to_char(q.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
            FROM questions q
            WHERE 1 = 1",
        );

        // 跟踪参数绑定数量（用于调试和验证）
        let mut bind_count = 0;

        // ----------------------------------------------------
        // 条件 1: 分类过滤
        // ----------------------------------------------------
        if let Some(category) = &self.category {
            builder.push(" AND q.category = ").push_bind(category);
            bind_count += 1;
        }

        // ----------------------------------------------------
        // 条件 2: 标签过滤（使用 EXISTS 子查询）
        // ----------------------------------------------------
        if let Some(tag) = &self.tag {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_tags qt WHERE qt.question_id = q.question_id AND qt.tag = ")
                .push_bind(tag)
                .push(")");
            bind_count += 1;
        }

        // ----------------------------------------------------
        // 条件 3: 难度标签过滤（支持范围）
        // ----------------------------------------------------
        if let Some(difficulty_tag) = &self.difficulty_tag {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_difficulties qd WHERE qd.question_id = q.question_id AND qd.algorithm_tag = ")
                .push_bind(difficulty_tag);
            bind_count += 1;

            // 可选：最小难度分数
            if let Some(difficulty_min) = self.difficulty_min {
                builder.push(" AND qd.score >= ").push_bind(difficulty_min);
                bind_count += 1;
            }

            // 可选：最大难度分数
            if let Some(difficulty_max) = self.difficulty_max {
                builder.push(" AND qd.score <= ").push_bind(difficulty_max);
                bind_count += 1;
            }

            builder.push(")");
        }

        // ----------------------------------------------------
        // 条件 4: 所属试卷过滤
        // ----------------------------------------------------
        if let Some(paper_id) = &self.paper_id {
            builder
                .push(" AND EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.question_id = q.question_id AND pq.paper_id = ")
                .push_bind(paper_id)
                .push("::uuid)");
            bind_count += 1;
        }

        // ----------------------------------------------------
        // 条件 5: 全文搜索（description 字段）
        // ----------------------------------------------------
        if let Some(search) = &self.q {
            // 使用 ILIKE 进行不区分大小写的模糊匹配
            // % 是 SQL 通配符，表示任意字符序列
            let needle = format!("%{search}%");
            builder
                .push(" AND COALESCE(q.description, '') ILIKE ")
                .push_bind(needle);
            bind_count += 1;
        }

        // ----------------------------------------------------
        // 添加分页和排序
        // ----------------------------------------------------
        let limit = self.normalized_limit();
        let offset = self.normalized_offset();

        // 按创建时间倒序排序（新的在前），相同时间按 ID 排序（保证稳定性）
        builder
            .push(" ORDER BY q.created_at DESC, q.question_id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        // 返回构建好的查询计划
        // bind_count + 2 是因为最后添加了 limit 和 offset 两个绑定
        QuestionsQuery {
            sql: builder.sql().to_owned(),
            bind_count: bind_count + 2,
            limit,
            offset,
        }
    }
}

// ============================================================
// validate_question_filters 函数
// ============================================================
/// 验证查询参数的合法性
///
/// # 验证规则
/// | 参数 | 规则 | 错误信息 |
/// |------|------|----------|
/// | category | 必须是 none/T/E 之一 | "category must be one of: none, T, E" |
/// | difficulty_tag | 不能为空字符串 | "difficulty_tag must not be empty" |
/// | difficulty_min/max | 必须在 [1, 10] 范围内 | "difficulty_min/max must be between 1 and 10" |
/// | difficulty_min + difficulty_max | min 必须 <= max | "difficulty_min must be less than or equal to difficulty_max" |
/// | difficulty_min/max 单独使用 | 必须同时提供 difficulty_tag | "difficulty_tag is required when difficulty_min or difficulty_max is provided" |
/// | q (search) | 不能为空字符串 | "q must not be empty" |
///
/// # 返回值
/// - Ok(()): 所有参数都合法
/// - Err(anyhow::Error): 第一个失败的验证错误
pub(crate) fn validate_question_filters(params: &QuestionsParams) -> Result<()> {
    // 验证分类参数
    if let Some(category) = &params.category {
        validate_question_category(category)
            .map_err(|_| anyhow!("category must be one of: none, T, E"))?;
    }

    // 验证难度标签不能为空
    if let Some(difficulty_tag) = &params.difficulty_tag {
        if difficulty_tag.trim().is_empty() {
            return Err(anyhow!("difficulty_tag must not be empty"));
        }
    }

    // 验证：如果提供了难度范围，必须同时提供难度标签
    // 原因：难度分数是针对特定标签的，没有标签就无法确定范围的意义
    if (params.difficulty_min.is_some() || params.difficulty_max.is_some())
        && params.difficulty_tag.is_none()
    {
        return Err(anyhow!(
            "difficulty_tag is required when difficulty_min or difficulty_max is provided"
        ));
    }

    // 验证最小难度范围
    if let Some(difficulty_min) = params.difficulty_min {
        if !(1..=10).contains(&difficulty_min) {
            return Err(anyhow!("difficulty_min must be between 1 and 10"));
        }
    }

    // 验证最大难度范围
    if let Some(difficulty_max) = params.difficulty_max {
        if !(1..=10).contains(&difficulty_max) {
            return Err(anyhow!("difficulty_max must be between 1 and 10"));
        }
    }

    // 验证：最小难度必须 <= 最大难度
    if let (Some(difficulty_min), Some(difficulty_max)) =
        (params.difficulty_min, params.difficulty_max)
    {
        if difficulty_min > difficulty_max {
            return Err(anyhow!(
                "difficulty_min must be less than or equal to difficulty_max"
            ));
        }
    }

    // 验证搜索关键词不能为空
    if let Some(q) = &params.q {
        if q.trim().is_empty() {
            return Err(anyhow!("q must not be empty"));
        }
    }

    // 所有验证通过
    Ok(())
}

// ============================================================
// execute_questions_query 函数
// ============================================================
/// 执行题目查询
///
/// # 参数
/// - pool: 数据库连接池
/// - params: 查询参数（用于绑定）
/// - plan: 查询计划（包含 SQL 和元数据）
///
/// # 返回值
/// - Ok(Vec<PgRow>): 查询结果行
/// - Err(sqlx::Error): SQL 执行错误
///
/// # 设计说明
/// 此函数负责将参数绑定到 SQL 查询并执行
/// 使用 `debug_assert_eq!` 验证绑定数量是否正确（仅调试模式）
pub(crate) async fn execute_questions_query(
    pool: &PgPool,
    params: &QuestionsParams,
    plan: &QuestionsQuery,
) -> Result<Vec<PgRow>, sqlx::Error> {
    // 从计划中创建基础查询
    let mut query = query(&plan.sql);

    // 按顺序绑定参数（必须与 build_query 中的顺序一致）
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
        // 搜索关键词需要重新格式化（添加通配符）
        let needle = format!("%{search}%");
        query = query.bind(needle);
    }

    // 调试模式断言：绑定数量必须匹配
    // 这是一个重要的 sanity check，防止 build_query 和此函数的逻辑不一致
    debug_assert_eq!(plan.bind_count, count_question_binds(params));

    // 绑定 limit 和 offset，然后执行查询
    query
        .bind(plan.limit)
        .bind(plan.offset)
        .fetch_all(pool)
        .await
}

// ============================================================
// count_question_binds 函数
// ============================================================
/// 计算查询参数的绑定数量
///
/// # 用途
/// 仅用于调试验证（被 debug_assert_eq! 调用）
///
/// # 返回值
/// 参数绑定总数 = 条件参数数量 + 2（limit 和 offset）
pub(crate) fn count_question_binds(params: &QuestionsParams) -> usize {
    // 计算每个可选参数的绑定数量
    usize::from(params.category.is_some())
        + usize::from(params.tag.is_some())
        + usize::from(params.difficulty_tag.is_some())
        + params.difficulty_min.as_ref().map(|_| 1).unwrap_or(0)
        + params.difficulty_max.as_ref().map(|_| 1).unwrap_or(0)
        + usize::from(params.paper_id.is_some())
        + params.q.as_ref().map(|_| 1).unwrap_or(0)
        + 2  // limit 和 offset
}

// ============================================================
// load_question_tags 函数
// ============================================================
/// 加载题目的标签列表
///
/// # 参数
/// - pool: 数据库连接池
/// - question_id: 题目 ID（文本格式）
///
/// # 返回值
/// - Ok(Vec<String>): 标签列表，按 sort_order 和 tag 排序
/// - Err(sqlx::Error): SQL 执行错误
///
/// # SQL 查询
/// ```sql
/// SELECT tag FROM question_tags
/// WHERE question_id = $1::uuid
/// ORDER BY sort_order, tag
/// ```
pub(crate) async fn load_question_tags(
    pool: &PgPool,
    question_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    query("SELECT tag FROM question_tags WHERE question_id = $1::uuid ORDER BY sort_order, tag")
        .bind(question_id)
        .fetch_all(pool)
        .await
        .map(|rows| {
            // 将行迭代器转换为标签字符串的 Vec
            rows.into_iter()
                .map(|row| row.get::<String, _>("tag"))
                .collect()
        })
}

// ============================================================
// load_question_difficulties 函数
// ============================================================
/// 加载题目的难度评估
///
/// # 参数
/// - pool: 数据库连接池
/// - question_id: 题目 ID（文本格式）
///
/// # 返回值
/// - Ok(QuestionDifficulty): BTreeMap<算法标签，难度值>
/// - Err(sqlx::Error): SQL 执行错误
///
/// # 数据说明
/// QuestionDifficulty 包含一个 entries 字段
/// entries 是 BTreeMap<String, QuestionDifficultyValue>
/// - key: algorithm_tag（算法标签）
/// - value: QuestionDifficultyValue { score, notes }
///
/// # SQL 查询
/// ```sql
/// SELECT algorithm_tag, score, notes
/// FROM question_difficulties
/// WHERE question_id = $1::uuid
/// ORDER BY algorithm_tag
/// ```
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
        // 将行迭代器转换为 BTreeMap
        entries: rows
            .into_iter()
            .map(|row| {
                (
                    row.get("algorithm_tag"),  // key: 算法标签
                    QuestionDifficultyValue {
                        score: row.get("score"),    // 难度分数 (1-10)
                        notes: row.get("notes"),    // 可选备注
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    })
}

// ============================================================
// load_question_files 函数
// ============================================================
/// 加载题目的关联文件
///
/// # 参数
/// - pool: 数据库连接池
/// - question_id: 题目 ID（文本格式）
/// - file_kind: 文件类型（如 "solution", "answer" 等）
///
/// # 返回值
/// - Ok(Vec<QuestionAssetRef>): 文件引用列表
/// - Err(sqlx::Error): SQL 执行错误
///
/// # QuestionAssetRef 结构
/// - path: 文件路径
/// - file_kind: 文件类型
/// - object_id: 对象存储 ID
/// - mime_type: MIME 类型
///
/// # SQL 查询
/// ```sql
/// SELECT qf.file_path, qf.file_kind, qf.mime_type, qf.object_id::text AS object_id
/// FROM question_files qf
/// WHERE qf.question_id = $1::uuid AND qf.file_kind = $2
/// ORDER BY qf.file_path
/// ```
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
        // 将行迭代器转换为 QuestionAssetRef 的 Vec
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

// ============================================================
// map_paper_summary 函数
// ============================================================
/// 将数据库行映射为 PaperSummary
///
/// # 用途
/// 用于试卷摘要查询的响应映射
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

// ============================================================
// map_paper_question_summary 函数
// ============================================================
/// 将数据库行和标签映射为 PaperQuestionSummary
///
/// # 参数
/// - row: 数据库行（包含题目基本信息）
/// - tags: 预加载的题目标签列表
///
/// # 用途
/// 用于试卷详情中的题目摘要列表
pub(crate) fn map_paper_question_summary(row: PgRow, tags: Vec<String>) -> PaperQuestionSummary {
    PaperQuestionSummary {
        question_id: row.get("question_id"),
        sort_order: row.get("sort_order"),
        category: row.get("category"),
        status: row.get("status"),
        tags,
    }
}

// ============================================================
// map_question_summary 函数
// ============================================================
/// 将数据库行、标签和难度映射为 QuestionSummary
///
/// # 参数
/// - row: 数据库行（包含题目基本信息）
/// - tags: 预加载的题目标签列表
/// - difficulty: 预加载的题目难度评估
///
/// # QuestionSummary 结构
/// - question_id: 题目 ID
/// - source: QuestionSourceRef { tex: 源文件路径 }
/// - category: 分类 (none/T/E)
/// - status: 状态 (none/available/archived)
/// - description: 描述
/// - tags: 标签列表
/// - difficulty: 难度评估
/// - created_at/updated_at: 时间戳
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
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

// ============================================================
// map_question_paper_ref 函数
// ============================================================
/// 将数据库行映射为 QuestionPaperRef
///
/// # 用途
/// 用于题目详情中展示所属试卷的引用
///
/// # QuestionPaperRef 结构
/// - paper_id: 试卷 ID
/// - description: 试卷描述
/// - title: 试卷标题
/// - subtitle: 试卷副标题
/// - sort_order: 排序顺序
pub(crate) fn map_question_paper_ref(row: PgRow) -> QuestionPaperRef {
    QuestionPaperRef {
        paper_id: row.get("paper_id"),
        description: row.get("description"),
        title: row.get("title"),
        subtitle: row.get("subtitle"),
        sort_order: row.get("sort_order"),
    }
}

// ============================================================
// map_question_detail 函数
// ============================================================
/// 将数据库行和关联数据映射为 QuestionDetail
///
/// # 参数
/// - row: 数据库行
/// - tex_object_id: TeX 文件的对象存储 ID
/// - tags: 预加载的题目标签列表
/// - difficulty: 预加载的题目难度评估
/// - assets: 预加载的关联文件列表
/// - papers: 预加载的所属试卷列表
///
/// # QuestionDetail 结构
/// QuestionDetail 是题目的完整表示，包含：
/// - 基本信息（ID、分类、状态、描述）
/// - 源文件信息（tex_object_id, source）
/// - 关联数据（tags, difficulty, assets, papers）
/// - 时间戳（created_at, updated_at）
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
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        assets,
        papers,
    }
}

// ============================================================
// map_paper_detail 函数
// ============================================================
/// 将数据库行和题目列表映射为 PaperDetail
///
/// # 参数
/// - row: 数据库行（包含试卷基本信息）
/// - questions: 预加载的题目摘要列表
///
/// # PaperDetail 结构
/// PaperDetail 是试卷的完整表示，包含：
/// - 基本信息（ID、标题、副标题、描述、作者、审阅者）
/// - 题目列表（questions）
/// - 时间戳（created_at, updated_at）
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

// ============================================================
// 知识点讲解 (SQL 查询构建和映射)
// ============================================================
//
// 1. QueryBuilder 动态 SQL 构建
//    - 用于根据可选参数动态构建 WHERE 子句
//    - push() 添加 SQL 片段
//    - push_bind() 添加参数绑定（防止 SQL 注入）
//    - 最终通过.sql().to_owned() 获取 SQL 字符串
//
// 2. WHERE 1 = 1 技巧
//    - 便于后续统一用 "AND 条件" 追加
//    - 避免判断是否是第一个条件
//    - 对性能无影响（优化器会消除）
//
// 3. EXISTS 子查询
//    - 用于一对多关系的过滤
//    - 比 JOIN 更高效（找到匹配即可停止）
//    - 示例：检查题目是否有某个标签
//
// 4. COALESCE 函数
//    - 处理 NULL 值
//    - COALESCE(q.description, '') 将 NULL 转为空字符串
//    - 避免客户端处理 NULL
//
// 5. 时区转换
//    - q.created_at AT TIME ZONE 'UTC'
//    - 将时间戳转换为 UTC 时区
//    - to_char 格式化为 ISO 8601 字符串
//
// 6. BTreeMap vs HashMap
//    - BTreeMap: 有序（按键排序），遍历顺序稳定
//    - HashMap: 无序，但平均性能更好
//    - 这里使用 BTreeMap 确保响应的一致性
//
// 7. debug_assert_eq!
//    - 仅在调试模式（非 release）下检查
//    - 用于验证 build_query 和 execute_questions_query 的一致性
//    - release 模式下无开销
//
// ============================================================
// 查询流程图
// ============================================================
//
// 用户请求 (Query Params)
//        │
//        ▼
// QuestionsParams::build_query()
//        │
//        ├─► 验证参数 (validate_question_filters)
//        │
//        ├─► 构建 SQL (QueryBuilder)
//        │   │
//        │   ├─► category 过滤
//        │   ├─► tag 过滤 (EXISTS 子查询)
//        │   ├─► difficulty 过滤 (EXISTS + 范围)
//        │   ├─► paper_id 过滤 (EXISTS 子查询)
//        │   └─► search 过滤 (ILIKE)
//        │
//        └─► QuestionsQuery { sql, bind_count, limit, offset }
//                │
//                ▼
//        execute_questions_query()
//                │
//                ├─► 绑定参数
//                │
//                ▼
//         query.fetch_all(pool)
//                │
//                ▼
//           Vec<PgRow>
//                │
//                ├─► 加载标签 (load_question_tags)
//                ├─► 加载难度 (load_question_difficulties)
//                ├─► 加载文件 (load_question_files)
//                └─► 加载试卷 (load_paper_refs)
//                        │
//                        ▼
//                map_question_summary() / map_question_detail()
//                        │
//                        ▼
//                   QuestionSummary / QuestionDetail
//
// ============================================================
