// ============================================================
// 文件：src/api/shared/utils.rs
// 说明：共享工具函数
// ============================================================

//! 共享的低层级文件系统工具函数
//!
//! 提供路径处理、文件名验证等通用功能

// 导入标准库的路径和环境模块
use std::{
    env,
    path::{Path, PathBuf},
};

// 导入 anyhow 错误处理库
use anyhow::{bail, Result};

// ============================================================
// expand_path 函数
// ============================================================
/// 展开路径中的波浪号 (~)
///
/// # 参数
/// - input: 输入路径字符串
///
/// # 返回
/// 展开后的 PathBuf
///
/// # 示例
/// - "~/"home" → "/home/用户名/home"
/// - "~" → "/home/用户名"
/// - "/absolute/path" → "/absolute/path" (不变)
pub(crate) fn expand_path(input: &str) -> PathBuf {
    // 处理单独的 "~"
    if input == "~" {
        // 从环境变量获取用户主目录
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }

    // 处理 "~/" 开头的路径
    if let Some(stripped) = input.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            // 将主目录和剩余路径组合
            return PathBuf::from(home).join(stripped);
        }
    }

    // 其他情况直接转换为 PathBuf
    PathBuf::from(input)
}

// ============================================================
// canonical_or_original 函数
// ============================================================
/// 获取路径的规范形式，失败则返回原始路径
///
/// # 参数
/// - path: 输入路径
///
/// # 返回
/// 规范化的路径字符串
///
/// # 说明
/// canonicalize() 会解析符号链接和相对路径
/// 但可能失败 (如路径不存在)，此时返回原始路径
pub(crate) fn canonical_or_original(path: &Path) -> String {
    path.canonicalize()  // 尝试规范化路径
        // 如果失败，使用原始路径
        .unwrap_or_else(|_| path.to_path_buf())
        // 转换为字符串
        .to_string_lossy()
        .to_string()
}

// ============================================================
// normalize_bundle_description 函数
// ============================================================
/// 规范化打包描述字段
///
/// # 参数
/// - field: 字段名称 (用于错误消息)
/// - value: 输入值
///
/// # 返回
/// 修剪后的描述字符串
///
/// # 验证规则
/// - 不能为空
/// - 不能是 "." 或 ".."
/// - 不能以 "." 结尾
/// - 不能超过 80 字符
/// - 不能包含文件名非法字符
pub(crate) fn normalize_bundle_description(field: &str, value: &str) -> Result<String> {
    // 修剪首尾空白
    let normalized = value.trim().to_string();

    // 验证规范化后的值
    validate_bundle_description(field, &normalized)?;

    // 返回结果
    Ok(normalized)
}

// ============================================================
// normalize_optional_bundle_description 函数
// ============================================================
/// 规范化可选的打包描述字段
///
/// # 参数
/// - field: 字段名称
/// - value: 可选的输入值
///
/// # 返回
/// 规范化后的字符串
///
/// # 错误处理
/// 如果值为 None，返回错误
pub(crate) fn normalize_optional_bundle_description(
    field: &str,
    value: Option<String>,
) -> Result<String> {
    // 检查是否为 None
    let Some(text) = value else {
        // bail! 宏立即返回错误
        bail!("{field} must not be null");
    };

    // 调用普通规范化函数
    normalize_bundle_description(field, &text)
}

// ============================================================
// bundle_directory_name 函数
// ============================================================
/// 生成打包目录名称
///
/// # 参数
/// - description: 描述文本
/// - id: UUID 字符串
///
/// # 返回
/// 格式为 "{description}_{id 前 6 位}" 的字符串
///
/// # 示例
/// ("热学决赛", "550e8400-e29b-...") → "热学决赛_550e84"
pub(crate) fn bundle_directory_name(description: &str, id: &str) -> String {
    // 从 UUID 中提取 6 个字符 (去掉连字符)
    let suffix = id
        .chars()  // 遍历字符
        .filter(|ch| *ch != '-')  // 过滤掉 '-'
        .take(6)  // 取前 6 个
        .collect::<String>();  // 收集为 String

    // 格式化输出
    format!("{description}_{suffix}")
}

// ============================================================
// validate_bundle_description 函数
// ============================================================
/// 验证打包描述字段
///
/// # 参数
/// - field: 字段名称 (用于错误消息)
/// - value: 要验证的值
///
/// # 返回
/// - Ok(()): 验证通过
/// - Err: 验证失败，包含错误描述
fn validate_bundle_description(field: &str, value: &str) -> Result<()> {
    // 检查是否为空
    if value.is_empty() {
        bail!("{field} must not be empty");
    }

    // 检查是否为 "." 或 ".."
    // 这些是特殊的目录名，不允许使用
    if value == "." || value == ".." {
        bail!("{field} must not be '.' or '..'");
    }

    // 检查是否以 '.' 结尾
    // 某些系统对以 '.' 结尾的名称有特殊处理
    if value.ends_with('.') {
        bail!("{field} must not end with '.'");
    }

    // 检查长度限制 (80 字符)
    // 限制长度避免文件名过长
    if value.chars().count() > 80 {
        bail!("{field} must be at most 80 characters");
    }

    // 遍历每个字符进行检查
    for ch in value.chars() {
        // 检查控制字符 (不可打印字符)
        if ch.is_control() {
            bail!("{field} must not contain control characters");
        }

        // 检查文件名非法字符
        // 这些字符在 Windows/Unix 上有特殊含义
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            bail!("{field} contains an invalid filename character: {ch}");
        }
    }

    // 所有检查通过
    Ok(())
}

// ============================================================
// 单元测试模块
// ============================================================
#[cfg(test)]
mod tests {
    // 导入要测试的函数
    use super::{bundle_directory_name, normalize_bundle_description};

    // ========================================================
    // 测试：接受中文描述
    // ========================================================
    #[test]
    fn normalize_bundle_description_accepts_chinese() {
        // 测试中文描述的规范化 (包含首尾空格)
        let normalized =
            normalize_bundle_description("description", "  热学 决赛卷 A  ")
                .expect("valid");  // 期望成功

        // 验证结果：修剪了空格
        assert_eq!(normalized, "热学 决赛卷 A");
    }

    // ========================================================
    // 测试：拒绝非法字符
    // ========================================================
    #[test]
    fn normalize_bundle_description_rejects_invalid_filename_chars() {
        // 测试包含 '/' 的非法描述
        let err = normalize_bundle_description("description", "bad/name")
            .expect_err("should fail");  // 期望失败

        // 验证错误消息包含预期内容
        assert!(err.to_string().contains("invalid filename character"));
    }

    // ========================================================
    // 测试：目录名称生成
    // ========================================================
    #[test]
    fn bundle_directory_name_appends_id_suffix() {
        // 测试目录名生成 (使用示例 UUID)
        let directory = bundle_directory_name(
            "热学决赛卷",
            "550e8400-e29b-41d4-a716-446655440000"
        );

        // 验证结果：取 UUID 前 6 位 (不含 '-')
        assert_eq!(directory, "热学决赛卷_550e84");
    }
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. PathBuf vs &Path
 *    - PathBuf: 拥有的路径类型 (类似 String)
 *    - &Path: 路径的借用视图 (类似 &str)
 *    - 函数参数用 &Path，返回值用 PathBuf
 *
 * 2. bail! 宏
 *    - 立即返回错误
 *    - 等同于 return Err(anyhow!(...))
 *    - 简化错误返回
 *
 * 3. if let Some(x) = ...
 *    - 模式匹配语法
 *    - 如果匹配 Some，提取内部值
 *    - 处理 Option 的常用方式
 *
 * 4. .filter().take().collect()
 *    - 迭代器适配器链
 *    - filter: 过滤元素
 *    - take: 取前 N 个
 *    - collect: 收集为集合
 *
 * 5. matches! 宏
 *    - 模式匹配的简洁写法
 *    - 返回布尔值
 *    - 适合多值匹配
 *
 * ============================================================
 * 文件名验证规则
 * ============================================================
 *
 * 合法: "热学决赛卷", "demo-paper-1"
 * 非法: "", ".", "..", "bad/name", "file.txt."
 * 非法: "a".repeat(81), "file\x00" (控制字符)
 *
 * 非法字符列表：/ \ : * ? " < > |
 * (这些字符在文件系统中有特殊含义)
 *
 */
