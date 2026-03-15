# 录入规范

## 1. 先整理原始资料
- 原始 PDF、截图、文本、成绩表全部放入 `raw/`。
- 原始文件不直接修改，只做重命名、登记和备注。

## 2. 建立清洗包
- 每套卷子单独建立一个 bundle。
- bundle 必须包含 `manifest.json` 与 `questions/*.json`。
- 每道题必须有 `question_id`、`question_no`、`category`、`latex_body`、`plain_text`、`status`、`source_pages`、`tags`、`assets`。

## 3. 图片命名
- 统一命名为 `paperid_questionid_scene.ext`。
- 所有图片使用相对路径记录，不写绝对路径。
- 任何录入前先计算 SHA256。

## 4. 导入前检查
- 先运行 `python scripts/validate_bundle.py <bundle>`。
- 再运行 `python scripts/import_bundle.py <bundle>` 做 dry-run。
- 确认无错误后，加 `--commit` 真正写库。

## 5. 成绩统计导入
- 原始成绩表先整理成逐题 CSV。
- CSV 至少要有 `question_id`、`exam_session`、`score`、`max_score`。
- 导入后必须重新跑难度脚本和质量检查。
