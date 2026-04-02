// ============================================================
// 文件：src/api/papers/handlers.rs
// 说明：试卷管理的 HTTP 请求处理器
// ============================================================

//! 试卷管理 API 的请求处理器
//!
//! 实现试卷的查询、创建、详情、更新、删除、文件替换等 HTTP 端点

// 导入标准库集合类型
use std::collections::HashSet;

// 导入 anyhow 错误处理库
use anyhow::{anyhow, bail, Result};

// 导入 Serde 序列化库
use serde::{Deserialize, Serialize};

// 导入题目分类验证函数
use crate::api::questions::models::validate_question_category;

// 导入共享工具函数
use crate::api::shared::utils::{
    normalize_bundle_description, normalize_optional_bundle_description,
};

// ============================================================
// PaperSummary 结构体
// ============================================================
/// 试卷摘要响应
///
/// # 字段
/// - paper_id: 试卷 UUID
/// - description: 试卷描述
/// - title: 试卷标题
/// - subtitle: 子标题
/// - authors: 作者列表
/// - reviewers: 审核者列表
/// - question_count: 题目数量
/// - created_at/updated_at: 时间戳
#[derive(Debug, Serialize)]
pub struct PaperSummary {
    pub(crate) paper_id: String,
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) question_count: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

// ============================================================
// PaperQuestionSummary 结构体
// ============================================================
/// 试卷中题目的摘要
///
/// # 字段
/// - question_id: 题目 UUID
/// - sort_order: 排序顺序
/// - category: 题目分类
/// - status: 题目状态
/// - tags: 题目标签
#[derive(Debug, Serialize)]
pub struct PaperQuestionSummary {
    pub(crate) question_id: String,
    pub(crate) sort_order: i32,
    pub(crate) category: String,
    pub(crate) status: String,
    pub(crate) tags: Vec<String>,
}

// ============================================================
// PaperDetail 结构体
// ============================================================
/// 试卷详情响应
///
/// # 字段
/// - 基本信息：ID、描述、标题、作者等
/// - questions: 试卷包含的题目列表
#[derive(Debug, Serialize)]
pub struct PaperDetail {
    pub(crate) paper_id: String,
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) questions: Vec<PaperQuestionSummary>,
}

// ============================================================
// CreatePaperRequest 结构体
// ============================================================
/// 创建试卷的请求（内部使用）
///
/// # 字段
/// - description: 试卷描述
/// - title: 标题
/// - subtitle: 子标题
/// - authors: 作者列表
/// - reviewers: 审核者列表
/// - question_ids: 题目 ID 列表
#[derive(Debug)]
pub(crate) struct CreatePaperRequest {
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) question_ids: Vec<String>,
}

// ============================================================
// PapersParams 结构体
// ============================================================
/// 试卷列表查询参数
///
/// # 字段
/// - question_id: 按题目 ID 过滤
/// - category: 按分类过滤
/// - tag: 按标签过滤
/// - q: 搜索关键词
/// - limit/offset: 分页参数
#[derive(Debug, Deserialize)]
pub(crate) struct PapersParams {
    pub(crate) question_id: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

// ============================================================
// PaperImportResponse 结构体
// ============================================================
/// 试卷导入响应
#[derive(Debug, Serialize)]
pub(crate) struct PaperImportResponse {
    pub(crate) paper_id: String,
    pub(crate) file_name: String,
    pub(crate) status: &'static str,
    pub(crate) question_count: usize,
}

// ============================================================
// PaperFileReplaceResponse 结构体
// ============================================================
/// 试卷文件替换响应
#[derive(Debug, Serialize)]
pub(crate) struct PaperFileReplaceResponse {
    pub(crate) paper_id: String,
    pub(crate) file_name: String,
    pub(crate) status: &'static str,
}

// ============================================================
// UpdatePaperRequest 结构体
// ============================================================
/// 更新试卷的请求
///
/// # 说明
/// 所有字段都是可选的，支持部分更新
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]  // 拒绝未知字段
pub(crate) struct UpdatePaperRequest {
    #[serde(default)]
    pub(crate) description: Option<Option<String>>,
    #[serde(default)]
    pub(crate) title: Option<Option<String>>,
    #[serde(default)]
    pub(crate) subtitle: Option<Option<String>>,
    #[serde(default)]
    pub(crate) authors: Option<Option<Vec<String>>>,
    #[serde(default)]
    pub(crate) reviewers: Option<Option<Vec<String>>>,
    #[serde(default)]
    pub(crate) question_ids: Option<Option<Vec<String>>>,
}

// ============================================================
// NormalizedCreatePaperRequest 结构体
// ============================================================
/// 规范化后的创建请求
#[derive(Debug)]
pub(crate) struct NormalizedCreatePaperRequest {
    pub(crate) description: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) question_ids: Vec<String>,
}

// ============================================================
// NormalizedPaperUpdate 结构体
// ============================================================
/// 规范化后的更新请求
#[derive(Debug)]
pub(crate) struct NormalizedPaperUpdate {
    pub(crate) description: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) subtitle: Option<String>,
    pub(crate) authors: Option<Vec<String>>,
    pub(crate) reviewers: Option<Vec<String>>,
    pub(crate) question_ids: Option<Vec<String>>,
}

// ============================================================
// PaperDeleteResponse 结构体
// ============================================================
/// 删除试卷响应
#[derive(Debug, Serialize)]
pub(crate) struct PaperDeleteResponse {
    pub(crate) paper_id: String,
    pub(crate) status: &'static str,
}

// ============================================================
// CreatePaperRequest 实现
// ============================================================
impl CreatePaperRequest {
    /// 规范化请求数据
    ///
    /// # 验证
    /// - 描述、标题、子标题：修剪空白，不能为空
    /// - 作者、审核者：去重，修剪空白
    /// - 题目 IDs：验证 UUID 格式，去重，不能为空
    pub(crate) fn normalize(self) -> Result<NormalizedCreatePaperRequest> {
        // 规范化各字段
        let description = normalize_required_description("description", &self.description)?;
        let title = normalize_required_metadata_text("title", &self.title)?;
        let subtitle = normalize_required_metadata_text("subtitle", &self.subtitle)?;
        let authors = normalize_text_list("authors", self.authors)?;
        let reviewers = normalize_text_list("reviewers", self.reviewers)?;
        let question_ids = normalize_question_ids(self.question_ids)?;

        // 组装规范化后的请求
        Ok(NormalizedCreatePaperRequest {
            description,
            title,
            subtitle,
            authors,
            reviewers,
            question_ids,
        })
    }
}

// ============================================================
// UpdatePaperRequest 实现
// ============================================================
impl UpdatePaperRequest {
    /// 规范化更新请求
    ///
    /// # 验证
    /// - 至少包含一个更新字段
    /// - 各字段分别验证（不能为空、去重等）
    pub(crate) fn normalize(self) -> Result<NormalizedPaperUpdate> {
        // 检查是否至少有一个字段
        if self.description.is_none()
            && self.title.is_none()
            && self.subtitle.is_none()
            && self.authors.is_none()
            && self.reviewers.is_none()
            && self.question_ids.is_none()
        {
            return Err(anyhow!(
                "request body must include at least one of: description, title, subtitle, authors, reviewers, question_ids"
            ));
        }

        // 分别规范化各字段
        let description = self
            .description
            .map(|value| normalize_required_plaintext("description", value))
            .transpose()?;
        let title = self
            .title
            .map(|value| normalize_required_metadata_option("title", value))
            .transpose()?;
        let subtitle = self
            .subtitle
            .map(|value| normalize_required_metadata_option("subtitle", value))
            .transpose()?;
        let authors = self
            .authors
            .map(|value| normalize_required_text_list("authors", value))
            .transpose()?;
        let reviewers = self
            .reviewers
            .map(|value| normalize_required_text_list("reviewers", value))
            .transpose()?;
        let question_ids = self
            .question_ids
            .map(|value| normalize_required_question_ids("question_ids", value))
            .transpose()?;

        Ok(NormalizedPaperUpdate {
            description,
            title,
            subtitle,
            authors,
            reviewers,
            question_ids,
        })
    }
}

// ============================================================
// PapersParams 实现
// ============================================================
impl PapersParams {
    /// 获取规范化后的分页限制（默认 20，范围 1-100）
    pub(crate) fn normalized_limit(&self) -> i64 {
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    /// 获取规范化后的偏移量（默认 0，最小 0）
    pub(crate) fn normalized_offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

// ============================================================
// 规范化辅助函数
// ============================================================

/// 规范化必填的描述字段
fn normalize_required_description(field: &str, value: &str) -> Result<String> {
    normalize_bundle_description(field, value)
}

/// 规范化必填的纯文本字段（可为 None）
fn normalize_required_plaintext(field: &str, value: Option<String>) -> Result<String> {
    normalize_optional_bundle_description(field, value)
}

/// 规范化必填的元数据选项字段
fn normalize_required_metadata_option(field: &str, value: Option<String>) -> Result<String> {
    let Some(text) = value else {
        bail!("{field} must not be null");
    };
    normalize_required_metadata_text(field, &text)
}

/// 规范化必填的元数据文本字段
///
/// # 验证规则
/// - 修剪首尾空白
/// - 不能为空
/// - 不能包含控制字符
fn normalize_required_metadata_text(field: &str, value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        bail!("{field} must not be empty");
    }
    if normalized.chars().any(char::is_control) {
        bail!("{field} must not contain control characters");
    }
    Ok(normalized)
}

/// 规范化必填的文本列表字段
fn normalize_required_text_list(field: &str, value: Option<Vec<String>>) -> Result<Vec<String>> {
    let Some(items) = value else {
        bail!("{field} must not be null");
    };
    normalize_text_list(field, items)
}

/// 规范化文本列表
///
/// # 验证
/// - 每个元素修剪空白
/// - 去重
fn normalize_text_list(field: &str, values: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();

    for value in values {
        let item = normalize_required_metadata_text(field, &value)?;
        if seen.insert(item.clone()) {
            normalized.push(item);
        }
    }

    Ok(normalized)
}

/// 规范化必填的题目 ID 列表
fn normalize_required_question_ids(field: &str, value: Option<Vec<String>>) -> Result<Vec<String>> {
    let Some(items) = value else {
        bail!("{field} must not be null");
    };
    normalize_question_ids(items)
}

/// 规范化题目 ID 列表
///
/// # 验证规则
/// - 不能为空列表
/// - 每个 ID 修剪空白
/// - 不能有空字符串
/// - 去重
fn normalize_question_ids(question_ids: Vec<String>) -> Result<Vec<String>> {
    if question_ids.is_empty() {
        bail!("question_ids must not be empty");
    }

    let mut normalized = Vec::with_capacity(question_ids.len());
    let mut seen = HashSet::new();

    for question_id in question_ids {
        let candidate = question_id.trim().to_string();
        if candidate.is_empty() {
            bail!("question_ids must not contain empty strings");
        }
        if !seen.insert(candidate.clone()) {
            bail!("duplicate question_id in question_ids: {candidate}");
        }
        normalized.push(candidate);
    }

    Ok(normalized)
}

// ============================================================
// validate_paper_filters 函数
// ============================================================
/// 验证试卷查询参数
///
/// # 验证规则
/// - question_id 必须是合法 UUID
/// - category 必须是 none/T/E 之一
/// - q 不能为空字符串
pub(crate) fn validate_paper_filters(params: &PapersParams) -> Result<()> {
    if let Some(question_id) = &params.question_id {
        uuid::Uuid::parse_str(question_id)
            .map_err(|_| anyhow!("question_id must be a valid UUID"))?;
    }
    if let Some(category) = &params.category {
        validate_question_category(category)
            .map_err(|_| anyhow!("category must be one of: none, T, E"))?;
    }
    if let Some(q) = &params.q {
        if q.trim().is_empty() {
            bail!("q must not be empty");
        }
    }
    Ok(())
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. Option<Option<T>> 模式
 *    - 外层 Option：字段是否存在于请求中
 *    - 内层 Option：字段值是否为 null
 *    - 用于支持部分更新（PATCH）
 *
 * 2. .transpose() 用法
 *    - Option<Result<T>> → Result<Option<T>>
 *    - 将 Option 包裹的 Result 转换为 Result 包裹的 Option
 *
 * 3. 验证模式
 *    - normalize() 函数：验证 + 规范化
 *    - 返回 Result<规范化数据，错误>
 *    - 使用 bail!() 提前返回错误
 *
 * 4. HashSet 去重
 *    - seen.insert(item) 返回 bool
 *    - true 表示首次出现，false 表示重复
 *
 * ============================================================
 * 试卷数据模型关系
 * ============================================================
 *
 * 请求 → 规范化 → 验证 → 数据库操作 → 响应
 *
 * CreatePaperRequest → NormalizedCreatePaperRequest
 * UpdatePaperRequest → NormalizedPaperUpdate
 *
 * 响应类型:
 * - PaperSummary: 列表项
 * - PaperDetail: 完整详情
 * - PaperImportResponse: 导入结果
 * - PaperDeleteResponse: 删除结果
 *
 */
