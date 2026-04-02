from __future__ import annotations

import json
import re
import zipfile
from dataclasses import dataclass
from pathlib import Path

from .config import REAL_TEST2_ZIP_PATH, REAL_TEST_ZIP_PATH
from .session import TestSession
from .specs import PAPER_APPENDIX_SPECS, QUESTION_SPECS

PROBLEM_TITLE_RE = re.compile(r"\\begin\{problem\}(?:\[[^\]]*\])?\{(?P<title>[^{}]*)\}")


@dataclass
class RealQuestionFixture:
    slug: str
    upload_path: Path
    create_description: str
    create_difficulty: dict
    patch: dict
    asset_count: int
    title_hint: str
    source_dir_name: str


def build_sample_question_zips(session: TestSession) -> list[Path]:
    zip_paths: list[Path] = []
    for spec in QUESTION_SPECS:
        zip_path = session.samples_dir / spec["zip_name"]
        with zipfile.ZipFile(zip_path, "w") as archive:
            archive.writestr(spec["tex_name"], spec["tex_body"])
            archive.writestr("assets/", b"")
            for asset_path, content in spec["assets"].items():
                archive.writestr(asset_path, content)
        zip_paths.append(zip_path)
        session.register_input(
            {
                "kind": "synthetic_question",
                "slug": spec["slug"],
                "upload_file": str(zip_path),
                "zip_entries": [spec["tex_name"], "assets/", *spec["assets"].keys()],
                "create_difficulty": spec["create_difficulty"],
                "metadata_patch": spec["patch"],
            }
        )
    return zip_paths


def build_sample_paper_appendix_zips(session: TestSession) -> dict[str, Path]:
    zip_paths: dict[str, Path] = {}
    for spec in PAPER_APPENDIX_SPECS:
        zip_path = session.samples_dir / spec["zip_name"]
        with zipfile.ZipFile(zip_path, "w") as archive:
            for entry_path, content in spec["appendix_entries"].items():
                archive.writestr(entry_path, content)
        zip_paths[spec["slug"]] = zip_path
        session.register_input(
            {
                "kind": "paper_appendix",
                "slug": spec["slug"],
                "upload_file": str(zip_path),
                "zip_entries": list(spec["appendix_entries"].keys()),
            }
        )

    session.invalid_paper_upload_path.write_text("not a zip archive", encoding="utf-8")
    session.register_input(
        {
            "kind": "invalid_paper_appendix",
            "upload_file": str(session.invalid_paper_upload_path),
        }
    )
    return zip_paths


def build_real_theory_question_zips(session: TestSession) -> list[RealQuestionFixture]:
    fixtures = build_real_question_zips(
        session,
        zip_path=REAL_TEST_ZIP_PATH,
        archive_root_name="CPHOS2",
        extracted_root_name="real_theory_source",
        upload_prefix="real_theory",
        slug_prefix="real-theory",
        kind_label="real_theory_question",
        description_prefix="真实理论样题",
        title_fallback_prefix="theory",
        category="T",
        tag_prefixes=["theory", "real-batch"],
        create_notes_prefix="imported from test.zip folder",
        patch_notes_prefix="real theory fixture",
        expected_count=6,
    )
    return fixtures


def build_real_experiment_question_zips(
    session: TestSession,
) -> list[RealQuestionFixture]:
    fixtures = build_real_question_zips(
        session,
        zip_path=REAL_TEST2_ZIP_PATH,
        archive_root_name="CPHOS4-E",
        extracted_root_name="real_experiment_source",
        upload_prefix="real_experiment",
        slug_prefix="real-experiment",
        kind_label="real_experiment_question",
        description_prefix="真实实验样题",
        title_fallback_prefix="experiment",
        category="E",
        tag_prefixes=["experiment", "real-exp-batch"],
        create_notes_prefix="imported from test2.zip folder",
        patch_notes_prefix="real experiment fixture",
        expected_count=4,
    )
    return fixtures


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
    session.ensure(zip_path.exists(), f"missing test fixture zip: {zip_path}")

    extracted_root = session.tmp_dir / extracted_root_name
    with zipfile.ZipFile(zip_path, "r") as archive:
        archive.extractall(extracted_root)

    base_dir = extracted_root / archive_root_name
    session.ensure(
        base_dir.exists(), f"expected extracted directory missing: {base_dir}"
    )
    upload_dir = session.samples_dir / "real_questions"
    upload_dir.mkdir(parents=True, exist_ok=True)

    fixtures: list[RealQuestionFixture] = []
    source_dirs = sorted(
        (path for path in base_dir.iterdir() if path.is_dir()),
        key=lambda path: int(path.name),
    )
    for index, source_dir in enumerate(source_dirs, start=1):
        tex_path = source_dir / "main.tex"
        session.ensure(
            tex_path.exists(), f"real question is missing main.tex: {source_dir}"
        )
        tex_body = tex_path.read_text(encoding="utf-8", errors="replace")
        title_hint = (
            extract_problem_title(tex_body)
            or f"{title_fallback_prefix}-{source_dir.name}"
        )
        upload_path = upload_dir / f"{upload_prefix}_{source_dir.name}.zip"
        assets_dir = source_dir / "assets"
        asset_paths = (
            sorted(path for path in assets_dir.rglob("*") if path.is_file())
            if assets_dir.exists()
            else []
        )

        with zipfile.ZipFile(upload_path, "w") as archive:
            archive.writestr("main.tex", tex_body)
            archive.writestr("assets/", b"")
            for asset_path in asset_paths:
                archive.write(
                    asset_path,
                    f"assets/{asset_path.relative_to(assets_dir).as_posix()}",
                )

        status = "reviewed" if index % 2 else "used"
        fixture = RealQuestionFixture(
            slug=f"{slug_prefix}-{source_dir.name}",
            upload_path=upload_path,
            create_description=f"{description_prefix} {source_dir.name}",
            create_difficulty={
                "human": {
                    "score": min(10, index + 3),
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

    session.ensure(
        len(fixtures) == expected_count,
        f"expected {expected_count} fixtures from {zip_path.name}, got {len(fixtures)}",
    )
    return fixtures


def extract_problem_title(tex_body: str) -> str | None:
    match = PROBLEM_TITLE_RE.search(tex_body)
    if not match:
        return None
    title = match.group("title").strip()
    return title or None
