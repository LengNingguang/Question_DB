# ============================================================
# 文件：scripts/tests/validators.py
# 说明：Bundle 验证函数
# ============================================================

"""
Bundle 验证器

验证 API 导出的 ZIP Bundle 是否符合预期结构
"""

from __future__ import annotations

import io
import zipfile
from pathlib import Path


# ============================================================
# 验证题目 Bundle
# ============================================================
def validate_question_bundle(
    manifest: dict,           # manifest.json 解析后的字典
    names: list[str],         # ZIP 内所有条目名称
    question_ids: list[str],  # 期望的题目 ID 列表
    ensure,                   # 断言函数 (condition, message)
) -> None:
    """
    验证题目 Bundle ZIP 的结构

    验证内容:
    1. manifest.kind == "question_bundle"
    2. 题目数量匹配
    3. 题目 ID 顺序一致
    4. 目录命名规范（以描述开头，不使用原始 ID）
    5. 文件路径正确（在描述目录下）
    6. 包含.tex 文件和 assets

    参数:
        manifest: manifest.json 的解析结果
        names: ZIP 条目列表
        question_ids: 期望的题目 ID
        ensure: 断言函数
    """
    # ========== 验证 manifest 基础字段 ==========
    # 验证种类：必须是 question_bundle
    ensure(
        manifest["kind"] == "question_bundle",
        "question bundle manifest kind mismatch",
    )

    # 验证题目数量：manifest 中的数量必须与期望一致
    ensure(
        manifest["question_count"] == len(question_ids),
        "question bundle count mismatch",
    )

    # ========== 验证题目 ID 顺序 ==========
    # 提取 manifest 中所有题目的 ID
    bundled_ids = [item["question_id"] for item in manifest["questions"]]

    # 验证 ID 列表一致（排序后比较）
    ensure(
        bundled_ids == question_ids,
        "question bundle ids should preserve request order",
    )

    # ========== 验证每个题目的结构 ==========
    for item in manifest["questions"]:
        # 期望的目录前缀：description_
        expected_prefix = f"{item['metadata']['description']}_"

        # 验证目录以描述开头
        ensure(
            item["directory"].startswith(expected_prefix),
            "question bundle directory should start with description",
        )

        # 验证目录不使用原始 ID（安全考虑）
        ensure(
            item["directory"] != item["question_id"],
            "question bundle directory should not use raw question id",
        )

        # 收集所有文件路径
        file_paths = {entry["zip_path"] for entry in item["files"]}

        # 验证所有文件都在描述目录下
        ensure(
            all(path.startswith(f"{item['directory']}/") for path in file_paths),
            "question bundle files should live under the description directory",
        )

        # 验证包含.tex 文件
        ensure(
            any(path.endswith(".tex") for path in file_paths),
            "question bundle should include tex",
        )

        # 验证包含 assets 目录
        ensure(
            any("/assets/" in path for path in file_paths),
            "question bundle should include assets",
        )

        # 验证 manifest 中的路径都存在于 ZIP
        ensure(
            all(path in names for path in file_paths),
            "question bundle manifest paths must exist in zip",
        )


# ============================================================
# 验证试卷 Bundle
# ============================================================
def validate_paper_bundle(
    manifest: dict,                    # manifest.json 解析后的字典
    names: list[str],                  # ZIP 内所有条目名称
    paper_ids: list[str],              # 期望的试卷 ID 列表
    bundle_path: Path,                 # Bundle ZIP 文件路径
    expected_papers: dict[str, dict],  # 期望的试卷详情
    expected_template_source: str,     # 期望的模板来源路径
    expected_category: str,            # 期望的分类（T/E）
    sample_problem_title: str,         # 样本问题标题（用于验证替换）
    ensure,                            # 断言函数
) -> None:
    """
    验证试卷 Bundle ZIP 的结构

    验证内容:
    1. manifest.kind == "paper_bundle"
    2. 试卷数量匹配
    3. 试卷 ID 顺序一致
    4. 模板来源正确
    5. 标题、副标题、作者、审稿人正确
    6. 附录 ZIP 完整
    7. main.tex 渲染正确（包含题目、替换占位符）
    8. 资源文件前缀正确（p1-, p2-, ...）

    参数:
        manifest: manifest.json 的解析结果
        names: ZIP 条目列表
        paper_ids: 期望的试卷 ID
        bundle_path: ZIP 文件路径
        expected_papers: 期望的试卷详情映射
        expected_template_source: 期望的模板来源
        expected_category: 期望的分类
        sample_problem_title: 样本问题标题
        ensure: 断言函数
    """
    # ========== 验证 manifest 基础字段 ==========
    # 验证种类：必须是 paper_bundle
    ensure(
        manifest["kind"] == "paper_bundle",
        "paper bundle manifest kind mismatch",
    )

    # 验证试卷数量
    ensure(
        manifest["paper_count"] == len(paper_ids),
        "paper bundle count mismatch",
    )

    # 验证试卷 ID 顺序
    bundled_ids = [item["paper_id"] for item in manifest["papers"]]
    ensure(
        bundled_ids == paper_ids,
        "paper bundle ids should preserve request order",
    )

    # ========== 打开 ZIP 文件验证内容 ==========
    with zipfile.ZipFile(bundle_path, "r") as archive:
        # 遍历每份试卷
        for item in manifest["papers"]:
            # 获取期望的试卷详情
            expected = expected_papers[item["paper_id"]]

            # 期望的目录前缀
            paper_prefix = f"{item['metadata']['description']}_"

            # 验证目录以描述开头
            ensure(
                item["directory"].startswith(paper_prefix),
                "paper bundle directory should start with description",
            )

            # 验证目录不使用原始 ID
            ensure(
                item["directory"] != item["paper_id"],
                "paper bundle directory should not use raw paper id",
            )

            # 验证模板来源（Theory/Experiment）
            ensure(
                item["template_source"] == expected_template_source,
                "paper bundle should use the expected paper template",
            )

            # ========== 验证元数据 ==========
            # 验证标题
            ensure(
                item["metadata"]["title"] == expected["title"],
                "paper title should round-trip",
            )

            # 验证副标题
            ensure(
                item["metadata"]["subtitle"] == expected["subtitle"],
                "paper subtitle should round-trip",
            )

            # 验证作者列表
            ensure(
                item["metadata"]["authors"] == expected["authors"],
                "paper authors should round-trip",
            )

            # 验证审稿人列表
            ensure(
                item["metadata"]["reviewers"] == expected["reviewers"],
                "paper reviewers should round-trip",
            )

            # ========== 验证附录文件 ==========
            append_file = item["append_file"]

            # 验证文件种类
            ensure(
                append_file["file_kind"] == "appendix",
                "paper appendix file kind mismatch",
            )

            # 验证 ZIP 内路径（重命名为 append.zip）
            ensure(
                append_file["zip_path"] == f"{item['directory']}/append.zip",
                "paper appendix should be renamed to append.zip",
            )

            # 验证原始文件名保留
            ensure(
                append_file["original_path"] == expected["appendix_path"].name,
                "paper appendix manifest should keep original file name",
            )

            # 验证附录存在于 ZIP
            ensure(
                append_file["zip_path"] in names,
                "paper appendix path should exist in bundle",
            )

            # 验证附录内容字节一致
            append_bytes = archive.read(append_file["zip_path"])
            ensure(
                append_bytes == expected["appendix_path"].read_bytes(),
                "paper appendix bytes should round-trip",
            )

            # 验证附录 ZIP 内部条目一致
            with zipfile.ZipFile(expected["appendix_path"], "r") as appendix_archive:
                expected_append_entries = sorted(appendix_archive.namelist())

            with zipfile.ZipFile(io.BytesIO(append_bytes), "r") as appendix_archive:
                ensure(
                    sorted(appendix_archive.namelist()) == expected_append_entries,
                    "paper appendix zip contents should round-trip",
                )

            # ========== 验证 main.tex ==========
            main_tex_file = item["main_tex_file"]

            # 验证 main.tex 路径
            ensure(
                main_tex_file["zip_path"] == f"{item['directory']}/main.tex",
                "paper bundle should expose main.tex at the paper root",
            )

            # 验证 main.tex 存在于 ZIP
            ensure(
                main_tex_file["zip_path"] in names,
                "main.tex should exist in the bundle",
            )

            # 验证模板来源路径
            ensure(
                main_tex_file["original_path"] == expected_template_source,
                "main.tex manifest should record the source template path",
            )

            # 读取并解码 main.tex 内容
            main_tex = archive.read(main_tex_file["zip_path"]).decode("utf-8")

            # 验证包含标题
            ensure(
                f"\\cphostitle{{{expected['title']}}}" in main_tex,
                "rendered main.tex should include the paper title",
            )

            # 验证包含副标题
            ensure(
                f"\\cphossubtitle{{{expected['subtitle']}}}" in main_tex,
                "rendered main.tex should include the paper subtitle",
            )

            # 验证 problem 环境数量与题目数量一致
            ensure(
                main_tex.count("\\begin{problem}") == len(expected["question_ids"]),
                "rendered main.tex should contain one problem block per paper question",
            )

            # 验证不包含模板样本问题标题
            ensure(
                sample_problem_title not in main_tex,
                "rendered main.tex should not keep the template sample problem",
            )

            # 验证作者占位符被替换
            ensure(
                "X~X\\quad XXX\\quad XXX" not in main_tex,
                "rendered main.tex should replace the template author placeholder",
            )

            # 验证审稿人占位符被替换
            ensure(
                "Y~Y\\quad YYY\\quad YYY" not in main_tex,
                "rendered main.tex should replace the template reviewer placeholder",
            )

            # ========== 验证题目列表 ==========
            actual_question_ids = [
                question["question_id"] for question in item["questions"]
            ]

            # 验证题目顺序一致
            ensure(
                actual_question_ids == expected["question_ids"],
                "paper bundle question order should preserve the paper order",
            )

            # 遍历每道题目验证
            for sequence, question in enumerate(item["questions"], start=1):
                # 验证序号从 1 开始递增
                ensure(
                    question["sequence"] == sequence,
                    "paper bundle question sequence should be 1-based and ordered",
                )

                # 验证资源前缀（p1-, p2-, ...）
                ensure(
                    question["asset_prefix"] == f"p{sequence}-",
                    "paper bundle question asset prefix should match the sequence",
                )

                # 验证源 TeX 路径（上传的题目保持 main.tex）
                ensure(
                    question["source_tex_path"] == "main.tex",
                    "uploaded real questions should keep main.tex as source path",
                )

                # 验证分类保持一致
                ensure(
                    question["metadata"]["category"] == expected_category,
                    "real paper bundle questions should keep the expected category",
                )

            # ========== 验证资源文件 ==========
            # 验证资源总数
            ensure(
                len(item["assets"]) == expected["asset_total"],
                "paper bundle merged asset count should match all paper question assets",
            )

            # 验证每个资源文件
            for asset in item["assets"]:
                # 验证资源存在于 ZIP
                ensure(
                    asset["zip_path"] in names,
                    "rendered asset path should exist in bundle",
                )

                # 验证资源在 assets 目录下
                ensure(
                    asset["zip_path"].startswith(f"{item['directory']}/assets/"),
                    "rendered assets should live under the merged assets directory",
                )

                # 验证资源关联到正确的题目
                ensure(
                    asset["source_question_id"] in expected["question_ids"],
                    "rendered asset should point back to one of the paper questions",
                )


"""
============================================================
知识点讲解 (Python 验证器)
============================================================

1. io.BytesIO
   - 内存中的二进制流
   - 用于在不写入磁盘的情况下操作 ZIP 数据
   - zipfile.ZipFile(io.BytesIO(data), "r")

2. 集合推导式
   {entry["zip_path"] for entry in item["files"]}
   - 类似列表推导式，但返回集合
   - 自动去重，支持 in 操作 O(1)

3. any() / all() 内置函数
   any(iterable)  - 任一为真则真
   all(iterable)  - 全部为真则真

   示例:
   any(path.endswith(".tex") for path in file_paths)
   all(path.startswith(prefix) for path in file_paths)

4. 嵌套列表推导式
   [item["paper_id"] for item in manifest["papers"]]
   - 从复杂结构中提取数据
   - 比 for 循环更简洁

5. zipfile 读取
   archive.read(path) 返回 bytes
   archive.namelist() 返回所有条目名

============================================================
验证器设计原则
============================================================

1. 防御性编程
   - 验证所有输入
   - 不信任外部数据（即使是 API 返回）
   - 明确的错误消息

2. 分层验证
   Layer 1: manifest.json 结构
   Layer 2: ZIP 条目存在性
   Layer 3: 文件内容正确性

3. 精确断言
   ❌ "something is wrong"
   ✓ "paper appendix bytes should round-trip"

============================================================
题目 Bundle 验证要点
============================================================

manifest.json 验证:
┌─────────────────────┐
│ kind: question_bundle│
│ question_count: N   │
│ questions: [...]    │
└─────────────────────┘

每个题目验证:
├── directory: description_uuid  ✓
├── question_id: uuid           ✓
├── files:                      ✓
│   ├── {directory}/main.tex   ✓
│   └── {directory}/assets/*   ✓

============================================================
试卷 Bundle 验证要点
============================================================

额外验证项（相比题目 Bundle）:

1. 模板验证:
   - template_source 正确（Theory/Experiment）
   - main.tex 包含\\cphostitle{}
   - main.tex 包含\\cphossubtitle{}

2. 内容渲染验证:
   - problem 环境数量 = 题目数量
   - 模板占位符被替换
   - 样本问题被移除

3. 附录验证:
   - append.zip 内容完整
   - ZIP 条目一致

4. 题目顺序验证:
   - sequence: 1, 2, 3...
   - asset_prefix: p1-, p2-, p3-...

5. 资源合并验证:
   - 所有题目资源在 assets/目录
   - source_question_id 关联正确

============================================================
验证流程对比
============================================================

题目 Bundle 验证 (简单):
1. 检查 manifest 结构
2. 验证题目数量
3. 验证 ID 顺序
4. 验证每个题目的文件

试卷 Bundle 验证 (复杂):
1. 检查 manifest 结构
2. 验证试卷数量
3. 验证 ID 顺序
4. 打开 ZIP 验证每个试卷:
   a. 元数据（标题、作者等）
   b. 附录文件（字节级验证）
   c. main.tex（内容验证）
   d. 题目顺序和序号
   e. 资源文件合并
"""
