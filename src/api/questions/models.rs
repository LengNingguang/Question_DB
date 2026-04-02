// ============================================================
// 文件：src/api/questions/models.rs
// 说明：题目数据模型定义
// ============================================================

//! 题目模块的数据模型
//!
//! 定义题目相关的结构体、验证函数和规范化逻辑

// 导入标准库集合类型
use std::collections::{BTreeMap, HashSet};

// 导入 anyhow 错误处理
use anyhow::{anyhow, bail, Result};

// 导入 Serde 序列化/反序列化
use serde::{Deserialize, Serialize};

// 导入共享工具函数
use crate::api::shared::utils::normalize_optional_bundle_description;


// ============================================================
// 常量定义
// ============================================================

/// 允许的题目分类（3 种）
/// - "none": 未分类
/// - "T": Theory (理论题)
/// - "E": Experiment (实验题)
pub(crate) const QUESTION_CATEGORIES: [&str; 3] = ["none", "T", "E"];

/// 允许的题目状态（3 种）
/// - "none": 未审核
/// - "reviewed": 已审核
/// - "used": 已使用
pub(crate) const QUESTION_STATUSES: [&str; 3] = ["none", "reviewed", "used"];


// ============================================================
// QuestionSourceRef 结构体
// ============================================================
/// 题目源码引用
///
/// 记录 TeX 源文件的路径信息
#[derive(Debug, Serialize)]
pub struct QuestionSourceRef {
    /// TeX 源文件路径
    pub(crate) tex: String,
}


// ============================================================
// QuestionDifficulty 结构体
// ============================================================
/// 题目难度定义
///
/// 使用 BTreeMap 存储多个来源的难度评估
/// 来源可以是：human, heuristic, ml, symbolic, simulator 等
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuestionDifficulty {
    /// 扁平化序列化：每个难度来源作为独立字段
    #[serde(flatten)]
    pub(crate) entries: BTreeMap<String, QuestionDifficultyValue>,
}


// ============================================================
// QuestionDifficultyValue 结构体
// ============================================================
/// 单个难度评估值
///
/// 包含分数和可选备注
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]  // 拒绝未知字段，严格的 JSON 解析
pub struct QuestionDifficultyValue {
    /// 难度分数（1-10）
    pub(crate) score: i32,

    /// 可选备注（序列化时如果为 None 则跳过）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) notes: Option<String>,
}


// ============================================================
// QuestionAssetRef 结构体
// ============================================================
/// 题目资源文件引用
///
/// 记录图片、数据文件等资源的元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAssetRef {
    /// 资源文件路径（如 assets/figure1.png）
    pub(crate) path: String,

    /// 文件类型（如 "asset"）
    pub(crate) file_kind: String,

    /// 对象 UUID（指向 objects 表）
    pub(crate) object_id: String,

    /// MIME 类型（可选，如 "image/png"）
    pub(crate) mime_type: Option<String>,
}


// ============================================================
// QuestionSummary 结构体
// ============================================================
/// 题目摘要信息
///
/// 用于列表接口，不包含完整的资源和关联试卷信息
#[derive(Debug, Serialize)]
pub struct QuestionSummary {
    /// 题目 UUID
    pub(crate) question_id: String,

    /// 源码引用
    pub(crate) source: QuestionSourceRef,

    /// 分类（none/T/E）
    pub(crate) category: String,

    /// 状态（none/reviewed/used）
    pub(crate) status: String,

    /// 描述文本
    pub(crate) description: String,

    /// 标签列表
    pub(crate) tags: Vec<String>,

    /// 难度定义
    pub(crate) difficulty: QuestionDifficulty,

    /// 创建时间（ISO 8601 格式）
    pub(crate) created_at: String,

    /// 更新时间（ISO 8601 格式）
    pub(crate) updated_at: String,
}


// ============================================================
// QuestionPaperRef 结构体
// ============================================================
/// 题目所属试卷引用
///
/// 记录题目被哪些试卷使用
#[derive(Debug, Serialize)]
pub struct QuestionPaperRef {
    /// 试卷 UUID
    pub(crate) paper_id: String,

    /// 试卷描述
    pub(crate) description: String,

    /// 试卷标题
    pub(crate) title: String,

    /// 试卷副标题
    pub(crate) subtitle: String,

    /// 排序顺序（题目在试卷中的序号）
    pub(crate) sort_order: i32,
}


// ============================================================
// QuestionDetail 结构体
// ============================================================
/// 题目详细信息
///
/// 用于详情接口，包含完整的资源和关联试卷信息
#[derive(Debug, Serialize)]
pub struct QuestionDetail {
    /// 题目 UUID
    pub(crate) question_id: String,

    /// TeX 对象 UUID（指向 objects 表）
    pub(crate) tex_object_id: String,

    /// 源码引用
    pub(crate) source: QuestionSourceRef,

    /// 分类
    pub(crate) category: String,

    /// 状态
    pub(crate) status: String,

    /// 描述
    pub(crate) description: String,

    /// 标签列表
    pub(crate) tags: Vec<String>,

    /// 难度定义
    pub(crate) difficulty: QuestionDifficulty,

    /// 创建时间
    pub(crate) created_at: String,

    /// 更新时间
    pub(crate) updated_at: String,

    /// 资源文件列表（图片、数据等）
    pub(crate) assets: Vec<QuestionAssetRef>,

    /// 所属试卷列表
    pub(crate) papers: Vec<QuestionPaperRef>,
}


// ============================================================
// QuestionsParams 结构体
// ============================================================
/// 题目查询参数
///
/// 用于解析 GET /questions 的查询字符串
#[derive(Debug, Deserialize)]
pub(crate) struct QuestionsParams {
    /// 按试卷 ID 过滤
    pub(crate) paper_id: Option<String>,

    /// 按分类过滤（T/E/none）
    pub(crate) category: Option<String>,

    /// 按标签过滤
    pub(crate) tag: Option<String>,

    /// 按难度标签过滤（human/heuristic/ml 等）
    pub(crate) difficulty_tag: Option<String>,

    /// 最小难度分数
    pub(crate) difficulty_min: Option<i32>,

    /// 最大难度分数
    pub(crate) difficulty_max: Option<i32>,

    /// 搜索关键词（全文搜索）
    pub(crate) q: Option<String>,

    /// 返回数量限制（默认 20，最大 100）
    pub(crate) limit: Option<i64>,

    /// 偏移量（用于分页）
    pub(crate) offset: Option<i64>,
}


// ============================================================
// UpdateQuestionMetadataRequest 结构体
// ============================================================
/// 更新题目元数据的请求体
///
/// 用于 PATCH /questions/{id} 请求
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]  // 拒绝未知字段
pub(crate) struct UpdateQuestionMetadataRequest {
    /// 分类（可选）
    #[serde(default)]
    pub(crate) category: Option<String>,

    /// 描述（可选，Option<String>表示可以为 null 来清除描述）
    #[serde(default)]
    pub(crate) description: Option<Option<String>>,

    /// 标签列表（可选）
    #[serde(default)]
    pub(crate) tags: Option<Vec<String>>,

    /// 状态（可选）
    #[serde(default)]
    pub(crate) status: Option<String>,

    /// 难度定义（可选）
    #[serde(default)]
    pub(crate) difficulty: Option<QuestionDifficulty>,
}


// ============================================================
// NormalizedQuestionMetadataUpdate 结构体
// ============================================================
/// 规范化后的题目元数据更新
///
/// 经过验证和清理后的数据结构
#[derive(Debug)]
pub(crate) struct NormalizedQuestionMetadataUpdate {
    /// 分类（已验证）
    pub(crate) category: Option<String>,

    /// 描述（已去除首尾空格）
    pub(crate) description: Option<String>,

    /// 标签列表（已去重、验证）
    pub(crate) tags: Option<Vec<String>>,

    /// 状态（已验证）
    pub(crate) status: Option<String>,

    /// 难度定义（已验证和规范化）
    pub(crate) difficulty: Option<NormalizedQuestionDifficulty>,
}


// ============================================================
// NormalizedQuestionDifficultyValue 结构体
// ============================================================
/// 规范化后的单个难度值
#[derive(Debug, Clone)]
pub(crate) struct NormalizedQuestionDifficultyValue {
    /// 难度分数（1-10）
    pub(crate) score: i32,

    /// 备注（已去除首尾空格，空字符串转为 None）
    pub(crate) notes: Option<String>,
}


// ============================================================
// 类型别名
// ============================================================
/// 规范化后的难度定义类型
/// BTreeMap 保持键的有序性
pub(crate) type NormalizedQuestionDifficulty = BTreeMap<String, NormalizedQuestionDifficultyValue>;


// ============================================================
// QuestionImportResponse 结构体
// ============================================================
/// 题目导入响应
///
/// POST /questions 成功后的返回值
#[derive(Debug, Serialize)]
pub(crate) struct QuestionImportResponse {
    /// 新创建的题目 UUID
    pub(crate) question_id: String,

    /// 上传的文件名
    pub(crate) file_name: String,

    /// 导入的资源文件数量
    pub(crate) imported_assets: usize,

    /// 状态（固定为"imported"）
    pub(crate) status: &'static str,
}


// ============================================================
// QuestionFileReplaceResponse 结构体
// ============================================================
/// 题目文件替换响应
///
/// PUT /questions/{id}/file 成功后的返回值
#[derive(Debug, Serialize)]
pub(crate) struct QuestionFileReplaceResponse {
    /// 题目 UUID
    pub(crate) question_id: String,

    /// 上传的文件名
    pub(crate) file_name: String,

    /// TeX 源文件路径
    pub(crate) source_tex_path: String,

    /// 导入的资源文件数量
    pub(crate) imported_assets: usize,

    /// 状态（固定为"replaced"）
    pub(crate) status: &'static str,
}


// ============================================================
// QuestionDeleteResponse 结构体
// ============================================================
/// 题目删除响应
///
/// DELETE /questions/{id} 成功后的返回值
#[derive(Debug, Serialize)]
pub(crate) struct QuestionDeleteResponse {
    /// 删除的题目 UUID
    pub(crate) question_id: String,

    /// 状态（固定为"deleted"）
    pub(crate) status: &'static str,
}


// ============================================================
// validate_question_category 函数
// ============================================================
/// 验证题目分类是否合法
///
/// # 参数
/// - category: 分类字符串
///
/// # 返回
/// - Ok(()): 分类合法
/// - Err: 分类不合法
///
/// # 允许的值
/// - "none": 未分类
/// - "T": 理论题
/// - "E": 实验题
pub(crate) fn validate_question_category(category: &str) -> Result<()> {
    // 检查是否在允许的列表中
    if !QUESTION_CATEGORIES.contains(&category) {
        bail!("category must be one of: none, T, E");
    }
    Ok(())
}


// ============================================================
// validate_question_status 函数
// ============================================================
/// 验证题目状态是否合法
///
/// # 参数
/// - status: 状态字符串
///
/// # 返回
/// - Ok(()): 状态合法
/// - Err: 状态不合法
///
/// # 允许的值
/// - "none": 未审核
/// - "reviewed": 已审核
/// - "used": 已使用
pub(crate) fn validate_question_status(status: &str) -> Result<()> {
    // 检查是否在允许的列表中
    if !QUESTION_STATUSES.contains(&status) {
        bail!("status must be one of: none, reviewed, used");
    }
    Ok(())
}


// ============================================================
// UpdateQuestionMetadataRequest 实现
// ============================================================
impl UpdateQuestionMetadataRequest {
    /// 规范化请求数据
    ///
    /// 执行以下操作：
    /// 1. 验证至少有一个字段被提供
    /// 2. 对每个字段进行验证和规范化
    /// 3. 返回规范化后的结构体
    ///
    /// # 返回
    /// - Ok(NormalizedQuestionMetadataUpdate): 规范化成功
    /// - Err: 验证失败
    pub(crate) fn normalize(self) -> Result<NormalizedQuestionMetadataUpdate> {
        // 验证：至少有一个字段被提供
        if self.category.is_none()
            && self.description.is_none()
            && self.tags.is_none()
            && self.status.is_none()
            && self.difficulty.is_none()
        {
            return Err(anyhow!(
                "request body must include at least one of: category, description, tags, status, difficulty"
            ));
        }

        // 规范化分类
        let category = self
            .category
            .map(|value| normalize_category(&value))
            .transpose()?;

        // 规范化描述
        let description = self
            .description
            .map(|value| normalize_required_plaintext("description", value))
            .transpose()?;

        // 规范化标签
        let tags = self.tags.map(normalize_tags).transpose()?;

        // 规范化状态
        let status = self
            .status
            .map(|value| normalize_status(&value))
            .transpose()?;

        // 规范化难度
        let difficulty = self
            .difficulty
            .map(QuestionDifficulty::normalize)
            .transpose()?;

        // 构建并返回规范化后的结构体
        Ok(NormalizedQuestionMetadataUpdate {
            category,
            description,
            tags,
            status,
            difficulty,
        })
    }
}


// ============================================================
// QuestionDifficulty 实现
// ============================================================
impl QuestionDifficulty {
    /// 规范化难度定义
    ///
    /// 调用 normalize_difficulty_entries 进行实际处理
    pub(crate) fn normalize(self) -> Result<NormalizedQuestionDifficulty> {
        normalize_difficulty_entries(self.entries)
    }
}


// ============================================================
// normalize_category 函数
// ============================================================
/// 规范化分类字符串
///
/// 1. 去除首尾空格
/// 2. 验证是否为允许的值
fn normalize_category(value: &str) -> Result<String> {
    // 去除首尾空格
    let normalized = value.trim().to_string();
    // 验证
    validate_question_category(&normalized)?;
    Ok(normalized)
}


// ============================================================
// normalize_status 函数
// ============================================================
/// 规范化状态字符串
///
/// 1. 去除首尾空格
/// 2. 验证是否为允许的值
fn normalize_status(value: &str) -> Result<String> {
    // 去除首尾空格
    let normalized = value.trim().to_string();
    // 验证
    validate_question_status(&normalized)?;
    Ok(normalized)
}


// ============================================================
// normalize_required_plaintext 函数
// ============================================================
/// 规范化必需的纯文本字段
///
/// 用于 description 等不能为空的字段
///
/// # 参数
/// - field: 字段名（用于错误消息）
/// - value: 可选的字符串值（None 表示清除字段）
fn normalize_required_plaintext(field: &str, value: Option<String>) -> Result<String> {
    // 复用共享工具函数
    normalize_optional_bundle_description(field, value)
}


// ============================================================
// normalize_optional_plaintext 函数
// ============================================================
/// 规范化可选的纯文本字段
///
/// 1. 去除首尾空格
/// 2. 空字符串转换为 None
fn normalize_optional_plaintext(value: String) -> Option<String> {
    // 去除首尾空格
    let trimmed = value.trim().to_string();

    // 空字符串转换为 None
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}


// ============================================================
// normalize_tags 函数
// ============================================================
/// 规范化标签列表
///
/// 1. 去除每个标签的首尾空格
/// 2. 拒绝空字符串
/// 3. 去重（保持首次出现的顺序）
///
/// # 参数
/// - values: 原始标签列表
///
/// # 返回
/// - Ok(Vec<String>): 规范化后的标签列表
/// - Err: 包含空字符串
fn normalize_tags(values: Vec<String>) -> Result<Vec<String>> {
    // 预分配容量
    let mut normalized = Vec::with_capacity(values.len());

    // 使用 HashSet 跟踪已见过的标签
    let mut seen = HashSet::new();

    // 遍历每个标签
    for value in values {
        // 去除首尾空格
        let tag = value.trim().to_string();

        // 拒绝空字符串
        if tag.is_empty() {
            bail!("tags must not contain empty strings");
        }

        // 去重：如果没见过则添加
        if seen.insert(tag.clone()) {
            normalized.push(tag);
        }
    }

    Ok(normalized)
}


// ============================================================
// normalize_difficulty_entries 函数
// ============================================================
/// 规范化难度条目
///
/// 1. 去除每个键的首尾空格
/// 2. 验证分数在 1-10 范围内
/// 3. 规范化备注（空字符串转 None）
/// 4. 去重
/// 5. 验证必须包含"human"评估
///
/// # 参数
/// - values: 原始难度条目 BTreeMap
///
/// # 返回
/// - Ok(NormalizedQuestionDifficulty): 规范化后的难度定义
/// - Err: 验证失败
fn normalize_difficulty_entries(
    values: BTreeMap<String, QuestionDifficultyValue>,
) -> Result<NormalizedQuestionDifficulty> {
    // 创建新的 BTreeMap 存储规范化结果
    let mut normalized = BTreeMap::new();

    // 遍历每个难度条目
    for (name, value) in values {
        // 去除键的首尾空格
        let tag = name.trim().to_string();

        // 拒绝空键
        if tag.is_empty() {
            bail!("difficulty keys must not be empty");
        }

        // 验证分数范围（1-10）
        if !(1..=10).contains(&value.score) {
            bail!("difficulty.{tag}.score must be between 1 and 10");
        }

        // 规范化备注
        let notes = value.notes.and_then(normalize_optional_plaintext);

        // 插入到规范化 Map，检查是否重复
        if normalized
            .insert(
                tag.clone(),
                NormalizedQuestionDifficultyValue {
                    score: value.score,
                    notes,
                },
            )
            .is_some()
        {
            bail!("difficulty tags must be unique after trimming");
        }
    }

    // 验证必须包含"human"评估
    if !normalized.contains_key("human") {
        bail!("difficulty must include a human entry");
    }

    Ok(normalized)
}


// ============================================================
// 单元测试模块
// ============================================================
#[cfg(test)]
mod tests {
    use super::UpdateQuestionMetadataRequest;

    // --------------------------------------------------------
    // 测试：标签规范化和去重
    // --------------------------------------------------------
    #[test]
    fn update_request_normalizes_and_deduplicates_tags() {
        // 构建测试请求
        let request = UpdateQuestionMetadataRequest {
            category: Some(" T ".into()),  // 包含空格
            description: Some(Some("  demo note  ".into())),  // 包含空格
            tags: Some(vec![" optics ".into(), "mechanics".into(), "optics".into()]),  // 有重复
            status: Some(" reviewed ".into()),  // 包含空格
            difficulty: None,
        };

        // 规范化
        let normalized = request.normalize().expect("request should normalize");

        // 验证分类被去除空格
        assert_eq!(normalized.category.as_deref(), Some("T"));

        // 验证描述被去除空格
        assert_eq!(normalized.description.as_deref(), Some("demo note"));

        // 验证标签去重（保留首次出现）
        assert_eq!(
            normalized.tags.expect("tags should be present"),
            vec!["optics".to_string(), "mechanics".to_string()]
        );

        // 验证状态被去除空格
        assert_eq!(normalized.status.as_deref(), Some("reviewed"));
    }

    // --------------------------------------------------------
    // 测试：必须有 human 难度评估
    // --------------------------------------------------------
    #[test]
    fn update_request_requires_human_difficulty() {
        // 解析 JSON，只有 ml 评估，缺少 human
        let request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"difficulty":{"ml":{"score":8}}}"#)
                .expect("json should parse");

        // 规范化应该失败
        assert!(request.normalize().is_err());
    }

    // --------------------------------------------------------
    // 测试：难度备注规范化
    // --------------------------------------------------------
    #[test]
    fn update_request_normalizes_difficulty_notes() {
        // 解析 JSON，包含空格和空备注
        let request: UpdateQuestionMetadataRequest = serde_json::from_str(
            r#"{
                "difficulty":{
                    " human ":{"score":7,"notes":"  calibrated  "},
                    "heuristic":{"score":5,"notes":"   "}
                }
            }"#,
        )
        .expect("json should parse");

        // 规范化
        let normalized = request.normalize().expect("request should normalize");
        let difficulty = normalized.difficulty.expect("difficulty update");

        // 验证 human 难度
        assert_eq!(difficulty["human"].score, 7);
        assert_eq!(difficulty["human"].notes.as_deref(), Some("calibrated"));

        // 验证 heuristic 难度（空备注转为 None）
        assert_eq!(difficulty["heuristic"].score, 5);
        assert_eq!(difficulty["heuristic"].notes, None);
    }

    // --------------------------------------------------------
    // 测试：描述不能为空或 null
    // --------------------------------------------------------
    #[test]
    fn update_request_rejects_empty_or_null_description() {
        // 空字符串描述
        let empty_request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"description":""}"#).expect("json should parse");

        // null 描述
        let null_request: UpdateQuestionMetadataRequest =
            serde_json::from_str(r#"{"description":null}"#).expect("json should parse");

        // 两者都应该失败
        assert!(empty_request.normalize().is_err());
        assert!(null_request.normalize().is_err());
    }
}


/*
============================================================
知识点讲解 (Rust 新手必读)
============================================================

1. #[serde(flatten)] 属性
   - 将嵌套结构扁平化序列化
   - QuestionDifficulty { entries: {"human": {...}} }
   - 序列化为：{"human": {...}} 而不是 {"entries": {"human": {...}}}

2. #[serde(deny_unknown_fields)]
   - 拒绝 JSON 中包含未知字段
   - 提高 API 的健壮性
   - 防止拼写错误导致的问题

3. #[serde(default, skip_serializing_if = "Option::is_none")]
   - default: 反序列化时缺失字段使用默认值（None）
   - skip_serializing_if: 序列化时 None 值不输出
   - 减少 JSON 体积

4. BTreeMap vs HashMap
   - BTreeMap: 有序（按键排序），适合需要稳定输出的场景
   - HashMap: 无序，更快
   - 这里选择 BTreeMap 确保 JSON 字段顺序一致

5. transpose() 的使用
   - 将 Option<Result<T, E>> 转换为 Result<Option<T>, E>
   - 用于链式调用中的错误传播

6. .and_then() 用于 Option
   - Some(x) => f(x)
   - None => None
   - 用于条件处理

============================================================
数据结构关系图
============================================================

QuestionDetail (完整详情)
├── QuestionSourceRef (源码引用)
├── QuestionDifficulty (难度定义)
│   └── BTreeMap<String, QuestionDifficultyValue>
├── Vec<QuestionAssetRef> (资源列表)
└── Vec<QuestionPaperRef> (试卷引用)

QuestionSummary (列表摘要)
├── 不包含 assets
└── 不包含 papers

============================================================
验证规则总结
============================================================

1. 分类验证:
   - 必须是：none, T, E
   - 自动去除首尾空格

2. 状态验证:
   - 必须是：none, reviewed, used
   - 自动去除首尾空格

3. 标签验证:
   - 不能有空字符串
   - 自动去重
   - 去除首尾空格

4. 难度验证:
   - 分数必须在 1-10 范围内
   - 必须包含"human"评估
   - 键不能为空
   - 键去重（去除空格后）
   - 备注空字符串转为 None

5. 描述验证:
   - 不能为空字符串
   - 可以为 null（表示清除描述）
   - 去除首尾空格

============================================================
测试覆盖
============================================================

| 测试场景                   | 覆盖状态 |
|----------------------------|----------|
| 标签规范化和去重           | ✅       |
| 必须有 human 难度          | ✅       |
| 难度备注规范化             | ✅       |
| 描述不能为空或 null        | ✅       |

============================================================
设计决策说明
============================================================

1. 为什么 difficulty 必须是 BTreeMap?
   - 确保 JSON 序列化时字段顺序一致
   - 便于测试和调试
   - 性能差异对于小数据量可忽略

2. 为什么 tags 要自动去重?
   - 防止用户意外提交重复标签
   - 减少数据库存储冗余
   - 提高查询效率

3. 为什么必须有 human 评估?
   - 人工评估是基准
   - 其他评估来源（ml、heuristic）可选
   - 确保题目有可靠的难度参考
*/
