// ============================================================
// 文件：src/api/ops/paper_render.rs
// 说明：LaTeX 试卷渲染引擎
// ============================================================

//! 试卷 LaTeX 渲染功能
//!
//! 将题目 TeX 内容注入模板，生成完整的试卷文档

// 导入标准库类型
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::LazyLock,
};

// 导入 anyhow 错误处理
use anyhow::{bail, Context, Result};

// 导入正则表达式库
use regex::{Captures, Regex};

// ============================================================
// 模板路径常量
// ============================================================
/// 理论卷模板路径
const THEORY_TEMPLATE_PATH: &str = "CPHOS-Latex/theory/examples/example-paper.tex";
/// 实验卷模板路径
const EXPERIMENT_TEMPLATE_PATH: &str = "CPHOS-Latex/experiment/examples/example-paper.tex";

/// 理论卷模板内容（编译时嵌入）
const THEORY_TEMPLATE: &str =
    include_str!("../../../CPHOS-Latex/theory/examples/example-paper.tex");
/// 实验卷模板内容（编译时嵌入）
const EXPERIMENT_TEMPLATE: &str =
    include_str!("../../../CPHOS-Latex/experiment/examples/example-paper.tex");

// ============================================================
// 正则表达式（惰性初始化）
// ============================================================

/// problem 环境匹配正则
/// 匹配 \begin{problem}...\end{problem}（非贪婪，跨行）
static PROBLEM_ENV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\\begin\{problem\}.*?\\end\{problem\}").unwrap());

/// 标题命令匹配正则
/// 匹配 \cphostitle{...}
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\cphostitle\{[^{}]*\}").unwrap());

/// 副标题命令匹配正则
/// 匹配 \cphossubtitle{...}
static SUBTITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\cphossubtitle\{[^{}]*\}").unwrap());

/// 命题人区块匹配正则
/// 匹配 \noindent{\textbf{命题人}}...\\noindent{\textbf{审题人}}
static AUTHORS_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)(\\noindent\{\\textbf\{命题人\}\}\s*)(.*?)(\s*\\noindent\{\\textbf\{审题人\}\})",
    )
    .unwrap()
});

/// 审题人区块匹配正则
/// 匹配 \noindent{\textbf{审题人}}...\vspace{0.5em}
static REVIEWERS_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(\\noindent\{\\textbf\{审题人\}\}\s*)(.*?)(\s*\\vspace\{0\.5em\})").unwrap()
});

/// includegraphics 命令匹配正则
/// 匹配 \includegraphics[options]{path}
static INCLUDEGRAPHICS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(?P<command>\\includegraphics\*?)(?P<options>\[[^\]]*\])?\{(?P<path>[^{}]+)\}")
        .unwrap()
});

/// 标签引用命令匹配正则
/// 匹配 \label/ref/eqref 等命令
static LABEL_REWRITE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\\(?P<command>label|ref|eqref|pageref|autoref|cref|Cref)\{(?P<target>[^{}]+)\}")
        .unwrap()
});

// ============================================================
// PaperTemplateKind 枚举
// ============================================================
/// 试卷模板类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaperTemplateKind {
    Theory,      // 理论卷
    Experiment,  // 实验卷
}

// ============================================================
// RenderPaperInput 结构体
// ============================================================
/// 试卷渲染输入数据
#[derive(Debug, Clone)]
pub(crate) struct RenderPaperInput {
    pub(crate) title: String,                  // 试卷标题
    pub(crate) subtitle: String,               // 副标题
    pub(crate) authors: Vec<String>,           // 命题人列表
    pub(crate) reviewers: Vec<String>,         // 审题人列表
    pub(crate) template_kind: PaperTemplateKind, // 模板类型
    pub(crate) questions: Vec<RenderQuestionInput>, // 题目列表
}

// ============================================================
// RenderQuestionInput 结构体
// ============================================================
/// 单个题目的渲染输入
#[derive(Debug, Clone)]
pub(crate) struct RenderQuestionInput {
    pub(crate) question_id: String,        // 题目 UUID
    pub(crate) sequence: usize,            // 题目序号
    pub(crate) source_tex_path: String,    // TeX 源路径
    pub(crate) source_tex: String,         // TeX 源码
    pub(crate) assets: Vec<RenderQuestionAssetInput>, // 资源文件
}

// ============================================================
// RenderQuestionAssetInput 结构体
// ============================================================
/// 题目资源文件输入
#[derive(Debug, Clone)]
pub(crate) struct RenderQuestionAssetInput {
    pub(crate) original_path: String,  // 原始路径
    pub(crate) object_id: String,      // 对象 UUID
    pub(crate) mime_type: Option<String>, // MIME 类型
    pub(crate) bytes: Vec<u8>,         // 文件字节
}

// ============================================================
// RenderedPaperBundle 结构体
// ============================================================
/// 渲染后的试卷包
#[derive(Debug)]
pub(crate) struct RenderedPaperBundle {
    pub(crate) main_tex: String,                  // 渲染后的主 TeX
    pub(crate) template_source_path: &'static str, // 模板源路径
    pub(crate) assets: Vec<RenderedPaperAsset>,   // 资源文件列表
    pub(crate) questions: Vec<RenderedPaperQuestion>, // 题目信息
}

// ============================================================
// RenderedPaperAsset 结构体
// ============================================================
/// 渲染后的资源文件
#[derive(Debug)]
pub(crate) struct RenderedPaperAsset {
    pub(crate) question_id: String,    // 来源题目 ID
    pub(crate) original_path: String,  // 原始路径
    pub(crate) output_path: String,    // 输出路径（带前缀）
    pub(crate) object_id: String,      // 对象 UUID
    pub(crate) mime_type: Option<String>, // MIME 类型
    pub(crate) bytes: Vec<u8>,         // 文件字节
}

// ============================================================
// RenderedPaperQuestion 结构体
// ============================================================
/// 渲染后的题目信息
#[derive(Debug)]
pub(crate) struct RenderedPaperQuestion {
    pub(crate) question_id: String,   // 题目 UUID
    pub(crate) sequence: usize,       // 题目序号
    pub(crate) source_tex_path: String, // TeX 源路径
    pub(crate) asset_prefix: String,  // 资源文件前缀
}

// ============================================================
// PaperTemplateKind 实现
// ============================================================
impl PaperTemplateKind {
    /// 获取模板源路径
    fn template_source_path(self) -> &'static str {
        match self {
            Self::Theory => THEORY_TEMPLATE_PATH,
            Self::Experiment => EXPERIMENT_TEMPLATE_PATH,
        }
    }

    /// 获取模板内容
    fn template_body(self) -> &'static str {
        match self {
            Self::Theory => THEORY_TEMPLATE,
            Self::Experiment => EXPERIMENT_TEMPLATE,
        }
    }
}

// ============================================================
// render_paper_bundle 函数
// ============================================================
/// 渲染试卷包
///
/// # 参数
/// - input: 渲染输入数据
///
/// # 处理流程
/// 1. 根据模板类型获取模板
/// 2. 调用 render_with_template 渲染
///
/// # 返回
/// 渲染后的试卷包
pub(crate) fn render_paper_bundle(input: RenderPaperInput) -> Result<RenderedPaperBundle> {
    // 获取模板内容和路径
    let template = input.template_kind.template_body();
    let template_source_path = input.template_kind.template_source_path();
    // 执行渲染
    render_with_template(template, template_source_path, input)
}

// ============================================================
// render_with_template 函数
// ============================================================
/// 使用模板渲染试卷
///
/// # 处理流程
/// 1. 验证题目非空
/// 2. 为每个题目：
///    - 生成资源文件输出名（带前缀）
///    - 提取 problem 环境
///    - 重写资源路径和标签引用
/// 3. 注入模板（标题、作者、题目）
fn render_with_template(
    template: &str,
    template_source_path: &'static str,
    input: RenderPaperInput,
) -> Result<RenderedPaperBundle> {
    // 验证至少有一道题目
    if input.questions.is_empty() {
        bail!("paper bundle rendering requires at least one question");
    }

    // 存储渲染结果
    let mut rendered_assets = Vec::new();
    let mut rendered_questions = Vec::with_capacity(input.questions.len());
    let mut rendered_problem_blocks = Vec::with_capacity(input.questions.len());

    // 遍历每个题目
    for question in input.questions {
        // 生成资源前缀（如 "p1-" 表示第 1 题）
        let asset_prefix = format!("p{}-", question.sequence);
        // 资源路径映射表
        let mut asset_path_map = HashMap::new();
        // 用于检测重复输出路径
        let mut seen_output_paths = HashSet::new();

        // 处理每个资源文件
        for asset in question.assets {
            // 生成输出路径（带前缀，路径分隔符转双下划线）
            let output_path = format!(
                "assets/{}",
                build_asset_output_name(&asset_prefix, &asset.original_path)?
            );
            // 检查重复
            if !seen_output_paths.insert(output_path.clone()) {
                bail!(
                    "question {} produces duplicate rendered asset path: {}",
                    question.question_id,
                    output_path
                );
            }

            // 为资源文件生成多个别名路径（支持不同引用方式）
            for alias in build_asset_aliases(&asset.original_path) {
                asset_path_map.insert(alias, output_path.clone());
            }

            // 添加到渲染资源列表
            rendered_assets.push(RenderedPaperAsset {
                question_id: question.question_id.clone(),
                original_path: asset.original_path,
                output_path,
                object_id: asset.object_id,
                mime_type: asset.mime_type,
                bytes: asset.bytes,
            });
        }

        // 提取 problem 环境
        let problem_block = extract_problem_block(&question.source_tex).with_context(|| {
            format!(
                "extract problem block failed for question {} ({})",
                question.question_id, question.source_tex_path
            )
        })?;

        // 重写 problem 环境中的路径和引用
        let rewritten_problem =
            rewrite_problem_block(&problem_block, &asset_prefix, &asset_path_map);
        rendered_problem_blocks.push(rewritten_problem);

        // 添加到题目列表
        rendered_questions.push(RenderedPaperQuestion {
            question_id: question.question_id,
            sequence: question.sequence,
            source_tex_path: question.source_tex_path,
            asset_prefix,
        });
    }

    // 格式化作者和审核者列表
    let authors = format_people_list(&input.authors);
    let reviewers = format_people_list(&input.reviewers);

    // 注入模板
    let rendered = inject_paper_content(
        template,
        &escape_latex_text(&input.title),      // 转义 LaTeX 特殊字符
        &escape_latex_text(&input.subtitle),
        &authors,
        &reviewers,
        &rendered_problem_blocks.join("\n\n"),  // 题目之间用空行分隔
    )
    .with_context(|| format!("render paper bundle from template failed: {template_source_path}"))?;

    Ok(RenderedPaperBundle {
        main_tex: rendered,
        template_source_path,
        assets: rendered_assets,
        questions: rendered_questions,
    })
}

// ============================================================
// inject_paper_content 函数
// ============================================================
/// 将内容注入模板
///
/// # 替换顺序
/// 1. 标题 \cphostitle{...}
/// 2. 副标题 \cphossubtitle{...}
/// 3. 命题人区块
/// 4. 审题人区块
/// 5. problem 环境
fn inject_paper_content(
    template: &str,
    title: &str,
    subtitle: &str,
    authors: &str,
    reviewers: &str,
    problems: &str,
) -> Result<String> {
    // 替换标题
    let with_title = replace_single_command(template, &TITLE_RE, "cphostitle", title)?;
    // 替换副标题
    let with_subtitle =
        replace_single_command(&with_title, &SUBTITLE_RE, "cphossubtitle", subtitle)?;
    // 替换命题人区块
    let with_authors =
        replace_named_block(&with_subtitle, &AUTHORS_BLOCK_RE, "authors block", authors)?;
    // 替换审题人区块
    let with_reviewers = replace_named_block(
        &with_authors,
        &REVIEWERS_BLOCK_RE,
        "reviewers block",
        reviewers,
    )?;
    // 替换 problem 环境
    replace_first_problem_block(&with_reviewers, problems)
}

// ============================================================
// replace_single_command 函数
// ============================================================
/// 替换单个 LaTeX 命令
///
/// # 参数
/// - input: 输入文本
/// - regex: 匹配正则
/// - command: 命令名（用于错误消息）
/// - value: 替换值
fn replace_single_command(
    input: &str,
    regex: &Regex,
    command: &str,
    value: &str,
) -> Result<String> {
    // 验证模板中存在该命令
    if !regex.is_match(input) {
        bail!("template is missing \\{command}{{...}}");
    }
    // 构建替换文本
    let replacement = format!(r"\{command}{{{value}}}");
    // 执行替换（只替换第一次）
    Ok(regex
        .replacen(input, 1, |_caps: &Captures<'_>| replacement.clone())
        .into_owned())
}

// ============================================================
// replace_named_block 函数
// ============================================================
/// 替换命名区块
///
/// # 参数
/// - input: 输入文本
/// - regex: 匹配正则（捕获前缀、内容、后缀）
/// - label: 区块名（用于错误消息）
/// - value: 新内容
fn replace_named_block(input: &str, regex: &Regex, label: &str, value: &str) -> Result<String> {
    // 验证模板中存在该区块
    if !regex.is_match(input) {
        bail!("template is missing {label}");
    }
    // 使用捕获组保留前缀和后缀，替换中间内容
    Ok(regex
        .replace(input, |caps: &Captures<'_>| {
            format!("{}{}{}", &caps[1], value, &caps[3])
        })
        .into_owned())
}

// ============================================================
// replace_first_problem_block 函数
// ============================================================
/// 替换第一个 problem 环境
fn replace_first_problem_block(template: &str, problems: &str) -> Result<String> {
    // 验证模板中存在 problem 环境
    if !PROBLEM_ENV_RE.is_match(template) {
        bail!("template does not contain a sample problem block");
    }
    // 替换第一个 problem 环境
    Ok(PROBLEM_ENV_RE
        .replacen(template, 1, |_caps: &Captures<'_>| problems.to_string())
        .into_owned())
}

// ============================================================
// extract_problem_block 函数
// ============================================================
/// 从 TeX 源码中提取 problem 环境
fn extract_problem_block(source: &str) -> Result<String> {
    PROBLEM_ENV_RE
        .find(source)
        .map(|matched| matched.as_str().to_string())
        .ok_or_else(|| anyhow::anyhow!("question tex does not contain a \\begin{{problem}} block"))
}

// ============================================================
// rewrite_problem_block 函数
// ============================================================
/// 重写 problem 环境中的路径和引用
///
/// # 处理内容
/// 1. 重写 \includegraphics 路径
/// 2. 重写 \label/\ref 等引用
fn rewrite_problem_block(
    problem_block: &str,
    asset_prefix: &str,
    asset_path_map: &HashMap<String, String>,
) -> String {
    // 重写图片路径
    let with_assets = INCLUDEGRAPHICS_RE.replace_all(problem_block, |caps: &Captures<'_>| {
        let original_path = caps
            .name("path")
            .map(|match_| match_.as_str())
            .unwrap_or_default();
        let normalized_path = normalize_tex_path(original_path);
        // 查找映射后的路径
        let replacement_path = asset_path_map
            .get(&normalized_path)
            .cloned()
            .unwrap_or_else(|| original_path.to_string());
        let options = caps
            .name("options")
            .map(|match_| match_.as_str())
            .unwrap_or("");
        // 重建命令
        format!("{}{}{{{}}}", &caps["command"], options, replacement_path)
    });

    // 重写标签引用
    let rewritten = LABEL_REWRITE_RE
        .replace_all(&with_assets, |caps: &Captures<'_>| {
            format!(
                "\\{}{{{}}}",
                &caps["command"],
                prefix_label_target(&caps["target"], asset_prefix)  // 添加前缀
            )
        })
        .into_owned();

    // 规范化环境分隔符
    normalize_environment_delimiter_lines(&rewritten)
}

// ============================================================
// normalize_environment_delimiter_lines 函数
// ============================================================
/// 规范化环境分隔符行（去除缩进）
///
/// # 处理
/// \begin{...} 和 \end{...} 行去掉首尾空白
fn normalize_environment_delimiter_lines(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            // 如果是环境开始/结束标记，去掉缩进
            if trimmed.starts_with(r"\begin{") || trimmed.starts_with(r"\end{") {
                trimmed.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ============================================================
// prefix_label_target 函数
// ============================================================
/// 为标签目标添加前缀
///
/// # 规则
/// - 如果有命名空间（如 "fig:sample"），前缀加在命名空间后
/// - 否则直接加前缀
///
/// # 示例
/// - "fig:sample" + "p1-" → "fig:p1-sample"
/// - "eq:main" + "p1-" → "eq:p1-main"
fn prefix_label_target(target: &str, asset_prefix: &str) -> String {
    match target.split_once(':') {
        Some((head, tail)) if !tail.is_empty() => format!("{head}:{asset_prefix}{tail}"),
        _ => format!("{asset_prefix}{target}"),
    }
}

// ============================================================
// build_asset_aliases 函数
// ============================================================
/// 生成资源文件的别名路径
///
/// # 目的
/// 支持不同的引用方式：
/// - assets/fig.png
/// - fig.png
/// - assets/fig (无扩展名)
fn build_asset_aliases(original_path: &str) -> Vec<String> {
    // 规范化路径
    let normalized = normalize_tex_path(original_path);
    let mut aliases = HashSet::new();
    aliases.insert(normalized.clone());

    // 如果以 assets/ 开头，添加不带前缀的版本
    if let Some(stripped) = normalized.strip_prefix("assets/") {
        aliases.insert(stripped.to_string());
    }

    // 为每个别名添加不带扩展名的版本
    let existing = aliases.iter().cloned().collect::<Vec<_>>();
    for alias in existing {
        if let Some(without_ext) = strip_extension(&alias) {
            aliases.insert(without_ext);
        }
    }

    // 排序后返回
    let mut result = aliases.into_iter().collect::<Vec<_>>();
    result.sort();
    result
}

// ============================================================
// build_asset_output_name 函数
// ============================================================
/// 生成资源文件的输出名称
///
/// # 规则
/// - 添加前缀
/// - 路径分隔符转为双下划线
///
/// # 示例
/// "assets/figs/sample.png" + "p1-" → "p1-figs__sample.png"
fn build_asset_output_name(asset_prefix: &str, original_path: &str) -> Result<String> {
    let normalized = normalize_tex_path(original_path);
    // 去掉 assets/ 前缀
    let relative = normalized.strip_prefix("assets/").unwrap_or(&normalized);
    if relative.is_empty() {
        bail!("asset path must not be empty");
    }

    // 路径分隔符转双下划线
    Ok(format!("{asset_prefix}{}", relative.replace('/', "__")))
}

// ============================================================
// strip_extension 函数
// ============================================================
/// 去除文件扩展名
fn strip_extension(path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let extension = candidate.extension()?.to_str()?;
    if extension.is_empty() {
        return None;
    }

    let stem = candidate.file_stem()?.to_str()?;
    let parent = candidate.parent().and_then(|parent| parent.to_str());
    Some(match parent {
        Some(parent) if !parent.is_empty() => format!("{parent}/{stem}"),
        _ => stem.to_string(),
    })
}

// ============================================================
// normalize_tex_path 函数
// ============================================================
/// 规范化 TeX 路径
///
/// # 处理
/// 1. 替换反斜杠为正斜杠
/// 2. 去除 ./ 前缀
/// 3. 合并多个斜杠
fn normalize_tex_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    // 去除 ./ 前缀
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    // 合并 //
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized
}

// ============================================================
// format_people_list 函数
// ============================================================
/// 格式化人员列表
///
/// # 输出格式
/// 人名之间用 \quad 分隔
fn format_people_list(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format_person_name(name))
        .collect::<Vec<_>>()
        .join(r"\quad ")
}

// ============================================================
// format_person_name 函数
// ============================================================
/// 格式化单个姓名
///
/// # 规则
/// - 两字中文名：字之间加 ~（防止换行）
/// - 其他：直接转义
fn format_person_name(name: &str) -> String {
    let trimmed = name.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    // 判断是否为两字中文
    if chars.len() == 2 && chars.iter().all(|ch| is_cjk_char(*ch)) {
        // 两字中文：张 ~三
        format!(
            "{}~{}",
            escape_latex_text(&chars[0].to_string()),
            escape_latex_text(&chars[1].to_string())
        )
    } else {
        escape_latex_text(trimmed)
    }
}

// ============================================================
// is_cjk_char 函数
// ============================================================
/// 判断是否为中日韩字符
fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF   // CJK 扩展 A
            | 0x4E00..=0x9FFF   // CJK 统一汉字
            | 0xF900..=0xFAFF   // CJK 兼容汉字
            | 0x20000..=0x2A6DF // CJK 扩展 B
            | 0x2A700..=0x2B73F // CJK 扩展 C
            | 0x2B740..=0x2B81F // CJK 扩展 D
            | 0x2B820..=0x2CEAF // CJK 扩展 E
            | 0x2CEB0..=0x2EBEF // CJK 扩展 F
    )
}

// ============================================================
// escape_latex_text 函数
// ============================================================
/// 转义 LaTeX 特殊字符
fn escape_latex_text(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str(r"\textbackslash{}"),
            '{' => escaped.push_str(r"\{"),
            '}' => escaped.push_str(r"\}"),
            '$' => escaped.push_str(r"\$"),
            '&' => escaped.push_str(r"\&"),
            '#' => escaped.push_str(r"\#"),
            '_' => escaped.push_str(r"\_"),
            '%' => escaped.push_str(r"\%"),
            '~' => escaped.push_str(r"\textasciitilde{}"),
            '^' => escaped.push_str(r"\textasciicircum{}"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. include_str! 宏
 *    - 编译时嵌入文件内容
 *    - 返回 &'static str
 *    - 适合模板、配置文件
 *
 * 2. LazyLock 惰性初始化
 *    - 第一次使用时初始化
 *    - 线程安全
 *    - 适合正则表达式等重型对象
 *
 * 3. 正则表达式替换
 *    - regex.replace_all(input, |caps| {...})
 *    - 使用闭包动态生成替换内容
 *    - Captures 通过索引或名称访问捕获组
 *
 * 4. 正则语法
 *    (?s) - 点号匹配换行（单行模式）
 *    .*? - 非贪婪匹配
 *    (?P<name>...) - 命名捕获组
 *    \\{...} - 匹配字面量 { }
 *
 * 5. CJK 字符判断
 *    - 使用 Unicode 码点范围
 *    - matches! 宏简洁判断
 *    - 覆盖所有中日韩扩展区
 *
 * ============================================================
 * LaTeX 模板替换流程
 * ============================================================
 *
 * 原始模板:
 * \documentclass[exam]{cphos}
 * \cphostitle{旧标题}
 * \cphossubtitle{旧副标题}
 * \noindent{\textbf{命题人}} X~X
 * \noindent{\textbf{审题人}} Y~Y
 * \begin{problem}[10]{示例题} 示例内容\end{problem}
 * \end{document}
 *
 * 替换步骤:
 * 1. \cphostitle{旧标题} → \cphostitle{新标题}
 * 2. \cphossubtitle{...} → \cphossubtitle{新副标题}
 * 3. 命题人区块 → 新命题人列表
 * 4. 审题人区块 → 新审题人列表
 * 5. problem 环境 → 题目内容
 *
 * ============================================================
 * 资源文件路径重写示例
 * ============================================================
 *
 * 原始 TeX:
 * \includegraphics[width=0.5\textwidth]{assets/figs/sample.png}
 * \ref{fig:sample}
 * \label{fig:sample}
 *
 * 渲染后 (第 1 题):
 * \includegraphics[width=0.5\textwidth]{assets/p1-figs__sample.png}
 * \ref{fig:p1-sample}
 * \label{fig:p1-sample}
 *
 * 输出文件:
 * assets/p1-figs__sample.png
 *
 * ============================================================
 * 试卷渲染输出结构
 * ============================================================
 *
 * RenderedPaperBundle {
 *   main_tex: "渲染后的完整 TeX 文档",
 *   template_source_path: "CPHOS-Latex/theory/...",
 *   assets: [
 *     RenderedPaperAsset {
 *       question_id: "q1",
 *       original_path: "assets/fig.png",
 *       output_path: "assets/p1-fig.png",
 *       bytes: [...]
 *     }
 *   ],
 *   questions: [
 *     RenderedPaperQuestion {
 *       question_id: "q1",
 *       sequence: 1,
 *       asset_prefix: "p1-"
 *     }
 *   ]
 * }
 *
 */
