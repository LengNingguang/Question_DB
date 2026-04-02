// ============================================================
// 文件：src/api/ops/models.rs
// 说明：运维操作的请求/响应模型
// ============================================================

//! 运维 API 的数据模型
//!
//! 定义导出、打包、质量检查的请求和响应结构

// 导入标准库的集合类型
use std::collections::HashSet;

// 导入 anyhow 错误处理库
use anyhow::{anyhow, bail, Result};

// 导入 Serde 序列化/反序列化 trait
use serde::{Deserialize, Serialize};

// 导入 UUID 库用于验证
use uuid::Uuid;

// ============================================================
// ExportRequest 结构体
// ============================================================
/// 导出请求参数
///
/// # 字段
/// - format: 导出格式 (Jsonl 或 Csv)
/// - public: 是否公开（控制是否包含 TeX 源码）
/// - output_path: 输出文件路径（可选，使用默认值）
#[derive(Debug, Deserialize)]
pub(crate) struct ExportRequest {
    // 导出格式
    pub(crate) format: ExportFormat,

    // 是否公开
    // public=true 时不包含 TeX 源码
    #[serde(default)]
    pub(crate) public: bool,

    // 输出路径（可选）
    // None 时使用默认路径
    pub(crate) output_path: Option<String>,
}

// ============================================================
// ExportFormat 枚举
// ============================================================
/// 导出格式
///
/// # 变体
/// - Jsonl: JSON Lines 格式，每行一个 JSON 对象
/// - Csv: CSV 格式，适合 Excel 打开
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ExportFormat {
    Jsonl,
    Csv,
}

// ============================================================
// QualityCheckRequest 结构体
// ============================================================
/// 质量检查请求参数
///
/// # 字段
/// - output_path: 报告输出路径（可选）
#[derive(Debug, Deserialize)]
pub(crate) struct QualityCheckRequest {
    pub(crate) output_path: Option<String>,
}

// ============================================================
// ExportResponse 结构体
// ============================================================
/// 导出响应
///
/// # 字段
/// - format: 实际使用的格式
/// - public: 是否公开
/// - output_path: 输出文件路径
/// - exported_questions: 导出的题目数量
#[derive(Debug, Serialize)]
pub(crate) struct ExportResponse {
    pub(crate) format: &'static str,
    pub(crate) public: bool,
    pub(crate) output_path: String,
    pub(crate) exported_questions: usize,
}

// ============================================================
// QuestionBundleRequest 结构体
// ============================================================
/// 题目打包请求参数
///
/// # 字段
/// - question_ids: 题目 ID 列表
#[derive(Debug, Deserialize)]
pub(crate) struct QuestionBundleRequest {
    pub(crate) question_ids: Vec<String>,
}

// ============================================================
// PaperBundleRequest 结构体
// ============================================================
/// 试卷打包请求参数
///
/// # 字段
/// - paper_ids: 试卷 ID 列表
#[derive(Debug, Deserialize)]
pub(crate) struct PaperBundleRequest {
    pub(crate) paper_ids: Vec<String>,
}

// ============================================================
// QuestionBundleRequest 实现
// ============================================================
impl QuestionBundleRequest {
    /// 规范化并验证请求
    ///
    /// # 返回
    /// - Ok(Vec<String>): 验证通过的 ID 列表
    /// - Err: 验证失败
    pub(crate) fn normalize(self) -> Result<Vec<String>> {
        normalize_ids("question_ids", self.question_ids)
    }
}

// ============================================================
// PaperBundleRequest 实现
// ============================================================
impl PaperBundleRequest {
    /// 规范化并验证请求
    pub(crate) fn normalize(self) -> Result<Vec<String>> {
        normalize_ids("paper_ids", self.paper_ids)
    }
}

// ============================================================
// normalize_ids 函数
// ============================================================
/// 规范化并验证 ID 列表
///
/// # 参数
/// - field_name: 字段名称（用于错误消息）
/// - ids: ID 字符串列表
///
/// # 验证规则
/// 1. 列表不能为空
/// 2. 每个 ID 不能为空字符串
/// 3. 每个 ID 必须是合法的 UUID
/// 4. 不能有重复的 ID
///
/// # 返回
/// - Ok(Vec<String>): 修剪后的 ID 列表
/// - Err: 验证失败原因
fn normalize_ids(field_name: &str, ids: Vec<String>) -> Result<Vec<String>> {
    // 检查是否为空列表
    if ids.is_empty() {
        return Err(anyhow!("{field_name} must not be empty"));
    }

    // 预分配容量，避免重新分配
    let mut normalized = Vec::with_capacity(ids.len());

    // 用于检测重复的 HashSet
    let mut seen = HashSet::new();

    // 遍历每个 ID
    for raw_id in ids {
        // 修剪首尾空白
        let id = raw_id.trim().to_string();

        // 检查是否为空字符串
        if id.is_empty() {
            bail!("{field_name} must not contain empty values");
        }

        // 验证 UUID 格式
        // Uuid::parse_str() 会验证格式是否正确
        Uuid::parse_str(&id).map_err(|_| anyhow!("invalid {field_name} entry: {id}"))?;

        // 检查是否重复
        if !seen.insert(id.clone()) {
            bail!("duplicate {field_name} entry: {id}");
        }

        // 添加到结果列表
        normalized.push(id);
    }

    // 返回验证通过的列表
    Ok(normalized)
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. Serde 属性
 *    #[serde(default)] - 字段缺失时使用 Default::default()
 *    #[serde(rename_all = "lowercase")] - 将枚举变体重命名为小写
 *
 * 2. &'static str
 *    - 生命周期为 'static 的字符串引用
 *    - 通常用于字符串字面量
 *    - 不需要分配，直接使用
 *
 * 3. HashSet 用于去重
 *    - insert() 返回 bool，表示是否插入成功
 *    - 如果元素已存在，返回 false
 *
 * 4. anyhow 错误处理
 *    anyhow!() - 创建错误
 *    bail!() - 立即返回错误
 *    ? - 传播错误
 *
 * 5. 验证函数设计
 *    - 输入：原始数据
 *    - 处理：验证 + 规范化
 *    - 输出：Result<规范化数据，错误>
 *
 * ============================================================
 * UUID 格式示例
 * ============================================================
 *
 * 合法 UUID:
 * "550e8400-e29b-41d4-a716-446655440000"
 * "550e8400e29b41d4a716446655440000" (无连字符)
 *
 * 非法 UUID:
 * "not-a-uuid"
 * "550e8400-e29b" (太短)
 * "" (空字符串)
 *
 */
