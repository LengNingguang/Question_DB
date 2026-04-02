use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::LazyLock,
};

use anyhow::{bail, Context, Result};
use regex::{Captures, Regex};

const THEORY_TEMPLATE_PATH: &str = "CPHOS-Latex/theory/examples/example-paper.tex";
const EXPERIMENT_TEMPLATE_PATH: &str = "CPHOS-Latex/experiment/examples/example-paper.tex";
const THEORY_TEMPLATE: &str =
    include_str!("../../../CPHOS-Latex/theory/examples/example-paper.tex");
const EXPERIMENT_TEMPLATE: &str =
    include_str!("../../../CPHOS-Latex/experiment/examples/example-paper.tex");

static PROBLEM_ENV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\\begin\{problem\}.*?\\end\{problem\}").unwrap());
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\cphostitle\{[^{}]*\}").unwrap());
static SUBTITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\cphossubtitle\{[^{}]*\}").unwrap());
static AUTHORS_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)(\\noindent\{\\textbf\{命题人\}\}\s*)(.*?)(\s*\\noindent\{\\textbf\{审题人\}\})",
    )
    .unwrap()
});
static REVIEWERS_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(\\noindent\{\\textbf\{审题人\}\}\s*)(.*?)(\s*\\vspace\{0\.5em\})").unwrap()
});
static INCLUDEGRAPHICS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(?P<command>\\includegraphics\*?)(?P<options>\[[^\]]*\])?\{(?P<path>[^{}]+)\}")
        .unwrap()
});
static LABEL_REWRITE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\\(?P<command>label|ref|eqref|pageref|autoref|cref|Cref)\{(?P<target>[^{}]+)\}")
        .unwrap()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaperTemplateKind {
    Theory,
    Experiment,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderPaperInput {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) reviewers: Vec<String>,
    pub(crate) template_kind: PaperTemplateKind,
    pub(crate) questions: Vec<RenderQuestionInput>,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderQuestionInput {
    pub(crate) question_id: String,
    pub(crate) sequence: usize,
    pub(crate) source_tex_path: String,
    pub(crate) source_tex: String,
    pub(crate) assets: Vec<RenderQuestionAssetInput>,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderQuestionAssetInput {
    pub(crate) original_path: String,
    pub(crate) object_id: String,
    pub(crate) mime_type: Option<String>,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct RenderedPaperBundle {
    pub(crate) main_tex: String,
    pub(crate) template_source_path: &'static str,
    pub(crate) assets: Vec<RenderedPaperAsset>,
    pub(crate) questions: Vec<RenderedPaperQuestion>,
}

#[derive(Debug)]
pub(crate) struct RenderedPaperAsset {
    pub(crate) question_id: String,
    pub(crate) original_path: String,
    pub(crate) output_path: String,
    pub(crate) object_id: String,
    pub(crate) mime_type: Option<String>,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct RenderedPaperQuestion {
    pub(crate) question_id: String,
    pub(crate) sequence: usize,
    pub(crate) source_tex_path: String,
    pub(crate) asset_prefix: String,
}

impl PaperTemplateKind {
    fn template_source_path(self) -> &'static str {
        match self {
            Self::Theory => THEORY_TEMPLATE_PATH,
            Self::Experiment => EXPERIMENT_TEMPLATE_PATH,
        }
    }

    fn template_body(self) -> &'static str {
        match self {
            Self::Theory => THEORY_TEMPLATE,
            Self::Experiment => EXPERIMENT_TEMPLATE,
        }
    }
}

pub(crate) fn render_paper_bundle(input: RenderPaperInput) -> Result<RenderedPaperBundle> {
    let template = input.template_kind.template_body();
    let template_source_path = input.template_kind.template_source_path();
    render_with_template(template, template_source_path, input)
}

fn render_with_template(
    template: &str,
    template_source_path: &'static str,
    input: RenderPaperInput,
) -> Result<RenderedPaperBundle> {
    if input.questions.is_empty() {
        bail!("paper bundle rendering requires at least one question");
    }

    let mut rendered_assets = Vec::new();
    let mut rendered_questions = Vec::with_capacity(input.questions.len());
    let mut rendered_problem_blocks = Vec::with_capacity(input.questions.len());

    for question in input.questions {
        let asset_prefix = format!("p{}-", question.sequence);
        let mut asset_path_map = HashMap::new();
        let mut seen_output_paths = HashSet::new();

        for asset in question.assets {
            let output_path = format!(
                "assets/{}",
                build_asset_output_name(&asset_prefix, &asset.original_path)?
            );
            if !seen_output_paths.insert(output_path.clone()) {
                bail!(
                    "question {} produces duplicate rendered asset path: {}",
                    question.question_id,
                    output_path
                );
            }

            for alias in build_asset_aliases(&asset.original_path) {
                asset_path_map.insert(alias, output_path.clone());
            }

            rendered_assets.push(RenderedPaperAsset {
                question_id: question.question_id.clone(),
                original_path: asset.original_path,
                output_path,
                object_id: asset.object_id,
                mime_type: asset.mime_type,
                bytes: asset.bytes,
            });
        }

        let problem_block = extract_problem_block(&question.source_tex).with_context(|| {
            format!(
                "extract problem block failed for question {} ({})",
                question.question_id, question.source_tex_path
            )
        })?;
        let rewritten_problem =
            rewrite_problem_block(&problem_block, &asset_prefix, &asset_path_map);
        rendered_problem_blocks.push(rewritten_problem);
        rendered_questions.push(RenderedPaperQuestion {
            question_id: question.question_id,
            sequence: question.sequence,
            source_tex_path: question.source_tex_path,
            asset_prefix,
        });
    }

    let authors = format_people_list(&input.authors);
    let reviewers = format_people_list(&input.reviewers);
    let rendered = inject_paper_content(
        template,
        &escape_latex_text(&input.title),
        &escape_latex_text(&input.subtitle),
        &authors,
        &reviewers,
        &rendered_problem_blocks.join("\n\n"),
    )
    .with_context(|| format!("render paper bundle from template failed: {template_source_path}"))?;

    Ok(RenderedPaperBundle {
        main_tex: rendered,
        template_source_path,
        assets: rendered_assets,
        questions: rendered_questions,
    })
}

fn inject_paper_content(
    template: &str,
    title: &str,
    subtitle: &str,
    authors: &str,
    reviewers: &str,
    problems: &str,
) -> Result<String> {
    let with_title = replace_single_command(template, &TITLE_RE, "cphostitle", title)?;
    let with_subtitle =
        replace_single_command(&with_title, &SUBTITLE_RE, "cphossubtitle", subtitle)?;
    let with_authors =
        replace_named_block(&with_subtitle, &AUTHORS_BLOCK_RE, "authors block", authors)?;
    let with_reviewers = replace_named_block(
        &with_authors,
        &REVIEWERS_BLOCK_RE,
        "reviewers block",
        reviewers,
    )?;
    replace_first_problem_block(&with_reviewers, problems)
}

fn replace_single_command(
    input: &str,
    regex: &Regex,
    command: &str,
    value: &str,
) -> Result<String> {
    if !regex.is_match(input) {
        bail!("template is missing \\{command}{{...}}");
    }
    let replacement = format!(r"\{command}{{{value}}}");
    Ok(regex
        .replacen(input, 1, |_caps: &Captures<'_>| replacement.clone())
        .into_owned())
}

fn replace_named_block(input: &str, regex: &Regex, label: &str, value: &str) -> Result<String> {
    if !regex.is_match(input) {
        bail!("template is missing {label}");
    }
    Ok(regex
        .replace(input, |caps: &Captures<'_>| {
            format!("{}{}{}", &caps[1], value, &caps[3])
        })
        .into_owned())
}

fn replace_first_problem_block(template: &str, problems: &str) -> Result<String> {
    if !PROBLEM_ENV_RE.is_match(template) {
        bail!("template does not contain a sample problem block");
    }
    Ok(PROBLEM_ENV_RE
        .replacen(template, 1, |_caps: &Captures<'_>| problems.to_string())
        .into_owned())
}

fn extract_problem_block(source: &str) -> Result<String> {
    PROBLEM_ENV_RE
        .find(source)
        .map(|matched| matched.as_str().to_string())
        .ok_or_else(|| anyhow::anyhow!("question tex does not contain a \\begin{{problem}} block"))
}

fn rewrite_problem_block(
    problem_block: &str,
    asset_prefix: &str,
    asset_path_map: &HashMap<String, String>,
) -> String {
    let with_assets = INCLUDEGRAPHICS_RE.replace_all(problem_block, |caps: &Captures<'_>| {
        let original_path = caps
            .name("path")
            .map(|match_| match_.as_str())
            .unwrap_or_default();
        let normalized_path = normalize_tex_path(original_path);
        let replacement_path = asset_path_map
            .get(&normalized_path)
            .cloned()
            .unwrap_or_else(|| original_path.to_string());
        let options = caps
            .name("options")
            .map(|match_| match_.as_str())
            .unwrap_or("");
        format!("{}{}{{{}}}", &caps["command"], options, replacement_path)
    });

    let rewritten = LABEL_REWRITE_RE
        .replace_all(&with_assets, |caps: &Captures<'_>| {
            format!(
                "\\{}{{{}}}",
                &caps["command"],
                prefix_label_target(&caps["target"], asset_prefix)
            )
        })
        .into_owned();

    normalize_environment_delimiter_lines(&rewritten)
}

fn normalize_environment_delimiter_lines(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with(r"\begin{") || trimmed.starts_with(r"\end{") {
                trimmed.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefix_label_target(target: &str, asset_prefix: &str) -> String {
    match target.split_once(':') {
        Some((head, tail)) if !tail.is_empty() => format!("{head}:{asset_prefix}{tail}"),
        _ => format!("{asset_prefix}{target}"),
    }
}

fn build_asset_aliases(original_path: &str) -> Vec<String> {
    let normalized = normalize_tex_path(original_path);
    let mut aliases = HashSet::new();
    aliases.insert(normalized.clone());
    if let Some(stripped) = normalized.strip_prefix("assets/") {
        aliases.insert(stripped.to_string());
    }

    let existing = aliases.iter().cloned().collect::<Vec<_>>();
    for alias in existing {
        if let Some(without_ext) = strip_extension(&alias) {
            aliases.insert(without_ext);
        }
    }

    let mut result = aliases.into_iter().collect::<Vec<_>>();
    result.sort();
    result
}

fn build_asset_output_name(asset_prefix: &str, original_path: &str) -> Result<String> {
    let normalized = normalize_tex_path(original_path);
    let relative = normalized.strip_prefix("assets/").unwrap_or(&normalized);
    if relative.is_empty() {
        bail!("asset path must not be empty");
    }

    Ok(format!("{asset_prefix}{}", relative.replace('/', "__")))
}

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

fn normalize_tex_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized
}

fn format_people_list(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format_person_name(name))
        .collect::<Vec<_>>()
        .join(r"\quad ")
}

fn format_person_name(name: &str) -> String {
    let trimmed = name.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() == 2 && chars.iter().all(|ch| is_cjk_char(*ch)) {
        format!(
            "{}~{}",
            escape_latex_text(&chars[0].to_string()),
            escape_latex_text(&chars[1].to_string())
        )
    } else {
        escape_latex_text(trimmed)
    }
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
    )
}

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

#[cfg(test)]
mod tests {
    use super::{
        inject_paper_content, render_with_template, PaperTemplateKind, RenderPaperInput,
        RenderQuestionAssetInput, RenderQuestionInput,
    };

    fn sample_template() -> &'static str {
        r#"\documentclass[exam]{cphos}
\cphostitle{旧标题}
\cphossubtitle{旧副标题}
\begin{document}
\begin{problem}[10]{示例题}
示例内容
\end{problem}
\begin{copyrightinfo}
\noindent{\textbf{命题人}}

X~X\quad XXX\quad XXX

\noindent{\textbf{审题人}}

Y~Y\quad YYY\quad YYY

\vspace{0.5em}
\end{copyrightinfo}
\end{document}
"#
    }

    #[test]
    fn inject_paper_content_replaces_metadata_and_problem_slot() {
        let rendered = inject_paper_content(
            sample_template(),
            "新标题",
            "新副标题",
            r"张~三\quad 李四五",
            r"王~五",
            r"\begin{problem}[20]{正式题目}内容\end{problem}",
        )
        .expect("template should render");

        assert!(rendered.contains(r"\cphostitle{新标题}"));
        assert!(rendered.contains(r"\cphossubtitle{新副标题}"));
        assert!(rendered.contains("张~三\\quad 李四五"));
        assert!(rendered.contains("王~五"));
        assert!(rendered.contains(r"\begin{problem}[20]{正式题目}内容\end{problem}"));
        assert!(!rendered.contains("示例内容"));
    }

    #[test]
    fn inject_paper_content_preserves_inline_math_in_problem_replacement() {
        let rendered = inject_paper_content(
            sample_template(),
            "新标题",
            "新副标题",
            r"张~三",
            r"王~五",
            r#"\begin{problem}[40]{电容器的击穿}
\begin{problemstatement}
圆形平行板面积为$S$，极板间距为$d_0$，环境温度为$T_0$，热容量为$C$，电压为$V_0$，电容为$1\,\upmu\text{F}$。
\end{problemstatement}
\begin{solution}
$V = V_0\cos(\omega t)$
\end{solution}
\end{problem}"#,
        )
        .expect("template should preserve math");

        assert!(rendered.contains(r"$S$"));
        assert!(rendered.contains(r"$d_0$"));
        assert!(rendered.contains(r"$T_0$"));
        assert!(rendered.contains(r"$C$"));
        assert!(rendered.contains(r"$V_0$"));
        assert!(rendered.contains(r"$1\,\upmu\text{F}$"));
        assert!(rendered.contains(r"$V = V_0\cos(\omega t)$"));
    }

    #[test]
    fn render_with_template_rewrites_assets_and_refs() {
        let rendered = render_with_template(
            sample_template(),
            "test-template.tex",
            RenderPaperInput {
                title: "竞赛试卷".into(),
                subtitle: "理论部分".into(),
                authors: vec!["张三".into(), "李四五".into()],
                reviewers: vec!["王五".into()],
                template_kind: PaperTemplateKind::Theory,
                questions: vec![RenderQuestionInput {
                    question_id: "q1".into(),
                    sequence: 1,
                    source_tex_path: "main.tex".into(),
                    source_tex: r#"\documentclass[answer]{cphos}
\begin{document}
\begin{problem}[40]{样题}
\begin{problemstatement}
见图\ref{fig:sample}，并参考式\eqref{eq:main}。
\begin{figure}[H]
\includegraphics[width=0.5\textwidth]{assets/figs/sample.png}
\caption{示意图}
\label{fig:sample}
\end{figure}
\end{problemstatement}
\begin{solution}
\begin{equation}
E=mc^2 \label{eq:main}
\end{equation}
\end{solution}
\end{problem}
\end{document}"#
                        .into(),
                    assets: vec![RenderQuestionAssetInput {
                        original_path: "assets/figs/sample.png".into(),
                        object_id: "obj-1".into(),
                        mime_type: Some("image/png".into()),
                        bytes: b"png".to_vec(),
                    }],
                }],
            },
        )
        .expect("paper should render");

        assert!(rendered.main_tex.contains(r"\cphostitle{竞赛试卷}"));
        assert!(rendered.main_tex.contains(r"\cphossubtitle{理论部分}"));
        assert!(rendered.main_tex.contains("张~三\\quad 李四五"));
        assert!(rendered.main_tex.contains("王~五"));
        assert!(rendered
            .main_tex
            .contains(r"\includegraphics[width=0.5\textwidth]{assets/p1-figs__sample.png}"));
        assert!(rendered.main_tex.contains(r"\label{fig:p1-sample}"));
        assert!(rendered.main_tex.contains(r"\ref{fig:p1-sample}"));
        assert!(rendered.main_tex.contains(r"\eqref{eq:p1-main}"));
        assert_eq!(rendered.assets.len(), 1);
        assert_eq!(rendered.assets[0].output_path, "assets/p1-figs__sample.png");
        assert_eq!(rendered.questions[0].asset_prefix, "p1-");
    }

    #[test]
    fn render_with_template_unindents_solution_delimiters() {
        let rendered = render_with_template(
            sample_template(),
            "test-template.tex",
            RenderPaperInput {
                title: "竞赛试卷".into(),
                subtitle: "理论部分".into(),
                authors: vec![],
                reviewers: vec![],
                template_kind: PaperTemplateKind::Theory,
                questions: vec![RenderQuestionInput {
                    question_id: "q1".into(),
                    sequence: 1,
                    source_tex_path: "main.tex".into(),
                    source_tex: r#"\documentclass[answer]{cphos}
\begin{document}
\begin{problem}[40]{样题}
    \begin{problemstatement}
        题干
    \end{problemstatement}
    \begin{solution}
        解答
    \end{solution}
\end{problem}
\end{document}"#
                        .into(),
                    assets: vec![],
                }],
            },
        )
        .expect("paper should render");

        assert!(rendered.main_tex.contains("\n\\begin{solution}\n"));
        assert!(rendered.main_tex.contains("\n\\end{solution}\n"));
        assert!(!rendered.main_tex.contains("\n    \\begin{solution}\n"));
        assert!(!rendered.main_tex.contains("\n    \\end{solution}\n"));
    }

    #[test]
    fn render_with_template_rejects_missing_problem_block() {
        let err = render_with_template(
            sample_template(),
            "test-template.tex",
            RenderPaperInput {
                title: "竞赛试卷".into(),
                subtitle: "理论部分".into(),
                authors: vec![],
                reviewers: vec![],
                template_kind: PaperTemplateKind::Theory,
                questions: vec![RenderQuestionInput {
                    question_id: "q1".into(),
                    sequence: 1,
                    source_tex_path: "broken.tex".into(),
                    source_tex: r"\documentclass{cphos}\begin{document}\end{document}".into(),
                    assets: vec![],
                }],
            },
        )
        .expect_err("missing problem block should fail");

        assert!(err.to_string().contains("extract problem block failed"));
    }
}
