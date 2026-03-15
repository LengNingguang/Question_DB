# 字段字典

## papers
- `paper_id`: 试卷唯一 ID，例如 `CPHOS-2024-FINAL`。
- `year`: 年份。
- `stage`: 赛段，如 `preliminary`、`final`、`mock`。
- `title`: 试卷标题。
- `source_pdf_path`: 原始 PDF 相对路径。
- `is_official`: 是否为正式试卷。
- `notes`: 内部备注。

## questions
- `question_id`: 题目唯一 ID。
- `paper_id`: 所属试卷。
- `question_no`: 卷内题号。
- `category`: `theory` 或 `experiment`。
- `latex_body`: LaTeX 题干。
- `plain_text`: 纯文本题干，便于搜索。
- `answer_latex`: LaTeX 答案。
- `answer_text`: 纯文本答案。
- `status`: `raw`、`reviewed`、`published`。
- `source_page_start` / `source_page_end`: 原卷页码。
- `tags_json`: JSON 数组字符串。

## question_assets
- `asset_id`: 资产唯一 ID。
- `kind`: `statement_image`、`answer_image`、`figure`。
- `file_path`: 相对路径。
- `sha256`: 文件哈希，便于校验。
- `caption`: 图注。
- `sort_order`: 同一道题多个图片时的顺序。

## question_stats
- `exam_session`: 统计所属场次，例如 `2024-final-a`。
- `participant_count`: 参与人数。
- `avg_score`: 平均分。
- `score_std`: 分数标准差。
- `full_mark_rate`: 满分率。
- `zero_score_rate`: 零分率。
- `max_score`: 该题满分。
- `min_score`: 该题最低分。
- `stats_source`: 数据来源标签。
- `stats_version`: 统计版本号。

## difficulty_scores
- `manual_level`: 人工难度标签，例如 `easy`、`medium`、`hard`。
- `derived_score`: 规则算法得分，范围 0 到 1。
- `method`: 算法名称。
- `method_version`: 算法版本。
- `confidence`: 置信度。
- `feature_json`: 用于生成难度的统计摘要。
