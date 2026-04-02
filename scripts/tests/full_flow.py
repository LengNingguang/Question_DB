# ============================================================
# 文件：scripts/tests/full_flow.py
# 说明：E2E 完整测试流程
# ============================================================

"""
E2E 完整测试流程

执行完整的端到端测试，覆盖：
1. 题目 CRUD（创建、读取、更新、删除）
2. 试卷 CRUD
3. 题目/试卷文件替换
4. Bundle 下载
5. 数据导出（JSONL/CSV）
6. 质量检查
7. 查询参数验证

测试流程分为 9 个步骤：
[1/9] 构建合成和真实夹具 ZIP
[2/9] 启动 PostgreSQL 容器
[3/9] 应用数据库迁移
[4/9] 启动 API 服务
[5/9] 运行合成题目 CRUD 和 Bundle 检查
[6/9] 上传真实理论题并运行试卷流程
[7/9] 上传真实实验题并运行试卷流程
[8/9] 运行 Ops API 并删除创建的数据
[9/9] 生成 Markdown 报告
"""

from __future__ import annotations

import atexit
import json
import signal
import traceback
import urllib.parse
import zipfile

from .fixtures import (
    RealQuestionFixture,
    build_real_experiment_question_zips,
    build_real_theory_question_zips,
    build_sample_paper_appendix_zips,
    build_sample_question_zips,
)
from .session import TestSession, parse_json, question_ids_from_body
from .specs import QUESTION_SPECS
from .validators import validate_paper_bundle, validate_question_bundle


# ============================================================
# 断言题目查询结果
# ============================================================
def assert_question_query(
    session: TestSession,
    label: str,
    path: str,
    expected_ids: list[str],
) -> None:
    """
    断言题目查询返回预期的题目 ID

    参数:
        session: 测试会话
        label: 测试标签
        path: 请求路径（包含查询参数）
        expected_ids: 期望的题目 ID 列表
    """
    # 发送请求获取响应体
    _, body, _ = session.perform_request(label, 200, path=path)

    # 从响应体提取题目 ID
    actual_ids = question_ids_from_body(body)

    # 验证 ID 列表一致（排序后比较，忽略顺序）
    session.ensure(
        sorted(actual_ids) == sorted(expected_ids),
        f"{label} should return {expected_ids}, got {actual_ids}",
    )

    # 记录验证笔记
    session.validation_notes.append(f"{label} -> {actual_ids}")


# ============================================================
# 上传和更新合成题目
# ============================================================
def upload_and_patch_synthetic_questions(
    session: TestSession,
    zip_paths: list,
    appendix_paths: dict[str, object],
) -> tuple[list[str], dict[str, str]]:
    """
    上传合成题目并测试 PATCH 更新

    测试内容:
    1. 错误处理（缺少参数、无效参数）
    2. 成功创建题目
    3. PATCH 更新元数据
    4. 查询参数验证
    5. Bundle 下载

    参数:
        session: 测试会话
        zip_paths: 合成题目 ZIP 路径列表
        appendix_paths: 试卷附录路径映射

    返回:
        (question_ids, question_by_slug) 元组
        - question_ids: 题目 ID 列表
        - question_by_slug: slug → ID 映射
    """
    question_ids: list[str] = []
    question_by_slug: dict[str, str] = {}

    # ========== 测试错误处理 ==========

    # 错误 1: 缺少 description 字段
    session.multipart_request(
        "POST /questions missing description",
        400,  # 期望 400 Bad Request
        path="/questions",
        text_fields=None,
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )

    # 错误 2: 缺少 difficulty 字段
    session.multipart_request(
        "POST /questions missing difficulty",
        400,
        path="/questions",
        text_fields={"description": QUESTION_SPECS[0]["create_description"]},
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )

    # 错误 3: description 包含非法字符（/）
    session.multipart_request(
        "POST /questions invalid description",
        400,
        path="/questions",
        text_fields={
            "description": "bad/name",  # 不允许包含/
            "difficulty": json.dumps(
                QUESTION_SPECS[0]["create_difficulty"], ensure_ascii=False
            ),
        },
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )

    # 错误 4: difficulty 缺少必需的 human 字段
    session.multipart_request(
        "POST /questions invalid difficulty missing human",
        400,
        path="/questions",
        text_fields={
            "description": QUESTION_SPECS[0]["create_description"],
            # 缺少 human 评估
            "difficulty": json.dumps({"heuristic": {"score": 5}}, ensure_ascii=False),
        },
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )

    # 错误 5: difficulty 分数超出范围（1-10）
    session.multipart_request(
        "POST /questions invalid difficulty score",
        400,
        path="/questions",
        text_fields={
            "description": QUESTION_SPECS[0]["create_description"],
            "difficulty": json.dumps({"human": {"score": 11}}, ensure_ascii=False),
        },
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )

    # ========== 成功创建题目 ==========

    # 遍历每个题目规格并创建
    for spec, zip_path in zip(QUESTION_SPECS, zip_paths):
        _, body, _ = session.multipart_request(
            f"POST /questions ({spec['slug']})",
            200,  # 期望 200 OK
            path="/questions",
            text_fields={
                "description": spec["create_description"],
                "difficulty": json.dumps(spec["create_difficulty"], ensure_ascii=False),
            },
            field_name="file",
            file_path=zip_path,
            content_type="application/zip",
        )

        # 解析响应
        response = parse_json(body)
        question_id = response["question_id"]

        # 验证状态为"imported"
        session.ensure(
            response["status"] == "imported", "question import should report imported"
        )

        question_ids.append(question_id)
        question_by_slug[spec["slug"]] = question_id

    session.validation_notes.append(
        f"Created synthetic question ids: {question_by_slug}."
    )

    # ========== PATCH 更新元数据 ==========

    # 对每道题目执行 PATCH 更新
    for spec in QUESTION_SPECS:
        question_id = question_by_slug[spec["slug"]]
        session.json_request(
            f"PATCH /questions/{question_id}",
            200,
            method="PATCH",
            path=f"/questions/{question_id}",
            payload=spec["patch"],  # 使用规格中定义的 patch 数据
        )

    # 测试 PATCH 错误：缺少 human 的难度
    session.json_request(
        f"PATCH /questions/{question_by_slug['mechanics']} invalid difficulty",
        400,
        method="PATCH",
        path=f"/questions/{question_by_slug['mechanics']}",
        payload={"difficulty": {"heuristic": {"score": 5}}},  # 缺少 human
    )

    # ========== 测试查询参数 ==========

    # 测试列表接口
    _, body, _ = session.perform_request(
        "GET /questions", 200, path="/questions?limit=10&offset=0"
    )
    session.ensure(
        len(parse_json(body)) == 3,
        "question list should contain three synthetic questions",
    )

    # 测试搜索：热学 + human 难度=5
    assert_question_query(
        session,
        "GET /questions?q=热学&difficulty_tag=human&difficulty_min=5&difficulty_max=5",
        "/questions?q=%E7%83%AD%E5%AD%A6&difficulty_tag=human&difficulty_min=5&difficulty_max=5",
        [question_by_slug["thermal"]],
    )

    # 测试过滤：T 类 + mechanics 标签+human 难度≤4
    assert_question_query(
        session,
        "GET /questions?category=T&tag=mechanics&difficulty_tag=human&difficulty_max=4",
        "/questions?category=T&tag=mechanics&difficulty_tag=human&difficulty_max=4",
        [question_by_slug["mechanics"]],
    )

    # 测试过滤：heuristic 难度≤5
    assert_question_query(
        session,
        "GET /questions?difficulty_tag=heuristic&difficulty_max=5",
        "/questions?difficulty_tag=heuristic&difficulty_max=5",
        [question_by_slug["mechanics"], question_by_slug["thermal"]],
    )

    # 测试过滤：optics 标签+symbolic 难度≥8
    assert_question_query(
        session,
        "GET /questions?tag=optics&difficulty_tag=symbolic&difficulty_min=8",
        "/questions?tag=optics&difficulty_tag=symbolic&difficulty_min=8",
        [question_by_slug["optics"]],
    )

    # 测试组合过滤：ml 难度≥8 + optics 标签+E 类
    assert_question_query(
        session,
        "GET /questions?difficulty_tag=ml&difficulty_min=8&tag=optics&category=E",
        "/questions?difficulty_tag=ml&difficulty_min=8&tag=optics&category=E",
        [question_by_slug["optics"]],
    )

    # ========== 测试查询错误处理 ==========

    # 错误：只有 difficulty_min 没有 difficulty_tag
    session.perform_request(
        "GET /questions invalid difficulty range without tag",
        400,
        path="/questions?difficulty_min=5",
    )

    # 错误：difficulty_min > difficulty_max
    session.perform_request(
        "GET /questions invalid difficulty range order",
        400,
        path="/questions?difficulty_tag=human&difficulty_min=8&difficulty_max=3",
    )

    # ========== 测试题目详情 ==========

    # 获取 mechanics 题目详情
    _, body, _ = session.perform_request(
        "GET /questions/{mechanics}",
        200,
        path=f"/questions/{question_by_slug['mechanics']}",
    )
    mechanics_detail = parse_json(body)

    # 验证 human 难度分数已更新
    session.ensure(
        mechanics_detail["difficulty"]["human"]["score"] == 4,
        "mechanics human difficulty should be updated to 4",
    )

    # 验证 heuristic 备注正确
    session.ensure(
        mechanics_detail["difficulty"]["heuristic"]["notes"] == "fast estimate",
        "mechanics heuristic notes should round-trip",
    )

    # 获取 optics 题目详情
    _, body, _ = session.perform_request(
        "GET /questions/{optics}",
        200,
        path=f"/questions/{question_by_slug['optics']}",
    )
    optics_detail = parse_json(body)

    # 验证 symbolic 难度存在
    session.ensure(
        optics_detail["difficulty"]["symbolic"]["score"] == 9,
        "optics symbolic difficulty should be present",
    )

    # 验证 ml 难度备注正确
    session.ensure(
        optics_detail["difficulty"]["ml"]["notes"] == "vision model struggle",
        "optics ml difficulty notes should round-trip",
    )

    # ========== 测试题目 Bundle 下载 ==========

    question_bundle_path = session.downloads_dir / "questions_bundle_synthetic.zip"
    question_manifest, question_names = session.binary_json_request(
        "POST /questions/bundles (synthetic)",
        200,
        path="/questions/bundles",
        payload={"question_ids": question_ids},
        output_path=question_bundle_path,
    )

    # 验证 Bundle 结构
    validate_question_bundle(
        question_manifest, question_names, question_ids, session.ensure
    )

    session.validation_notes.append(
        f"Saved synthetic question bundle zip to {question_bundle_path}."
    )

    # ========== 测试题目文件替换 ==========

    exercise_question_file_replacement(
        session,
        zip_paths,
        appendix_paths,
        question_by_slug,
    )

    return question_ids, question_by_slug


# ============================================================
# 测试题目文件替换
# ============================================================
def exercise_question_file_replacement(
    session: TestSession,
    zip_paths: list,
    appendix_paths: dict[str, object],
    question_by_slug: dict[str, str],
) -> None:
    """
    测试题目文件替换功能（PUT /questions/{id}/file）

    测试内容:
    1. 错误处理（无效 ID、不存在、缺少文件、无效 ZIP、无效布局）
    2. 成功替换文件
    3. 验证详情更新
    4. 验证 Bundle 内容

    参数:
        session: 测试会话
        zip_paths: 题目 ZIP 路径列表
        appendix_paths: 试卷附录路径映射
        question_by_slug: slug → ID 映射
    """
    # 获取 mechanics 题目 ID 和原始规格
    mechanics_id = question_by_slug["mechanics"]
    original_spec = QUESTION_SPECS[0]
    replacement_spec = QUESTION_SPECS[1]  # 使用 optics 的内容进行替换
    replacement_zip_path = zip_paths[1]

    # ========== 获取替换前的详情 ==========

    _, body, _ = session.perform_request(
        "GET /questions/{mechanics} before file replace",
        200,
        path=f"/questions/{mechanics_id}",
    )
    original_detail = parse_json(body)

    # ========== 测试错误处理 ==========

    # 错误 1: 无效的 UUID 格式
    session.multipart_request(
        "PUT /questions/{invalid}/file",
        400,
        method="PUT",
        path="/questions/not-a-uuid/file",
        text_fields=None,
        field_name="file",
        file_path=replacement_zip_path,
        content_type="application/zip",
    )

    # 错误 2: 题目不存在
    session.multipart_request(
        "PUT /questions/{missing}/file",
        404,
        method="PUT",
        path="/questions/550e8400-e29b-41d4-a716-446655440000/file",
        text_fields=None,
        field_name="file",
        file_path=replacement_zip_path,
        content_type="application/zip",
    )

    # 错误 3: 缺少上传文件
    session.multipart_request(
        "PUT /questions/{mechanics}/file missing file",
        400,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
    )

    # 错误 4: 无效的 ZIP 文件
    session.multipart_request(
        "PUT /questions/{mechanics}/file invalid zip",
        400,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
        field_name="file",
        file_path=session.invalid_paper_upload_path,  # 非 ZIP 文件
        content_type="application/zip",
    )

    # 错误 5: ZIP 布局无效（使用试卷附录 ZIP）
    session.multipart_request(
        "PUT /questions/{mechanics}/file invalid layout",
        400,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
        field_name="file",
        file_path=appendix_paths["mock-a"],  # 试卷附录 ZIP（布局不同）
        content_type="application/zip",
    )

    # ========== 成功替换文件 ==========

    _, body, _ = session.multipart_request(
        "PUT /questions/{mechanics}/file",
        200,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
        field_name="file",
        file_path=replacement_zip_path,
        content_type="application/zip",
    )
    replace_response = parse_json(body)

    # 验证响应状态
    session.ensure(
        replace_response["status"] == "replaced",
        "question file replace should report replaced",
    )

    # 验证返回文件名
    session.ensure(
        replace_response["file_name"] == replacement_zip_path.name,
        "question file replace should echo the uploaded file name",
    )

    # 验证返回 TeX 路径
    session.ensure(
        replace_response["source_tex_path"] == replacement_spec["tex_name"],
        "question file replace should report the replacement tex path",
    )

    # 验证返回资源数量
    session.ensure(
        replace_response["imported_assets"] == len(replacement_spec["assets"]),
        "question file replace should report the replacement asset count",
    )

    # ========== 验证详情更新 ==========

    _, body, _ = session.perform_request(
        "GET /questions/{mechanics} after file replace",
        200,
        path=f"/questions/{mechanics_id}",
    )
    replaced_detail = parse_json(body)

    # 验证 TeX 对象 ID 已更换
    session.ensure(
        replaced_detail["tex_object_id"] != original_detail["tex_object_id"],
        "question file replace should swap the tex object id",
    )

    # 验证 TeX 路径更新
    session.ensure(
        replaced_detail["source"]["tex"] == replacement_spec["tex_name"],
        "question detail should expose the replacement tex path",
    )

    # 验证资源路径更新
    session.ensure(
        [asset["path"] for asset in replaced_detail["assets"]]
        == sorted(replacement_spec["assets"].keys()),
        "question detail should expose the replacement asset paths",
    )

    # 验证元数据保持不变
    session.ensure(
        replaced_detail["category"] == original_spec["patch"]["category"],
        "question file replace should preserve category metadata",
    )
    session.ensure(
        replaced_detail["status"] == original_spec["patch"]["status"],
        "question file replace should preserve status metadata",
    )
    session.ensure(
        replaced_detail["description"] == original_spec["patch"]["description"],
        "question file replace should preserve description metadata",
    )
    session.ensure(
        replaced_detail["tags"] == original_spec["patch"]["tags"],
        "question file replace should preserve tags metadata",
    )

    # ========== 验证 Bundle 内容 ==========

    bundle_path = session.downloads_dir / "questions_bundle_replaced_mechanics.zip"
    manifest, names = session.binary_json_request(
        "POST /questions/bundles (replaced mechanics)",
        200,
        path="/questions/bundles",
        payload={"question_ids": [mechanics_id]},
        output_path=bundle_path,
    )

    # 验证 Bundle 结构
    validate_question_bundle(manifest, names, [mechanics_id], session.ensure)

    # 获取替换后的目录名
    directory = manifest["questions"][0]["directory"]
    replacement_tex_path = f"{directory}/{replacement_spec['tex_name']}"

    # 验证包含替换后的 TeX 文件
    session.ensure(
        replacement_tex_path in names,
        "question bundle should include the replacement tex file",
    )

    # 验证不再包含原始 TeX 文件
    session.ensure(
        f"{directory}/{original_spec['tex_name']}" not in names,
        "question bundle should no longer include the original tex file",
    )

    # 验证包含所有替换资源
    for asset_path in replacement_spec["assets"].keys():
        session.ensure(
            f"{directory}/{asset_path}" in names,
            "question bundle should include every replacement asset",
        )

    # 验证 TeX 内容
    with zipfile.ZipFile(bundle_path, "r") as archive:
        replacement_tex = archive.read(replacement_tex_path).decode("utf-8")
    session.ensure(
        "Optics setup" in replacement_tex,
        "question bundle should serve the replacement tex content",
    )

    session.validation_notes.append(
        "Question file replacement API covered invalid id, missing file, invalid zip, invalid layout, detail refresh, and bundle round-trip."
    )


# ============================================================
# 上传真实题目
# ============================================================
def upload_real_questions(
    session: TestSession,
    fixtures: list[RealQuestionFixture],
    *,
    category: str,
    tag: str,
    label_prefix: str,
) -> tuple[list[str], dict[str, str], dict[str, RealQuestionFixture]]:
    """
    上传真实题目（从 test.zip/test2.zip 提取）

    参数:
        session: 测试会话
        fixtures: 题目夹具列表
        category: 分类（T/E）
        tag: 标签
        label_prefix: 标签前缀

    返回:
        (question_ids, question_by_slug, fixture_by_slug) 元组
    """
    question_ids: list[str] = []
    question_by_slug: dict[str, str] = {}
    fixture_by_slug = {fixture.slug: fixture for fixture in fixtures}

    # ========== 上传每道题目 ==========

    for fixture in fixtures:
        _, body, _ = session.multipart_request(
            f"POST /questions ({fixture.slug})",
            200,
            path="/questions",
            text_fields={
                "description": fixture.create_description,
                "difficulty": json.dumps(fixture.create_difficulty, ensure_ascii=False),
            },
            field_name="file",
            file_path=fixture.upload_path,
            content_type="application/zip",
        )
        response = parse_json(body)
        question_id = response["question_id"]

        # 验证状态为"imported"
        session.ensure(
            response["status"] == "imported",
            f"{label_prefix} question import should report imported",
        )

        # 验证资源数量正确
        session.ensure(
            response["imported_assets"] == fixture.asset_count,
            f"{fixture.slug} imported asset count should match fixture contents",
        )

        question_ids.append(question_id)
        question_by_slug[fixture.slug] = question_id

        # PATCH 更新元数据
        session.json_request(
            f"PATCH /questions/{question_id} ({fixture.slug})",
            200,
            method="PATCH",
            path=f"/questions/{question_id}",
            payload=fixture.patch,
        )

    # ========== 验证第一道题目的详情 ==========

    first_id = question_ids[0]
    _, body, _ = session.perform_request(
        f"GET /questions/{label_prefix}-1",
        200,
        path=f"/questions/{first_id}",
    )
    first_detail = parse_json(body)

    # 验证分类已更新
    session.ensure(
        first_detail["category"] == category,
        f"{label_prefix} question should be patched to {category}",
    )

    # 验证状态为 publishable
    session.ensure(
        first_detail["status"] in {"reviewed", "used"},
        f"{label_prefix} question should be patched to a publishable status",
    )

    # ========== 测试查询 ==========

    assert_question_query(
        session,
        f"GET /questions?category={category}&tag={tag}",
        f"/questions?category={category}&tag={tag}",
        question_ids,
    )

    session.validation_notes.append(
        f"Created {label_prefix} question ids: {question_by_slug}."
    )

    return question_ids, question_by_slug, fixture_by_slug


# ============================================================
# 上传真实理论题目（包装函数）
# ============================================================
def upload_real_theory_questions(
    session: TestSession,
    fixtures: list[RealQuestionFixture],
) -> tuple[list[str], dict[str, str], dict[str, RealQuestionFixture]]:
    """
    上传真实理论题目（包装函数）

    参数:
        session: 测试会话
        fixtures: 题目夹具列表

    返回:
        (question_ids, question_by_slug, fixture_by_slug) 元组
    """
    return upload_real_questions(
        session,
        fixtures,
        category="T",  # 理论题
        tag="real-batch",
        label_prefix="real-theory",
    )


# ============================================================
# 上传真实实验题目（包装函数）
# ============================================================
def upload_real_experiment_questions(
    session: TestSession,
    fixtures: list[RealQuestionFixture],
) -> tuple[list[str], dict[str, str], dict[str, RealQuestionFixture]]:
    """
    上传真实实验题目（包装函数）

    参数:
        session: 测试会话
        fixtures: 题目夹具列表

    返回:
        (question_ids, question_by_slug, fixture_by_slug) 元组
    """
    return upload_real_questions(
        session,
        fixtures,
        category="E",  # 实验题
        tag="real-exp-batch",
        label_prefix="real-experiment",
    )


# ============================================================
# 测试试卷文件替换
# ============================================================
def exercise_paper_file_replacement(
    session: TestSession,
    appendix_paths: dict[str, object],
    paper_id: str,
) -> object:
    """
    测试试卷文件替换功能（PUT /papers/{id}/file）

    参数:
        session: 测试会话
        appendix_paths: 试卷附录路径映射
        paper_id: 试卷 ID

    返回:
        替换后的附录路径
    """
    replacement_path = appendix_paths["mock-b"]

    # ========== 测试错误处理 ==========

    # 错误 1: 无效的 UUID
    session.multipart_request(
        "PUT /papers/{invalid}/file",
        400,
        method="PUT",
        path="/papers/not-a-uuid/file",
        text_fields=None,
        field_name="file",
        file_path=replacement_path,
        content_type="application/zip",
    )

    # 错误 2: 试卷不存在
    session.multipart_request(
        "PUT /papers/{missing}/file",
        404,
        method="PUT",
        path="/papers/550e8400-e29b-41d4-a716-446655440000/file",
        text_fields=None,
        field_name="file",
        file_path=replacement_path,
        content_type="application/zip",
    )

    # 错误 3: 缺少上传文件
    session.multipart_request(
        "PUT /papers/{paper}/file missing file",
        400,
        method="PUT",
        path=f"/papers/{paper_id}/file",
        text_fields=None,
    )

    # 错误 4: 无效的 ZIP
    session.multipart_request(
        "PUT /papers/{paper}/file invalid zip",
        400,
        method="PUT",
        path=f"/papers/{paper_id}/file",
        text_fields=None,
        field_name="file",
        file_path=session.invalid_paper_upload_path,
        content_type="application/zip",
    )

    # ========== 成功替换 ==========

    _, body, _ = session.multipart_request(
        "PUT /papers/{paper}/file",
        200,
        method="PUT",
        path=f"/papers/{paper_id}/file",
        text_fields=None,
        field_name="file",
        file_path=replacement_path,
        content_type="application/zip",
    )
    replace_response = parse_json(body)

    # 验证响应
    session.ensure(
        replace_response["status"] == "replaced",
        "paper file replace should report replaced",
    )
    session.ensure(
        replace_response["file_name"] == replacement_path.name,
        "paper file replace should echo the uploaded file name",
    )

    session.validation_notes.append(
        "Paper file replacement API covered invalid id, missing file, invalid zip, and appendix swap."
    )

    return replacement_path


# ============================================================
# 运行真实理论试卷流程
# ============================================================
def run_real_theory_paper_flow(
    session: TestSession,
    appendix_paths: dict[str, object],
    sample_question_by_slug: dict[str, str],
    real_question_ids: list[str],
    real_question_by_slug: dict[str, str],
    real_fixtures_by_slug: dict[str, RealQuestionFixture],
) -> tuple[list[str], list[str]]:
    """
    运行真实理论试卷的完整流程

    测试内容:
    1. 创建试卷（错误处理 + 成功）
    2. 查询试卷
    3. PATCH 更新试卷
    4. 文件替换
    5. Bundle 下载验证

    参数:
        session: 测试会话
        appendix_paths: 试卷附录路径映射
        sample_question_by_slug: 合成题目 slug→ID 映射
        real_question_ids: 真实题目 ID 列表
        real_question_by_slug: 真实题目 slug→ID 映射
        real_fixtures_by_slug: 真实题目夹具映射

    返回:
        (paper_ids, created_question_ids) 元组
    """
    # 前 4 道真实题目和反转顺序
    first_four_real_ids = real_question_ids[:4]
    reversed_first_four_real_ids = list(reversed(first_four_real_ids))

    # ========== 构建试卷字段 ==========

    # 试卷 A：4 道题
    paper_a_fields = {
        "description": "真实理论联考 A",
        "title": "真实理论联考 A 卷",
        "subtitle": "回归测试 初版",
        "authors": json.dumps(["张三", "李四五"], ensure_ascii=False),
        "reviewers": json.dumps(["王五", "赵六七"], ensure_ascii=False),
        "question_ids": json.dumps(first_four_real_ids, ensure_ascii=False),
    }

    # 试卷 B:6 道题（完整）
    paper_b_fields = {
        "description": "真实理论联考 B",
        "title": "真实理论联考 B 卷",
        "subtitle": "六题完整版",
        "authors": json.dumps(["陈一", "孙二三"], ensure_ascii=False),
        "reviewers": json.dumps(["周四", "吴五六"], ensure_ascii=False),
        "question_ids": json.dumps(real_question_ids, ensure_ascii=False),
    }

    # ========== 测试错误处理 ==========

    # 错误 1: 缺少 title
    session.multipart_request(
        "POST /papers missing title",
        400,
        path="/papers",
        text_fields={
            key: value for key, value in paper_a_fields.items() if key != "title"
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 2: description 包含非法字符
    session.multipart_request(
        "POST /papers invalid description",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "description": "bad/name"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 3: authors JSON 格式无效
    session.multipart_request(
        "POST /papers invalid authors json",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "authors": "not-json"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 4: 无效的 ZIP 文件
    session.multipart_request(
        "POST /papers invalid upload zip",
        400,
        path="/papers",
        text_fields=paper_a_fields,
        field_name="file",
        file_path=session.invalid_paper_upload_path,
        content_type="application/zip",
    )

    # 错误 5: 题目 ID 不存在
    session.multipart_request(
        "POST /papers unknown question_id",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [real_question_ids[0], "550e8400-e29b-41d4-a716-446655440000"],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 6: 混合分类的题目（T 和 E 混用）
    session.multipart_request(
        "POST /papers mixed category questions",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [
                    sample_question_by_slug["mechanics"],  # T 类
                    sample_question_by_slug["optics"],     # E 类
                ],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 7: 题目状态为 none（未审核）
    session.multipart_request(
        "POST /papers question status none",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [
                    sample_question_by_slug["mechanics"],  # reviewed
                    sample_question_by_slug["thermal"],    # none（未审核）
                ],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # ========== 成功创建试卷 ==========

    # 创建试卷 A
    _, body, _ = session.multipart_request(
        "POST /papers (real mock-a)",
        200,
        path="/papers",
        text_fields=paper_a_fields,
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )
    paper_a_id = parse_json(body)["paper_id"]

    # 创建试卷 B
    _, body, _ = session.multipart_request(
        "POST /papers (real mock-b)",
        200,
        path="/papers",
        text_fields=paper_b_fields,
        field_name="file",
        file_path=appendix_paths["mock-b"],
        content_type="application/zip",
    )
    paper_b_id = parse_json(body)["paper_id"]
    paper_ids = [paper_a_id, paper_b_id]

    session.validation_notes.append(f"Created real theory paper ids: {paper_ids}.")

    # ========== 测试试卷查询 ==========

    # 测试列表接口
    _, body, _ = session.perform_request("GET /papers", 200, path="/papers")
    session.ensure(
        len(parse_json(body)) == 2, "paper list should contain two real papers"
    )

    # 测试副标题搜索
    _, body, _ = session.perform_request(
        "GET /papers?q=完整版",
        200,
        path="/papers?q=%E5%AE%8C%E6%95%B4%E7%89%88",
    )
    session.ensure(paper_b_id in body, "paper subtitle search should return paper B")

    # 测试组合过滤
    _, body, _ = session.perform_request(
        "GET /papers?category=T&tag=real-batch&q=张三",
        200,
        path="/papers?category=T&tag=real-batch&q=%E5%BC%A0%E4%B8%89",
    )
    session.ensure(paper_a_id in body, "combined paper filters should return paper A")

    # ========== 测试试卷详情 ==========

    _, body, _ = session.perform_request(
        "GET /papers/{paper_a}",
        200,
        path=f"/papers/{paper_a_id}",
    )
    paper_a_detail = parse_json(body)

    # 验证题目顺序
    session.ensure(
        [item["question_id"] for item in paper_a_detail["questions"]]
        == first_four_real_ids,
        "paper A should preserve its initial real question order",
    )

    # ========== 测试 PATCH 更新 ==========

    _, body, _ = session.json_request(
        f"PATCH /papers/{paper_a_id}",
        200,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={
            "description": "真实理论联考 A（修订）",
            "title": "真实理论联考 A 卷（修订）",
            "subtitle": "回归测试 终版",
            "authors": ["张三", "赵八九"],
            "reviewers": ["王五", "孙二"],
            "question_ids": reversed_first_four_real_ids,  # 反转顺序
        },
    )
    patched_paper_a = parse_json(body)

    # 验证标题更新
    session.ensure(
        patched_paper_a["title"] == "真实理论联考 A 卷（修订）",
        "paper patch should update the title",
    )

    # ========== 测试 PATCH 错误处理 ==========

    # 错误：description 包含非法字符
    session.json_request(
        f"PATCH /papers/{paper_a_id} invalid description",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={"description": "bad/name"},
    )

    # 错误：question_ids 为空
    session.json_request(
        f"PATCH /papers/{paper_a_id} invalid question_ids",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={"question_ids": []},
    )

    # 错误：混合分类
    session.json_request(
        f"PATCH /papers/{paper_a_id} mixed category",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={
            "question_ids": [real_question_ids[0], sample_question_by_slug["optics"]]
        },
    )

    # ========== 验证 PATCH 后的详情 ==========

    _, body, _ = session.perform_request(
        "GET /papers/{paper_a} after patch",
        200,
        path=f"/papers/{paper_a_id}",
    )
    paper_a_detail = parse_json(body)

    # 验证作者更新
    session.ensure(
        paper_a_detail["authors"] == ["张三", "赵八九"],
        "paper patch should update authors",
    )

    # 验证审稿人更新
    session.ensure(
        paper_a_detail["reviewers"] == ["王五", "孙二"],
        "paper patch should update reviewers",
    )

    # 验证题目顺序反转
    session.ensure(
        [item["question_id"] for item in paper_a_detail["questions"]]
        == reversed_first_four_real_ids,
        "paper patch should update question order",
    )

    # ========== 测试通过 paper_id 查询题目 ==========

    assert_question_query(
        session,
        "GET /questions?paper_id={paper_a}",
        f"/questions?paper_id={urllib.parse.quote(paper_a_id)}",
        reversed_first_four_real_ids,
    )

    assert_question_query(
        session,
        "GET /questions?paper_id={paper_b}&tag=real-batch&category=T",
        f"/questions?paper_id={urllib.parse.quote(paper_b_id)}&tag=real-batch&category=T",
        real_question_ids,
    )

    # ========== 测试试卷文件替换 ==========

    replaced_appendix_path = exercise_paper_file_replacement(
        session,
        appendix_paths,
        paper_a_id,
    )

    # ========== 测试试卷 Bundle 下载 ==========

    paper_bundle_path = session.downloads_dir / "papers_bundle_real_theory.zip"
    paper_manifest, paper_names = session.binary_json_request(
        "POST /papers/bundles (real theory)",
        200,
        path="/papers/bundles",
        payload={"paper_ids": paper_ids},
        output_path=paper_bundle_path,
    )

    # 构建资产数量映射
    asset_count_by_id = {
        real_question_by_slug[fixture.slug]: fixture.asset_count
        for fixture in real_fixtures_by_slug.values()
    }

    # 验证 Bundle 结构
    validate_paper_bundle(
        paper_manifest,
        paper_names,
        paper_ids,
        paper_bundle_path,
        {
            paper_a_id: {
                "appendix_path": replaced_appendix_path,
                "title": "真实理论联考 A 卷（修订）",
                "subtitle": "回归测试 终版",
                "authors": ["张三", "赵八九"],
                "reviewers": ["王五", "孙二"],
                "question_ids": reversed_first_four_real_ids,
                "asset_total": sum(
                    asset_count_by_id[question_id]
                    for question_id in reversed_first_four_real_ids
                ),
            },
            paper_b_id: {
                "appendix_path": appendix_paths["mock-b"],
                "title": "真实理论联考 B 卷",
                "subtitle": "六题完整版",
                "authors": ["陈一", "孙二三"],
                "reviewers": ["周四", "吴五六"],
                "question_ids": real_question_ids,
                "asset_total": sum(
                    asset_count_by_id[question_id] for question_id in real_question_ids
                ),
            },
        },
        # 期望的模板来源（理论卷）
        "CPHOS-Latex/theory/examples/example-paper.tex",
        # 期望的分类
        "T",
        # 样本问题标题（用于验证模板被替换）
        "太阳物理初步",
        session.ensure,
    )

    session.validation_notes.append(
        f"Saved real theory paper bundle zip to {paper_bundle_path}."
    )

    return paper_ids, [*real_question_ids]


# ============================================================
# 运行真实实验试卷流程
# ============================================================
def run_real_experiment_paper_flow(
    session: TestSession,
    appendix_paths: dict[str, object],
    sample_question_by_slug: dict[str, str],
    real_question_ids: list[str],
    real_question_by_slug: dict[str, str],
    real_fixtures_by_slug: dict[str, RealQuestionFixture],
) -> tuple[list[str], list[str]]:
    """
    运行真实实验试卷的完整流程

    与 run_real_theory_paper_flow 类似，但针对实验卷（E 类）

    参数:
        session: 测试会话
        appendix_paths: 试卷附录路径映射
        sample_question_by_slug: 合成题目 slug→ID 映射
        real_question_ids: 真实题目 ID 列表
        real_question_by_slug: 真实题目 slug→ID 映射
        real_fixtures_by_slug: 真实题目夹具映射

    返回:
        (paper_ids, created_question_ids) 元组
    """
    # 前 3 道真实题目和反转顺序
    first_three_real_ids = real_question_ids[:3]
    reversed_first_three_real_ids = list(reversed(first_three_real_ids))

    # ========== 构建试卷字段 ==========

    paper_a_fields = {
        "description": "真实实验联考 A",
        "title": "真实实验联考 A 卷",
        "subtitle": "回归测试 初版",
        "authors": json.dumps(["钱二", "郑八九"], ensure_ascii=False),
        "reviewers": json.dumps(["韩三", "卫四五"], ensure_ascii=False),
        "question_ids": json.dumps(first_three_real_ids, ensure_ascii=False),
    }

    paper_b_fields = {
        "description": "真实实验联考 B",
        "title": "真实实验联考 B 卷",
        "subtitle": "四题完整版",
        "authors": json.dumps(["高一", "冯二三"], ensure_ascii=False),
        "reviewers": json.dumps(["魏四", "沈五六"], ensure_ascii=False),
        "question_ids": json.dumps(real_question_ids, ensure_ascii=False),
    }

    # ========== 测试错误处理 ==========

    # 错误 1: 缺少 title
    session.multipart_request(
        "POST /papers (experiment) missing title",
        400,
        path="/papers",
        text_fields={
            key: value for key, value in paper_a_fields.items() if key != "title"
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 2: description 包含非法字符
    session.multipart_request(
        "POST /papers (experiment) invalid description",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "description": "bad/name"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 3: authors JSON 格式无效
    session.multipart_request(
        "POST /papers (experiment) invalid authors json",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "authors": "not-json"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 4: 无效的 ZIP 文件
    session.multipart_request(
        "POST /papers (experiment) invalid upload zip",
        400,
        path="/papers",
        text_fields=paper_a_fields,
        field_name="file",
        file_path=session.invalid_paper_upload_path,
        content_type="application/zip",
    )

    # 错误 5: 题目 ID 不存在
    session.multipart_request(
        "POST /papers (experiment) unknown question_id",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [real_question_ids[0], "550e8400-e29b-41d4-a716-446655440000"],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 6: 混合分类的题目
    session.multipart_request(
        "POST /papers (experiment) mixed category questions",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [
                    sample_question_by_slug["mechanics"],
                    sample_question_by_slug["optics"],
                ],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # 错误 7: 题目状态为 none
    session.multipart_request(
        "POST /papers (experiment) question status none",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [sample_question_by_slug["optics"], sample_question_by_slug["thermal"]],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

    # ========== 成功创建试卷 ==========

    # 创建试卷 A
    _, body, _ = session.multipart_request(
        "POST /papers (experiment mock-a)",
        200,
        path="/papers",
        text_fields=paper_a_fields,
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )
    paper_a_id = parse_json(body)["paper_id"]

    # 创建试卷 B
    _, body, _ = session.multipart_request(
        "POST /papers (experiment mock-b)",
        200,
        path="/papers",
        text_fields=paper_b_fields,
        field_name="file",
        file_path=appendix_paths["mock-b"],
        content_type="application/zip",
    )
    paper_b_id = parse_json(body)["paper_id"]
    paper_ids = [paper_a_id, paper_b_id]

    session.validation_notes.append(f"Created real experiment paper ids: {paper_ids}.")

    # ========== 测试试卷查询 ==========

    # 测试列表接口
    _, body, _ = session.perform_request("GET /papers", 200, path="/papers")
    paper_list = parse_json(body)
    all_paper_ids = {item["paper_id"] for item in paper_list}

    session.ensure(
        paper_a_id in all_paper_ids and paper_b_id in all_paper_ids,
        "paper list should include experiment papers",
    )

    # 测试副标题搜索
    _, body, _ = session.perform_request(
        "GET /papers?q=四题完整版",
        200,
        path="/papers?q=%E5%9B%9B%E9%A2%98%E5%AE%8C%E6%95%B4%E7%89%88",
    )
    session.ensure(
        paper_b_id in body, "experiment paper subtitle search should return paper B"
    )

    # 测试组合过滤
    _, body, _ = session.perform_request(
        "GET /papers?category=E&tag=real-exp-batch&q=钱二",
        200,
        path="/papers?category=E&tag=real-exp-batch&q=%E9%92%B1%E4%BA%8C",
    )
    session.ensure(
        paper_a_id in body, "combined experiment paper filters should return paper A"
    )

    # ========== 测试试卷详情 ==========

    _, body, _ = session.perform_request(
        "GET /papers/{experiment-paper_a}",
        200,
        path=f"/papers/{paper_a_id}",
    )
    paper_a_detail = parse_json(body)

    session.ensure(
        [item["question_id"] for item in paper_a_detail["questions"]]
        == first_three_real_ids,
        "experiment paper A should preserve its initial question order",
    )

    # ========== 测试 PATCH 更新 ==========

    _, body, _ = session.json_request(
        f"PATCH /papers/{paper_a_id} (experiment)",
        200,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={
            "description": "真实实验联考 A（修订）",
            "title": "真实实验联考 A 卷（修订）",
            "subtitle": "回归测试 终版",
            "authors": ["钱二", "齐一一"],
            "reviewers": ["韩三", "曹二"],
            "question_ids": reversed_first_three_real_ids,
        },
    )
    patched_paper_a = parse_json(body)

    session.ensure(
        patched_paper_a["title"] == "真实实验联考 A 卷（修订）",
        "experiment paper patch should update the title",
    )

    # ========== 测试 PATCH 错误处理 ==========

    session.json_request(
        f"PATCH /papers/{paper_a_id} (experiment) invalid description",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={"description": "bad/name"},
    )

    session.json_request(
        f"PATCH /papers/{paper_a_id} (experiment) invalid question_ids",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={"question_ids": []},
    )

    session.json_request(
        f"PATCH /papers/{paper_a_id} (experiment) mixed category",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={
            "question_ids": [real_question_ids[0], sample_question_by_slug["mechanics"]]
        },
    )

    # ========== 验证 PATCH 后的详情 ==========

    _, body, _ = session.perform_request(
        "GET /papers/{experiment-paper_a} after patch",
        200,
        path=f"/papers/{paper_a_id}",
    )
    paper_a_detail = parse_json(body)

    session.ensure(
        paper_a_detail["authors"] == ["钱二", "齐一一"],
        "experiment paper patch should update authors",
    )

    session.ensure(
        paper_a_detail["reviewers"] == ["韩三", "曹二"],
        "experiment paper patch should update reviewers",
    )

    session.ensure(
        [item["question_id"] for item in paper_a_detail["questions"]]
        == reversed_first_three_real_ids,
        "experiment paper patch should update question order",
    )

    # ========== 测试通过 paper_id 查询题目 ==========

    assert_question_query(
        session,
        "GET /questions?paper_id={experiment-paper_a}",
        f"/questions?paper_id={urllib.parse.quote(paper_a_id)}",
        reversed_first_three_real_ids,
    )

    assert_question_query(
        session,
        "GET /questions?paper_id={experiment-paper_b}&tag=real-exp-batch&category=E",
        f"/questions?paper_id={urllib.parse.quote(paper_b_id)}&tag=real-exp-batch&category=E",
        real_question_ids,
    )

    # ========== 测试试卷 Bundle 下载 ==========

    paper_bundle_path = session.downloads_dir / "papers_bundle_real_experiment.zip"
    paper_manifest, paper_names = session.binary_json_request(
        "POST /papers/bundles (real experiment)",
        200,
        path="/papers/bundles",
        payload={"paper_ids": paper_ids},
        output_path=paper_bundle_path,
    )

    # 构建资产数量映射
    asset_count_by_id = {
        real_question_by_slug[fixture.slug]: fixture.asset_count
        for fixture in real_fixtures_by_slug.values()
    }

    # 验证 Bundle 结构
    validate_paper_bundle(
        paper_manifest,
        paper_names,
        paper_ids,
        paper_bundle_path,
        {
            paper_a_id: {
                "appendix_path": appendix_paths["mock-a"],
                "title": "真实实验联考 A 卷（修订）",
                "subtitle": "回归测试 终版",
                "authors": ["钱二", "齐一一"],
                "reviewers": ["韩三", "曹二"],
                "question_ids": reversed_first_three_real_ids,
                "asset_total": sum(
                    asset_count_by_id[question_id]
                    for question_id in reversed_first_three_real_ids
                ),
            },
            paper_b_id: {
                "appendix_path": appendix_paths["mock-b"],
                "title": "真实实验联考 B 卷",
                "subtitle": "四题完整版",
                "authors": ["高一", "冯二三"],
                "reviewers": ["魏四", "沈五六"],
                "question_ids": real_question_ids,
                "asset_total": sum(
                    asset_count_by_id[question_id] for question_id in real_question_ids
                ),
            },
        },
        # 期望的模板来源（实验卷）
        "CPHOS-Latex/experiment/examples/example-paper.tex",
        # 期望的分类
        "E",
        # 样本问题标题
        "弗兰克 - 赫兹实验",
        session.ensure,
    )

    session.validation_notes.append(
        f"Saved real experiment paper bundle zip to {paper_bundle_path}."
    )

    return paper_ids, [*real_question_ids]


# ============================================================
# 运行 Ops API 和清理
# ============================================================
def run_ops_and_cleanup(
    session: TestSession,
    paper_ids: list[str],
    created_question_ids: list[str],
    synthetic_question_ids: list[str],
    expected_exported_questions: int,
) -> None:
    """
    运行 Ops API 并清理测试数据

    测试内容:
    1. 导出 API（JSONL 格式）
    2. 质量检查 API
    3. 删除试卷
    4. 删除题目

    参数:
        session: 测试会话
        paper_ids: 试卷 ID 列表
        created_question_ids: 创建的真实题目 ID 列表
        synthetic_question_ids: 合成题目 ID 列表
        expected_exported_questions: 期望导出的题目数量
    """
    # ========== 测试导出 API ==========

    _, body, _ = session.json_request(
        "POST /exports/run",
        200,
        method="POST",
        path="/exports/run",
        payload={
            "format": "jsonl",       # 导出格式
            "public": False,         # 内部版本（包含 TeX 源码）
            "output_path": str(session.export_path),
        },
    )
    export_response = parse_json(body)

    # 验证导出数量
    session.ensure(
        export_response["exported_questions"] == expected_exported_questions,
        "export should include all created questions",
    )

    # ========== 测试质量检查 API ==========

    _, body, _ = session.json_request(
        "POST /quality-checks/run",
        200,
        method="POST",
        path="/quality-checks/run",
        payload={"output_path": str(session.quality_path)},
    )
    quality_response = parse_json(body)

    # 验证报告包含 empty_papers 字段
    session.ensure(
        "empty_papers" in quality_response["report"],
        "quality report should include empty_papers",
    )

    # ========== 删除试卷（逆序） ==========

    for paper_id in reversed(paper_ids):
        session.perform_request(
            f"DELETE /papers/{paper_id}",
            200,
            method="DELETE",
            path=f"/papers/{paper_id}",
        )

    # 验证删除后返回 404
    session.perform_request(
        f"GET /papers/{paper_ids[0]} after delete",
        404,
        path=f"/papers/{paper_ids[0]}",
    )

    # ========== 删除题目（逆序） ==========

    for question_id in reversed(created_question_ids + synthetic_question_ids):
        session.perform_request(
            f"DELETE /questions/{question_id}",
            200,
            method="DELETE",
            path=f"/questions/{question_id}",
        )

    # 验证删除后返回 404
    session.perform_request(
        f"GET /questions/{created_question_ids[0]} after delete",
        404,
        path=f"/questions/{created_question_ids[0]}",
    )

    session.validation_notes.append(
        "Synthetic question CRUD/filter coverage, question/paper file replacement coverage, real-theory and real-experiment paper bundle coverage, export, quality-check, and delete assertions all passed."
    )


# ============================================================
# 主函数
# ============================================================
def main() -> None:
    """
    E2E 测试入口函数

    执行 9 个步骤的完整测试流程：
    1. 构建夹具 ZIP
    2. 启动 PostgreSQL 容器
    3. 应用数据库迁移
    4. 启动 API 服务
    5. 运行合成题目测试
    6. 运行真实理论题测试
    7. 运行真实实验题测试
    8. 运行 Ops API 和清理
    9. 生成 Markdown 报告
    """
    # 创建测试会话
    session = TestSession()

    # ========== 信号处理 ==========

    def handle_signal(signum: int, _frame) -> None:
        """处理 SIGINT/SIGTERM 信号，清理资源后退出"""
        session.cleanup()
        raise SystemExit(128 + signum)

    # 注册退出清理
    atexit.register(session.cleanup)

    # 注册信号处理
    signal.signal(signal.SIGINT, handle_signal)  # Ctrl+C
    signal.signal(signal.SIGTERM, handle_signal)  # kill 命令

    # 准备工作区
    session.prepare_workspace()

    # 初始化状态
    run_status = "passed"
    run_error = None

    try:
        # ========== [1/9] 构建夹具 ZIP ==========

        session.print_step("[1/9] Build synthetic and real fixture zips")

        # 构建合成题目 ZIP（3 道）
        synthetic_zip_paths = build_sample_question_zips(session)

        # 构建试卷附录 ZIP（2 个）
        appendix_paths = build_sample_paper_appendix_zips(session)

        # 构建真实理论题 ZIP（6 道，来自 test.zip）
        real_theory_fixtures = build_real_theory_question_zips(session)

        # 构建真实实验题 ZIP（4 道，来自 test2.zip）
        real_experiment_fixtures = build_real_experiment_question_zips(session)

        # 记录验证笔记
        session.validation_notes.append(
            f"Built {len(synthetic_zip_paths)} synthetic question zips."
        )
        session.validation_notes.append(
            f"Built {len(appendix_paths)} paper appendix zips."
        )
        session.validation_notes.append(
            f"Built {len(real_theory_fixtures)} real theory question zips from test.zip."
        )
        session.validation_notes.append(
            f"Built {len(real_experiment_fixtures)} real experiment question zips from test2.zip."
        )

        # ========== [2/9] 启动 PostgreSQL 容器 ==========

        session.print_step("[2/9] Start PostgreSQL container")
        session.start_postgres_container()

        # ========== [3/9] 应用数据库迁移 ==========

        session.print_step("[3/9] Apply migration")
        session.apply_migration()

        # ========== [4/9] 启动 API 服务 ==========

        session.print_step("[4/9] Start API")
        session.start_api()

        # 验证健康检查
        session.perform_request("GET /health", 200, path="/health")

        # ========== [5/9] 合成题目测试 ==========

        session.print_step("[5/9] Run synthetic question CRUD and bundle checks")
        synthetic_question_ids, synthetic_question_by_slug = (
            upload_and_patch_synthetic_questions(
                session,
                synthetic_zip_paths,
                appendix_paths,
            )
        )

        # ========== [6/9] 真实理论题测试 ==========

        session.print_step(
            "[6/9] Upload real theory questions and exercise paper flows"
        )

        # 上传理论题
        (
            real_theory_question_ids,
            real_theory_question_by_slug,
            real_theory_fixtures_by_slug,
        ) = upload_real_theory_questions(session, real_theory_fixtures)

        # 运行理论卷流程
        theory_paper_ids, created_real_theory_question_ids = run_real_theory_paper_flow(
            session,
            appendix_paths,
            synthetic_question_by_slug,
            real_theory_question_ids,
            real_theory_question_by_slug,
            real_theory_fixtures_by_slug,
        )

        # ========== [7/9] 真实实验题测试 ==========

        session.print_step(
            "[7/9] Upload real experiment questions and exercise paper flows"
        )

        # 上传实验题
        (
            real_experiment_question_ids,
            real_experiment_question_by_slug,
            real_experiment_fixtures_by_slug,
        ) = upload_real_experiment_questions(session, real_experiment_fixtures)

        # 运行实验卷流程
        experiment_paper_ids, created_real_experiment_question_ids = (
            run_real_experiment_paper_flow(
                session,
                appendix_paths,
                synthetic_question_by_slug,
                real_experiment_question_ids,
                real_experiment_question_by_slug,
                real_experiment_fixtures_by_slug,
            )
        )

        # 合并所有创建的 ID
        all_created_paper_ids = [*theory_paper_ids, *experiment_paper_ids]
        all_created_real_question_ids = [
            *created_real_theory_question_ids,
            *created_real_experiment_question_ids,
        ]

        # ========== [8/9] Ops API 和清理 ==========

        session.print_step("[8/9] Run ops APIs and delete created data")
        run_ops_and_cleanup(
            session,
            all_created_paper_ids,
            all_created_real_question_ids,
            synthetic_question_ids,
            len(synthetic_question_ids) + len(all_created_real_question_ids),
        )

    except Exception:
        # 捕获异常，记录状态
        run_status = "failed"
        run_error = traceback.format_exc()
        raise

    finally:
        # ========== [9/9] 生成报告 ==========

        session.print_step("[9/9] Write markdown report")
        session.write_report(run_status, run_error)


if __name__ == "__main__":
    main()
