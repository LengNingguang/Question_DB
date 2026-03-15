# Raw Materials

`raw/` 在源码仓中仅用于本地开发示例和清洗流程演示。

生产环境中的原始资料应放在服务器本地目录，并通过 `QUESTION_BANK_RAW_DIR` 注入，不直接提交到 Git。

规则：
- 原始文件只做拷贝、归档、编号和备注，不直接修改原始内容。
- 盘点时运行 `python scripts/register_raw_assets.py` 生成 `docs/material_inventory.csv`。
- 清洗后的结构化内容放进 bundle，不直接在 `raw/` 上做二次编辑。
