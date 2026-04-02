# ============================================================
# 文件：scripts/tests/fixtures.py
# 说明：测试夹具构建器
# ============================================================

"""
测试夹具 (Fixture) 构建器

生成测试用的 ZIP 文件，包括合成题目、试卷附录、真实样题
"""

from __future__ import annotations

import json
import re
import zipfile
from dataclasses import dataclass
from pathlib import Path

from .config import REAL_TEST2_ZIP_PATH, REAL_TEST_ZIP_PATH
from .session import TestSession
from .specs import PAPER_APPENDIX_SPECS, QUESTION_SPECS

# ============================================================
# 正则表达式：提取 LaTeX 题目标题
# ============================================================
# 匹配 \begin{problem}[分数]{标题} 中的标题部分
PROBLEM_TITLE_RE = re.compile(
    r"\\begin\{problem\}(?:\[[^\]]*\])?\{(?P<title>[^{}]*)\}"
)


# ============================================================
# 真实题目夹具数据类
# ============================================================
@dataclass
class RealQuestionFixture:
    """
    真实题目夹具的数据结构

    用于存储从 test.zip/test2.zip 提取的题目信息
    """
    # slug: 唯一标识符（如 real-theory-1）
    slug: str

    # upload_path: ZIP 文件上传路径
    upload_path: Path

    # create_description: 创建时的描述
    create_description: str

    # create_difficulty: 创建时的难度定义
    create_difficulty: dict

    # patch: PATCH 请求的元数据
    patch: dict

    # asset_count: 资源文件数量
    asset_count: int

    # title_hint: 从 LaTeX 提取的题目标题
    title_hint: str

    # source_dir_name: 源目录名（如 1, 2, 3）
    source_dir_name: str


# ============================================================
# 构建合成题目 ZIP
# ============================================================
def build_sample_question_zips(session: TestSession) -> list[Path]:
    """
    构建合成题目的 ZIP 文件

    参数:
        session: 测试会话对象

    返回:
        ZIP 文件路径列表
    """
    zip_paths: list[Path] = []

    # 遍历每道题目规格
    for spec in QUESTION_SPECS:
        # 确定 ZIP 文件路径
        zip_path = session.samples_dir / spec["zip_name"]

        # 创建 ZIP 文件
        with zipfile.ZipFile(zip_path, "w") as archive:
            # 写入 TeX 文件
            archive.writestr(spec["tex_name"], spec["tex_body"])
            # 写入空的 assets 目录（ZIP 中空目录需要特殊处理）
            archive.writestr("assets/", b"")
            # 写入每个资源文件
            for asset_path, content in spec["assets"].items():
                archive.writestr(asset_path, content)

        zip_paths.append(zip_path)

        # 注册测试输入（用于报告生成）
        session.register_input(
            {
                "kind": "synthetic_question",  # 类型：合成题目
                "slug": spec["slug"],
                "upload_file": str(zip_path),
                # 记录 ZIP 内的条目
                "zip_entries": [spec["tex_name"], "assets/", *spec["assets"].keys()],
                # 记录创建参数
                "create_difficulty": spec["create_difficulty"],
                "metadata_patch": spec["patch"],
            }
        )

    return zip_paths


# ============================================================
# 构建试卷附录 ZIP
# ============================================================
def build_sample_paper_appendix_zips(session: TestSession) -> dict[str, Path]:
    """
    构建试卷附录的 ZIP 文件

    参数:
        session: 测试会话对象

    返回:
        slug → ZIP 路径的映射
    """
    zip_paths: dict[str, Path] = {}

    # 遍历每个附录规格
    for spec in PAPER_APPENDIX_SPECS:
        # 确定 ZIP 文件路径
        zip_path = session.samples_dir / spec["zip_name"]

        # 创建 ZIP 文件
        with zipfile.ZipFile(zip_path, "w") as archive:
            # 写入每个条目
            for entry_path, content in spec["appendix_entries"].items():
                archive.writestr(entry_path, content)

        zip_paths[spec["slug"]] = zip_path

        # 注册测试输入
        session.register_input(
            {
                "kind": "paper_appendix",  # 类型：试卷附录
                "slug": spec["slug"],
                "upload_file": str(zip_path),
                "zip_entries": list(spec["appendix_entries"].keys()),
            }
        )

    # 创建无效的上传文件（用于测试错误处理）
    # 内容为"not a zip archive"，用于测试非 ZIP 文件的上传
    session.invalid_paper_upload_path.write_text("not a zip archive", encoding="utf-8")
    session.register_input(
        {
            "kind": "invalid_paper_appendix",  # 类型：无效附录
            "upload_file": str(session.invalid_paper_upload_path),
        }
    )

    return zip_paths


# ============================================================
# 构建真实理论题目 ZIP
# ============================================================
def build_real_theory_question_zips(session: TestSession) -> list[RealQuestionFixture]:
    """
    从 test.zip 构建真实理论题目的 ZIP 文件

    test.zip 结构:
        CPHOS2/
          1/
            main.tex
            assets/
          2/
            ...

    参数:
        session: 测试会话对象

    返回:
        RealQuestionFixture 列表
    """
    fixtures = build_real_question_zips(
        session,
        zip_path=REAL_TEST_ZIP_PATH,  # test.zip
        archive_root_name="CPHOS2",  # ZIP 解压后的根目录名
        extracted_root_name="real_theory_source",  # 临时提取目录名
        upload_prefix="real_theory",  # 上传文件前缀
        slug_prefix="real-theory",  # slug 前缀
        kind_label="real_theory_question",  # 类型标签
        description_prefix="真实理论样题",  # 描述前缀
        title_fallback_prefix="theory",  # 标题后备前缀
        category="T",  # 分类：理论
        tag_prefixes=["theory", "real-batch"],  # 标签前缀
        create_notes_prefix="imported from test.zip folder",  # 创建备注前缀
        patch_notes_prefix="real theory fixture",  # 补丁备注前缀
        expected_count=6,  # 期望的题目数量
    )
    return fixtures


# ============================================================
# 构建真实实验题目 ZIP
# ============================================================
def build_real_experiment_question_zips(
    session: TestSession,
) -> list[RealQuestionFixture]:
    """
    从 test2.zip 构建真实实验题目的 ZIP 文件

    参数:
        session: 测试会话对象

    返回:
        RealQuestionFixture 列表
    """
    fixtures = build_real_question_zips(
        session,
        zip_path=REAL_TEST2_ZIP_PATH,  # test2.zip
        archive_root_name="CPHOS4-E",  # ZIP 解压后的根目录名
        extracted_root_name="real_experiment_source",  # 临时提取目录名
        upload_prefix="real_experiment",  # 上传文件前缀
        slug_prefix="real-experiment",  # slug 前缀
        kind_label="real_experiment_question",  # 类型标签
        description_prefix="真实实验样题",  # 描述前缀
        title_fallback_prefix="experiment",  # 标题后备前缀
        category="E",  # 分类：实验
        tag_prefixes=["experiment", "real-exp-batch"],  # 标签前缀
        create_notes_prefix="imported from test2.zip folder",  # 创建备注前缀
        patch_notes_prefix="real experiment fixture",  # 补丁备注前缀
        expected_count=4,  # 期望的题目数量
    )
    return fixtures


# ============================================================
# 构建真实题目 ZIP（通用函数）
# ============================================================
def build_real_question_zips(
    session: TestSession,
    *,
    zip_path: Path,
    archive_root_name: str,
    extracted_root_name: str,
    upload_prefix: str,
    slug_prefix: str,
    kind_label: str,
    description_prefix: str,
    title_fallback_prefix: str,
    category: str,
    tag_prefixes: list[str],
    create_notes_prefix: str,
    patch_notes_prefix: str,
    expected_count: int,
) -> list[RealQuestionFixture]:
    """
    从真实 ZIP 文件构建题目夹具（通用函数）

    参数:
        session: 测试会话对象
        zip_path: 源 ZIP 文件路径（test.zip 或 test2.zip）
        archive_root_name: ZIP 内部的根目录名
        extracted_root_name: 解压后的临时目录名
        upload_prefix: 上传文件前缀
        slug_prefix: slug 前缀
        kind_label: 类型标签
        description_prefix: 描述前缀
        title_fallback_prefix: 标题后备前缀
        category: 分类（T/E）
        tag_prefixes: 标签前缀列表
        create_notes_prefix: 创建备注前缀
        patch_notes_prefix: 补丁备注前缀
        expected_count: 期望的题目数量

    返回:
        RealQuestionFixture 列表
    """
    # 检查 ZIP 文件是否存在
    session.ensure(zip_path.exists(), f"missing test fixture zip: {zip_path}")

    # 创建临时提取目录
    extracted_root = session.tmp_dir / extracted_root_name

    # 解压 ZIP 文件
    with zipfile.ZipFile(zip_path, "r") as archive:
        archive.extractall(extracted_root)

    # 基础目录路径（如 real_theory_source/CPHOS2/）
    base_dir = extracted_root / archive_root_name

    # 检查解压后的目录是否存在
    session.ensure(
        base_dir.exists(), f"expected extracted directory missing: {base_dir}"
    )

    # 创建上传目录
    upload_dir = session.samples_dir / "real_questions"
    upload_dir.mkdir(parents=True, exist_ok=True)

    fixtures: list[RealQuestionFixture] = []

    # 获取所有子目录（按数字排序）
    source_dirs = sorted(
        (path for path in base_dir.iterdir() if path.is_dir()),
        key=lambda path: int(path.name),  # 按目录名数字排序
    )

    # 遍历每个题目目录
    for index, source_dir in enumerate(source_dirs, start=1):
        # TeX 文件路径
        tex_path = source_dir / "main.tex"

        # 检查 main.tex 是否存在
        session.ensure(
            tex_path.exists(), f"real question is missing main.tex: {source_dir}"
        )

        # 读取 TeX 内容
        tex_body = tex_path.read_text(encoding="utf-8", errors="replace")

        # 提取题目标题（如果失败则使用后备标题）
        title_hint = (
            extract_problem_title(tex_body)
            or f"{title_fallback_prefix}-{source_dir.name}"
        )

        # 上传 ZIP 文件路径
        upload_path = upload_dir / f"{upload_prefix}_{source_dir.name}.zip"

        # 资源文件目录
        assets_dir = source_dir / "assets"

        # 获取所有资源文件（递归）
        asset_paths = (
            sorted(path for path in assets_dir.rglob("*") if path.is_file())
            if assets_dir.exists()
            else []
        )

        # 创建上传 ZIP
        with zipfile.ZipFile(upload_path, "w") as archive:
            # 写入 main.tex
            archive.writestr("main.tex", tex_body)
            # 写入空的 assets 目录
            archive.writestr("assets/", b"")
            # 写入每个资源文件（保持相对路径）
            for asset_path in asset_paths:
                archive.write(
                    asset_path,
                    f"assets/{asset_path.relative_to(assets_dir).as_posix()}",
                )

        # 根据索引确定状态（奇数 reviewed，偶数 used）
        status = "reviewed" if index % 2 else "used"

        # 构建夹具对象
        fixture = RealQuestionFixture(
            slug=f"{slug_prefix}-{source_dir.name}",
            upload_path=upload_path,
            create_description=f"{description_prefix} {source_dir.name}",
            create_difficulty={
                "human": {
                    "score": min(10, index + 3),  # 难度随索引递增，最大 10
                    "notes": f"{create_notes_prefix} {source_dir.name}",
                }
            },
            patch={
                "category": category,
                "description": f"{description_prefix} {source_dir.name}",
                "tags": [*tag_prefixes, f"folder-{source_dir.name}"],
                "status": status,
                "difficulty": {
                    "human": {
                        "score": min(10, index + 4),
                        "notes": f"{patch_notes_prefix} {source_dir.name}",
                    },
                    "heuristic": {"score": min(10, index + 2)},
                },
            },
            asset_count=len(asset_paths),
            title_hint=title_hint,
            source_dir_name=source_dir.name,
        )

        fixtures.append(fixture)

        # 注册测试输入
        session.register_input(
            {
                "kind": kind_label,
                "slug": fixture.slug,
                "source_dir": fixture.source_dir_name,
                "title_hint": fixture.title_hint,
                "upload_file": str(fixture.upload_path),
                "asset_count": fixture.asset_count,
                "create_difficulty": fixture.create_difficulty,
                "metadata_patch": fixture.patch,
            }
        )

    # 验证题目数量是否符合预期
    session.ensure(
        len(fixtures) == expected_count,
        f"expected {expected_count} fixtures from {zip_path.name}, got {len(fixtures)}",
    )

    return fixtures


# ============================================================
# 提取 LaTeX 题目标题
# ============================================================
def extract_problem_title(tex_body: str) -> str | None:
    """
    从 LaTeX 内容中提取 problem 环境的标题

    匹配模式:
        \begin{problem}[20]{标题}
        \begin{problem}{标题}

    参数:
        tex_body: LaTeX 文档内容

    返回:
        题目标题，如果未找到则返回 None
    """
    # 搜索匹配
    match = PROBLEM_TITLE_RE.search(tex_body)

    # 未找到匹配
    if not match:
        return None

    # 获取标题组并去除首尾空格
    title = match.group("title").strip()

    # 返回非空标题
    return title or None


"""
============================================================
知识点讲解 (Python 测试夹具)
============================================================

1. dataclass 数据类
   @dataclass
   class RealQuestionFixture:
       slug: str
       upload_path: Path
       ...

   - 自动生成__init__、__repr__等方法
   - 类型注解定义字段
   - 比普通字典更结构化和安全

2. 类型注解
   list[Path], dict[str, Path], str | None
   - Python 3.9+ 支持内置泛型
   - | 表示联合类型（Union）

3. zipfile 模块
   ZipFile(path, "w") 创建/写入
   ZipFile(path, "r") 读取
   writestr(name, data) 写入字符串
   write(src, arcname) 添加文件
   extractall(dest) 解压所有

4. 正则表达式命名组
   (?P<title>[^{}]*)
   - ?P<name> 定义命名组
   - match.group("title") 获取匹配

5. 路径操作
   path.relative_to(base) 计算相对路径
   path.as_posix() 转为 POSIX 格式（/分隔）
   path.rglob("*") 递归匹配所有文件

============================================================
ZIP 文件结构设计
============================================================

合成题目 ZIP:
├── mechanics.tex          # TeX 源文件
└── assets/                # 资源目录
    ├── diagram.txt
    └── data.csv

真实题目 ZIP:
├── main.tex               # TeX 源文件（固定名称）
└── assets/
    ├── figure1.png
    └── data/
        └── table.csv

试卷附录 ZIP:
├── meta/
│   └── info.json
└── drafts/
    └── notes.txt

============================================================
真实题目处理流程
============================================================

1. 解压 test.zip
   CPHOS2/1/, CPHOS2/2/, ...

2. 遍历子目录（按数字排序）
   1 → 2 → 3 → 4 → 5 → 6

3. 每个目录提取:
   - main.tex 内容
   - assets/下所有文件
   - problem 环境标题

4. 重新打包为上传 ZIP
   real_questions/real_theory_1.zip

5. 构建夹具对象记录元数据

============================================================
测试夹具的意义
============================================================

Fixture（夹具）是测试中的概念：
- 准备测试所需的环境和数据
- 可重复使用
- 隔离测试依赖

本文件构建三类夹具：
1. 合成题目：完全由代码生成，可控性强
2. 试卷附录：简单的多文件 ZIP
3. 真实题目：从现有 test.zip 提取，更接近生产数据
"""
