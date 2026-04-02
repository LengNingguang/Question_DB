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


def assert_question_query(
    session: TestSession,
    label: str,
    path: str,
    expected_ids: list[str],
) -> None:
    _, body, _ = session.perform_request(label, 200, path=path)
    actual_ids = question_ids_from_body(body)
    session.ensure(
        sorted(actual_ids) == sorted(expected_ids),
        f"{label} should return {expected_ids}, got {actual_ids}",
    )
    session.validation_notes.append(f"{label} -> {actual_ids}")


def upload_and_patch_synthetic_questions(
    session: TestSession,
    zip_paths: list,
    appendix_paths: dict[str, object],
) -> tuple[list[str], dict[str, str]]:
    question_ids: list[str] = []
    question_by_slug: dict[str, str] = {}

    session.multipart_request(
        "POST /questions missing description",
        400,
        path="/questions",
        text_fields=None,
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /questions missing difficulty",
        400,
        path="/questions",
        text_fields={"description": QUESTION_SPECS[0]["create_description"]},
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /questions invalid description",
        400,
        path="/questions",
        text_fields={
            "description": "bad/name",
            "difficulty": json.dumps(
                QUESTION_SPECS[0]["create_difficulty"], ensure_ascii=False
            ),
        },
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /questions invalid difficulty missing human",
        400,
        path="/questions",
        text_fields={
            "description": QUESTION_SPECS[0]["create_description"],
            "difficulty": json.dumps({"heuristic": {"score": 5}}, ensure_ascii=False),
        },
        field_name="file",
        file_path=zip_paths[0],
        content_type="application/zip",
    )
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

    for spec, zip_path in zip(QUESTION_SPECS, zip_paths):
        _, body, _ = session.multipart_request(
            f"POST /questions ({spec['slug']})",
            200,
            path="/questions",
            text_fields={
                "description": spec["create_description"],
                "difficulty": json.dumps(spec["create_difficulty"], ensure_ascii=False),
            },
            field_name="file",
            file_path=zip_path,
            content_type="application/zip",
        )
        response = parse_json(body)
        question_id = response["question_id"]
        session.ensure(
            response["status"] == "imported", "question import should report imported"
        )
        question_ids.append(question_id)
        question_by_slug[spec["slug"]] = question_id

    session.validation_notes.append(
        f"Created synthetic question ids: {question_by_slug}."
    )

    for spec in QUESTION_SPECS:
        question_id = question_by_slug[spec["slug"]]
        session.json_request(
            f"PATCH /questions/{question_id}",
            200,
            method="PATCH",
            path=f"/questions/{question_id}",
            payload=spec["patch"],
        )

    session.json_request(
        f"PATCH /questions/{question_by_slug['mechanics']} invalid difficulty",
        400,
        method="PATCH",
        path=f"/questions/{question_by_slug['mechanics']}",
        payload={"difficulty": {"heuristic": {"score": 5}}},
    )

    _, body, _ = session.perform_request(
        "GET /questions", 200, path="/questions?limit=10&offset=0"
    )
    session.ensure(
        len(parse_json(body)) == 3,
        "question list should contain three synthetic questions",
    )

    assert_question_query(
        session,
        "GET /questions?q=热学&difficulty_tag=human&difficulty_min=5&difficulty_max=5",
        "/questions?q=%E7%83%AD%E5%AD%A6&difficulty_tag=human&difficulty_min=5&difficulty_max=5",
        [question_by_slug["thermal"]],
    )
    assert_question_query(
        session,
        "GET /questions?category=T&tag=mechanics&difficulty_tag=human&difficulty_max=4",
        "/questions?category=T&tag=mechanics&difficulty_tag=human&difficulty_max=4",
        [question_by_slug["mechanics"]],
    )
    assert_question_query(
        session,
        "GET /questions?difficulty_tag=heuristic&difficulty_max=5",
        "/questions?difficulty_tag=heuristic&difficulty_max=5",
        [question_by_slug["mechanics"], question_by_slug["thermal"]],
    )
    assert_question_query(
        session,
        "GET /questions?tag=optics&difficulty_tag=symbolic&difficulty_min=8",
        "/questions?tag=optics&difficulty_tag=symbolic&difficulty_min=8",
        [question_by_slug["optics"]],
    )
    assert_question_query(
        session,
        "GET /questions?difficulty_tag=ml&difficulty_min=8&tag=optics&category=E",
        "/questions?difficulty_tag=ml&difficulty_min=8&tag=optics&category=E",
        [question_by_slug["optics"]],
    )
    # Test score filtering (score is parsed from \begin{problem}[20]{title} in TeX)
    assert_question_query(
        session,
        "GET /questions?score_min=15&score_max=25",
        "/questions?score_min=15&score_max=25",
        [question_by_slug["mechanics"], question_by_slug["optics"], question_by_slug["thermal"]],
    )
    assert_question_query(
        session,
        "GET /questions?score_min=20&score_max=20",
        "/questions?score_min=20&score_max=20",
        [question_by_slug["mechanics"], question_by_slug["optics"], question_by_slug["thermal"]],
    )

    session.perform_request(
        "GET /questions invalid difficulty range without tag",
        400,
        path="/questions?difficulty_min=5",
    )
    session.perform_request(
        "GET /questions invalid difficulty range order",
        400,
        path="/questions?difficulty_tag=human&difficulty_min=8&difficulty_max=3",
    )

    _, body, _ = session.perform_request(
        "GET /questions/{mechanics}",
        200,
        path=f"/questions/{question_by_slug['mechanics']}",
    )
    mechanics_detail = parse_json(body)
    session.ensure(
        mechanics_detail["difficulty"]["human"]["score"] == 4,
        "mechanics human difficulty should be updated to 4",
    )
    session.ensure(
        mechanics_detail["difficulty"]["heuristic"]["notes"] == "fast estimate",
        "mechanics heuristic notes should round-trip",
    )

    _, body, _ = session.perform_request(
        "GET /questions/{optics}",
        200,
        path=f"/questions/{question_by_slug['optics']}",
    )
    optics_detail = parse_json(body)
    session.ensure(
        optics_detail["difficulty"]["symbolic"]["score"] == 9,
        "optics symbolic difficulty should be present",
    )
    session.ensure(
        optics_detail["difficulty"]["ml"]["notes"] == "vision model struggle",
        "optics ml difficulty notes should round-trip",
    )

    question_bundle_path = session.downloads_dir / "questions_bundle_synthetic.zip"
    question_manifest, question_names = session.binary_json_request(
        "POST /questions/bundles (synthetic)",
        200,
        path="/questions/bundles",
        payload={"question_ids": question_ids},
        output_path=question_bundle_path,
    )
    validate_question_bundle(
        question_manifest, question_names, question_ids, session.ensure
    )
    session.validation_notes.append(
        f"Saved synthetic question bundle zip to {question_bundle_path}."
    )

    exercise_question_file_replacement(
        session,
        zip_paths,
        appendix_paths,
        question_by_slug,
    )

    return question_ids, question_by_slug


def exercise_question_file_replacement(
    session: TestSession,
    zip_paths: list,
    appendix_paths: dict[str, object],
    question_by_slug: dict[str, str],
) -> None:
    mechanics_id = question_by_slug["mechanics"]
    original_spec = QUESTION_SPECS[0]
    replacement_spec = QUESTION_SPECS[1]
    replacement_zip_path = zip_paths[1]

    _, body, _ = session.perform_request(
        "GET /questions/{mechanics} before file replace",
        200,
        path=f"/questions/{mechanics_id}",
    )
    original_detail = parse_json(body)

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
    session.multipart_request(
        "PUT /questions/{mechanics}/file missing file",
        400,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
    )
    session.multipart_request(
        "PUT /questions/{mechanics}/file invalid zip",
        400,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
        field_name="file",
        file_path=session.invalid_paper_upload_path,
        content_type="application/zip",
    )
    session.multipart_request(
        "PUT /questions/{mechanics}/file invalid layout",
        400,
        method="PUT",
        path=f"/questions/{mechanics_id}/file",
        text_fields=None,
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

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
    session.ensure(
        replace_response["status"] == "replaced",
        "question file replace should report replaced",
    )
    session.ensure(
        replace_response["file_name"] == replacement_zip_path.name,
        "question file replace should echo the uploaded file name",
    )
    session.ensure(
        replace_response["source_tex_path"] == replacement_spec["tex_name"],
        "question file replace should report the replacement tex path",
    )
    session.ensure(
        replace_response["imported_assets"] == len(replacement_spec["assets"]),
        "question file replace should report the replacement asset count",
    )

    _, body, _ = session.perform_request(
        "GET /questions/{mechanics} after file replace",
        200,
        path=f"/questions/{mechanics_id}",
    )
    replaced_detail = parse_json(body)
    session.ensure(
        replaced_detail["tex_object_id"] != original_detail["tex_object_id"],
        "question file replace should swap the tex object id",
    )
    session.ensure(
        replaced_detail["source"]["tex"] == replacement_spec["tex_name"],
        "question detail should expose the replacement tex path",
    )
    session.ensure(
        [asset["path"] for asset in replaced_detail["assets"]]
        == sorted(replacement_spec["assets"].keys()),
        "question detail should expose the replacement asset paths",
    )
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

    bundle_path = session.downloads_dir / "questions_bundle_replaced_mechanics.zip"
    manifest, names = session.binary_json_request(
        "POST /questions/bundles (replaced mechanics)",
        200,
        path="/questions/bundles",
        payload={"question_ids": [mechanics_id]},
        output_path=bundle_path,
    )
    validate_question_bundle(manifest, names, [mechanics_id], session.ensure)

    directory = manifest["questions"][0]["directory"]
    replacement_tex_path = f"{directory}/{replacement_spec['tex_name']}"
    session.ensure(
        replacement_tex_path in names,
        "question bundle should include the replacement tex file",
    )
    session.ensure(
        f"{directory}/{original_spec['tex_name']}" not in names,
        "question bundle should no longer include the original tex file",
    )
    for asset_path in replacement_spec["assets"].keys():
        session.ensure(
            f"{directory}/{asset_path}" in names,
            "question bundle should include every replacement asset",
        )

    with zipfile.ZipFile(bundle_path, "r") as archive:
        replacement_tex = archive.read(replacement_tex_path).decode("utf-8")
    session.ensure(
        "Optics setup" in replacement_tex,
        "question bundle should serve the replacement tex content",
    )

    session.validation_notes.append(
        "Question file replacement API covered invalid id, missing file, invalid zip, invalid layout, detail refresh, and bundle round-trip."
    )


def upload_real_questions(
    session: TestSession,
    fixtures: list[RealQuestionFixture],
    *,
    category: str,
    tag: str,
    label_prefix: str,
) -> tuple[list[str], dict[str, str], dict[str, RealQuestionFixture]]:
    question_ids: list[str] = []
    question_by_slug: dict[str, str] = {}
    fixture_by_slug = {fixture.slug: fixture for fixture in fixtures}

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
        session.ensure(
            response["status"] == "imported",
            f"{label_prefix} question import should report imported",
        )
        session.ensure(
            response["imported_assets"] == fixture.asset_count,
            f"{fixture.slug} imported asset count should match fixture contents",
        )
        question_ids.append(question_id)
        question_by_slug[fixture.slug] = question_id

        session.json_request(
            f"PATCH /questions/{question_id} ({fixture.slug})",
            200,
            method="PATCH",
            path=f"/questions/{question_id}",
            payload=fixture.patch,
        )

    first_id = question_ids[0]
    _, body, _ = session.perform_request(
        f"GET /questions/{label_prefix}-1",
        200,
        path=f"/questions/{first_id}",
    )
    first_detail = parse_json(body)
    session.ensure(
        first_detail["category"] == category,
        f"{label_prefix} question should be patched to {category}",
    )
    session.ensure(
        first_detail["status"] in {"reviewed", "used"},
        f"{label_prefix} question should be patched to a publishable status",
    )

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


def upload_real_theory_questions(
    session: TestSession,
    fixtures: list[RealQuestionFixture],
) -> tuple[list[str], dict[str, str], dict[str, RealQuestionFixture]]:
    return upload_real_questions(
        session,
        fixtures,
        category="T",
        tag="real-batch",
        label_prefix="real-theory",
    )


def upload_real_experiment_questions(
    session: TestSession,
    fixtures: list[RealQuestionFixture],
) -> tuple[list[str], dict[str, str], dict[str, RealQuestionFixture]]:
    return upload_real_questions(
        session,
        fixtures,
        category="E",
        tag="real-exp-batch",
        label_prefix="real-experiment",
    )


def exercise_paper_file_replacement(
    session: TestSession,
    appendix_paths: dict[str, object],
    paper_id: str,
) -> object:
    replacement_path = appendix_paths["mock-b"]

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
    session.multipart_request(
        "PUT /papers/{paper}/file missing file",
        400,
        method="PUT",
        path=f"/papers/{paper_id}/file",
        text_fields=None,
    )
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


def run_real_theory_paper_flow(
    session: TestSession,
    appendix_paths: dict[str, object],
    sample_question_by_slug: dict[str, str],
    real_question_ids: list[str],
    real_question_by_slug: dict[str, str],
    real_fixtures_by_slug: dict[str, RealQuestionFixture],
) -> tuple[list[str], list[str]]:
    first_four_real_ids = real_question_ids[:4]
    reversed_first_four_real_ids = list(reversed(first_four_real_ids))

    paper_a_fields = {
        "description": "真实理论联考 A",
        "title": "真实理论联考 A 卷",
        "subtitle": "回归测试 初版",
        "authors": json.dumps(["张三", "李四五"], ensure_ascii=False),
        "reviewers": json.dumps(["王五", "赵六七"], ensure_ascii=False),
        "question_ids": json.dumps(first_four_real_ids, ensure_ascii=False),
    }
    paper_b_fields = {
        "description": "真实理论联考 B",
        "title": "真实理论联考 B 卷",
        "subtitle": "六题完整版",
        "authors": json.dumps(["陈一", "孙二三"], ensure_ascii=False),
        "reviewers": json.dumps(["周四", "吴五六"], ensure_ascii=False),
        "question_ids": json.dumps(real_question_ids, ensure_ascii=False),
    }

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
    session.multipart_request(
        "POST /papers invalid description",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "description": "bad/name"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /papers invalid authors json",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "authors": "not-json"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /papers invalid upload zip",
        400,
        path="/papers",
        text_fields=paper_a_fields,
        field_name="file",
        file_path=session.invalid_paper_upload_path,
        content_type="application/zip",
    )
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
    session.multipart_request(
        "POST /papers mixed category questions",
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
    session.multipart_request(
        "POST /papers question status none",
        400,
        path="/papers",
        text_fields={
            **paper_a_fields,
            "question_ids": json.dumps(
                [
                    sample_question_by_slug["mechanics"],
                    sample_question_by_slug["thermal"],
                ],
                ensure_ascii=False,
            ),
        },
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )

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

    _, body, _ = session.perform_request("GET /papers", 200, path="/papers")
    session.ensure(
        len(parse_json(body)) == 2, "paper list should contain two real papers"
    )

    _, body, _ = session.perform_request(
        "GET /papers?q=完整版",
        200,
        path="/papers?q=%E5%AE%8C%E6%95%B4%E7%89%88",
    )
    session.ensure(paper_b_id in body, "paper subtitle search should return paper B")

    _, body, _ = session.perform_request(
        "GET /papers?category=T&tag=real-batch&q=张三",
        200,
        path="/papers?category=T&tag=real-batch&q=%E5%BC%A0%E4%B8%89",
    )
    session.ensure(paper_a_id in body, "combined paper filters should return paper A")

    _, body, _ = session.perform_request(
        "GET /papers/{paper_a}",
        200,
        path=f"/papers/{paper_a_id}",
    )
    paper_a_detail = parse_json(body)
    session.ensure(
        [item["question_id"] for item in paper_a_detail["questions"]]
        == first_four_real_ids,
        "paper A should preserve its initial real question order",
    )

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
            "question_ids": reversed_first_four_real_ids,
        },
    )
    patched_paper_a = parse_json(body)
    session.ensure(
        patched_paper_a["title"] == "真实理论联考 A 卷（修订）",
        "paper patch should update the title",
    )

    session.json_request(
        f"PATCH /papers/{paper_a_id} invalid description",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={"description": "bad/name"},
    )
    session.json_request(
        f"PATCH /papers/{paper_a_id} invalid question_ids",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={"question_ids": []},
    )
    session.json_request(
        f"PATCH /papers/{paper_a_id} mixed category",
        400,
        method="PATCH",
        path=f"/papers/{paper_a_id}",
        payload={
            "question_ids": [real_question_ids[0], sample_question_by_slug["optics"]]
        },
    )

    _, body, _ = session.perform_request(
        "GET /papers/{paper_a} after patch",
        200,
        path=f"/papers/{paper_a_id}",
    )
    paper_a_detail = parse_json(body)
    session.ensure(
        paper_a_detail["authors"] == ["张三", "赵八九"],
        "paper patch should update authors",
    )
    session.ensure(
        paper_a_detail["reviewers"] == ["王五", "孙二"],
        "paper patch should update reviewers",
    )
    session.ensure(
        [item["question_id"] for item in paper_a_detail["questions"]]
        == reversed_first_four_real_ids,
        "paper patch should update question order",
    )

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

    replaced_appendix_path = exercise_paper_file_replacement(
        session,
        appendix_paths,
        paper_a_id,
    )

    paper_bundle_path = session.downloads_dir / "papers_bundle_real_theory.zip"
    paper_manifest, paper_names = session.binary_json_request(
        "POST /papers/bundles (real theory)",
        200,
        path="/papers/bundles",
        payload={"paper_ids": paper_ids},
        output_path=paper_bundle_path,
    )

    asset_count_by_id = {
        real_question_by_slug[fixture.slug]: fixture.asset_count
        for fixture in real_fixtures_by_slug.values()
    }
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
        "CPHOS-Latex/theory/examples/example-paper.tex",
        "T",
        "太阳物理初步",
        session.ensure,
    )
    session.validation_notes.append(
        f"Saved real theory paper bundle zip to {paper_bundle_path}."
    )

    return paper_ids, [*real_question_ids]


def run_real_experiment_paper_flow(
    session: TestSession,
    appendix_paths: dict[str, object],
    sample_question_by_slug: dict[str, str],
    real_question_ids: list[str],
    real_question_by_slug: dict[str, str],
    real_fixtures_by_slug: dict[str, RealQuestionFixture],
) -> tuple[list[str], list[str]]:
    first_three_real_ids = real_question_ids[:3]
    reversed_first_three_real_ids = list(reversed(first_three_real_ids))

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
    session.multipart_request(
        "POST /papers (experiment) invalid description",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "description": "bad/name"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /papers (experiment) invalid authors json",
        400,
        path="/papers",
        text_fields={**paper_a_fields, "authors": "not-json"},
        field_name="file",
        file_path=appendix_paths["mock-a"],
        content_type="application/zip",
    )
    session.multipart_request(
        "POST /papers (experiment) invalid upload zip",
        400,
        path="/papers",
        text_fields=paper_a_fields,
        field_name="file",
        file_path=session.invalid_paper_upload_path,
        content_type="application/zip",
    )
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

    _, body, _ = session.perform_request("GET /papers", 200, path="/papers")
    paper_list = parse_json(body)
    all_paper_ids = {item["paper_id"] for item in paper_list}
    session.ensure(
        paper_a_id in all_paper_ids and paper_b_id in all_paper_ids,
        "paper list should include experiment papers",
    )

    _, body, _ = session.perform_request(
        "GET /papers?q=四题完整版",
        200,
        path="/papers?q=%E5%9B%9B%E9%A2%98%E5%AE%8C%E6%95%B4%E7%89%88",
    )
    session.ensure(
        paper_b_id in body, "experiment paper subtitle search should return paper B"
    )

    _, body, _ = session.perform_request(
        "GET /papers?category=E&tag=real-exp-batch&q=钱二",
        200,
        path="/papers?category=E&tag=real-exp-batch&q=%E9%92%B1%E4%BA%8C",
    )
    session.ensure(
        paper_a_id in body, "combined experiment paper filters should return paper A"
    )

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

    paper_bundle_path = session.downloads_dir / "papers_bundle_real_experiment.zip"
    paper_manifest, paper_names = session.binary_json_request(
        "POST /papers/bundles (real experiment)",
        200,
        path="/papers/bundles",
        payload={"paper_ids": paper_ids},
        output_path=paper_bundle_path,
    )

    asset_count_by_id = {
        real_question_by_slug[fixture.slug]: fixture.asset_count
        for fixture in real_fixtures_by_slug.values()
    }
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
        "CPHOS-Latex/experiment/examples/example-paper.tex",
        "E",
        "弗兰克-赫兹实验",
        session.ensure,
    )
    session.validation_notes.append(
        f"Saved real experiment paper bundle zip to {paper_bundle_path}."
    )

    return paper_ids, [*real_question_ids]


def run_ops_and_cleanup(
    session: TestSession,
    paper_ids: list[str],
    created_question_ids: list[str],
    synthetic_question_ids: list[str],
    expected_exported_questions: int,
) -> None:
    _, body, _ = session.json_request(
        "POST /exports/run",
        200,
        method="POST",
        path="/exports/run",
        payload={
            "format": "jsonl",
            "public": False,
            "output_path": str(session.export_path),
        },
    )
    export_response = parse_json(body)
    session.ensure(
        export_response["exported_questions"] == expected_exported_questions,
        "export should include all created questions",
    )

    _, body, _ = session.json_request(
        "POST /quality-checks/run",
        200,
        method="POST",
        path="/quality-checks/run",
        payload={"output_path": str(session.quality_path)},
    )
    quality_response = parse_json(body)
    session.ensure(
        "empty_papers" in quality_response["report"],
        "quality report should include empty_papers",
    )

    for paper_id in reversed(paper_ids):
        session.perform_request(
            f"DELETE /papers/{paper_id}",
            200,
            method="DELETE",
            path=f"/papers/{paper_id}",
        )
    session.perform_request(
        f"GET /papers/{paper_ids[0]} after delete",
        404,
        path=f"/papers/{paper_ids[0]}",
    )

    for question_id in reversed(created_question_ids + synthetic_question_ids):
        session.perform_request(
            f"DELETE /questions/{question_id}",
            200,
            method="DELETE",
            path=f"/questions/{question_id}",
        )
    session.perform_request(
        f"GET /questions/{created_question_ids[0]} after delete",
        404,
        path=f"/questions/{created_question_ids[0]}",
    )

    session.validation_notes.append(
        "Synthetic question CRUD/filter coverage, question/paper file replacement coverage, real-theory and real-experiment paper bundle coverage, export, quality-check, and delete assertions all passed."
    )


def main() -> None:
    session = TestSession()

    def handle_signal(signum: int, _frame) -> None:
        session.cleanup()
        raise SystemExit(128 + signum)

    atexit.register(session.cleanup)
    signal.signal(signal.SIGINT, handle_signal)
    signal.signal(signal.SIGTERM, handle_signal)

    session.prepare_workspace()
    run_status = "passed"
    run_error = None

    try:
        session.print_step("[1/9] Build synthetic and real fixture zips")
        synthetic_zip_paths = build_sample_question_zips(session)
        appendix_paths = build_sample_paper_appendix_zips(session)
        real_theory_fixtures = build_real_theory_question_zips(session)
        real_experiment_fixtures = build_real_experiment_question_zips(session)
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

        session.print_step("[2/9] Start PostgreSQL container")
        session.start_postgres_container()

        session.print_step("[3/9] Apply migration")
        session.apply_migration()

        session.print_step("[4/9] Start API")
        session.start_api()
        session.perform_request("GET /health", 200, path="/health")

        session.print_step("[5/9] Run synthetic question CRUD and bundle checks")
        synthetic_question_ids, synthetic_question_by_slug = (
            upload_and_patch_synthetic_questions(
                session,
                synthetic_zip_paths,
                appendix_paths,
            )
        )

        session.print_step(
            "[6/9] Upload real theory questions and exercise paper flows"
        )
        (
            real_theory_question_ids,
            real_theory_question_by_slug,
            real_theory_fixtures_by_slug,
        ) = upload_real_theory_questions(session, real_theory_fixtures)
        theory_paper_ids, created_real_theory_question_ids = run_real_theory_paper_flow(
            session,
            appendix_paths,
            synthetic_question_by_slug,
            real_theory_question_ids,
            real_theory_question_by_slug,
            real_theory_fixtures_by_slug,
        )

        session.print_step(
            "[7/9] Upload real experiment questions and exercise paper flows"
        )
        (
            real_experiment_question_ids,
            real_experiment_question_by_slug,
            real_experiment_fixtures_by_slug,
        ) = upload_real_experiment_questions(session, real_experiment_fixtures)
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

        all_created_paper_ids = [*theory_paper_ids, *experiment_paper_ids]
        all_created_real_question_ids = [
            *created_real_theory_question_ids,
            *created_real_experiment_question_ids,
        ]

        session.print_step("[8/9] Run ops APIs and delete created data")
        run_ops_and_cleanup(
            session,
            all_created_paper_ids,
            all_created_real_question_ids,
            synthetic_question_ids,
            len(synthetic_question_ids) + len(all_created_real_question_ids),
        )
    except Exception:
        run_status = "failed"
        run_error = traceback.format_exc()
        raise
    finally:
        session.print_step("[9/9] Write markdown report")
        session.write_report(run_status, run_error)


if __name__ == "__main__":
    main()
