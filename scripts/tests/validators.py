from __future__ import annotations

import io
import zipfile
from pathlib import Path


def validate_question_bundle(
    manifest: dict,
    names: list[str],
    question_ids: list[str],
    ensure,
) -> None:
    ensure(
        manifest["kind"] == "question_bundle", "question bundle manifest kind mismatch"
    )
    ensure(
        manifest["question_count"] == len(question_ids),
        "question bundle count mismatch",
    )
    bundled_ids = [item["question_id"] for item in manifest["questions"]]
    ensure(
        bundled_ids == question_ids, "question bundle ids should preserve request order"
    )
    for item in manifest["questions"]:
        expected_prefix = f"{item['metadata']['description']}_"
        ensure(
            item["directory"].startswith(expected_prefix),
            "question bundle directory should start with description",
        )
        ensure(
            item["directory"] != item["question_id"],
            "question bundle directory should not use raw question id",
        )
        file_paths = {entry["zip_path"] for entry in item["files"]}
        ensure(
            all(path.startswith(f"{item['directory']}/") for path in file_paths),
            "question bundle files should live under the description directory",
        )
        ensure(
            any(path.endswith(".tex") for path in file_paths),
            "question bundle should include tex",
        )
        ensure(
            any("/assets/" in path for path in file_paths),
            "question bundle should include assets",
        )
        ensure(
            all(path in names for path in file_paths),
            "question bundle manifest paths must exist in zip",
        )


def validate_paper_bundle(
    manifest: dict,
    names: list[str],
    paper_ids: list[str],
    bundle_path: Path,
    expected_papers: dict[str, dict],
    expected_template_source: str,
    expected_category: str,
    sample_problem_title: str,
    ensure,
) -> None:
    ensure(manifest["kind"] == "paper_bundle", "paper bundle manifest kind mismatch")
    ensure(manifest["paper_count"] == len(paper_ids), "paper bundle count mismatch")
    bundled_ids = [item["paper_id"] for item in manifest["papers"]]
    ensure(bundled_ids == paper_ids, "paper bundle ids should preserve request order")

    with zipfile.ZipFile(bundle_path, "r") as archive:
        for item in manifest["papers"]:
            expected = expected_papers[item["paper_id"]]
            paper_prefix = f"{item['metadata']['description']}_"
            ensure(
                item["directory"].startswith(paper_prefix),
                "paper bundle directory should start with description",
            )
            ensure(
                item["directory"] != item["paper_id"],
                "paper bundle directory should not use raw paper id",
            )
            ensure(
                item["template_source"] == expected_template_source,
                "paper bundle should use the expected paper template",
            )
            ensure(
                item["metadata"]["title"] == expected["title"],
                "paper title should round-trip",
            )
            ensure(
                item["metadata"]["subtitle"] == expected["subtitle"],
                "paper subtitle should round-trip",
            )
            ensure(
                item["metadata"]["authors"] == expected["authors"],
                "paper authors should round-trip",
            )
            ensure(
                item["metadata"]["reviewers"] == expected["reviewers"],
                "paper reviewers should round-trip",
            )

            append_file = item["append_file"]
            ensure(
                append_file["file_kind"] == "appendix",
                "paper appendix file kind mismatch",
            )
            ensure(
                append_file["zip_path"] == f"{item['directory']}/append.zip",
                "paper appendix should be renamed to append.zip",
            )
            ensure(
                append_file["original_path"] == expected["appendix_path"].name,
                "paper appendix manifest should keep original file name",
            )
            ensure(
                append_file["zip_path"] in names,
                "paper appendix path should exist in bundle",
            )
            append_bytes = archive.read(append_file["zip_path"])
            ensure(
                append_bytes == expected["appendix_path"].read_bytes(),
                "paper appendix bytes should round-trip",
            )
            with zipfile.ZipFile(expected["appendix_path"], "r") as appendix_archive:
                expected_append_entries = sorted(appendix_archive.namelist())
            with zipfile.ZipFile(io.BytesIO(append_bytes), "r") as appendix_archive:
                ensure(
                    sorted(appendix_archive.namelist()) == expected_append_entries,
                    "paper appendix zip contents should round-trip",
                )

            main_tex_file = item["main_tex_file"]
            ensure(
                main_tex_file["zip_path"] == f"{item['directory']}/main.tex",
                "paper bundle should expose main.tex at the paper root",
            )
            ensure(
                main_tex_file["zip_path"] in names,
                "main.tex should exist in the bundle",
            )
            ensure(
                main_tex_file["original_path"] == expected_template_source,
                "main.tex manifest should record the source template path",
            )
            main_tex = archive.read(main_tex_file["zip_path"]).decode("utf-8")
            ensure(
                f"\\cphostitle{{{expected['title']}}}" in main_tex,
                "rendered main.tex should include the paper title",
            )
            ensure(
                f"\\cphossubtitle{{{expected['subtitle']}}}" in main_tex,
                "rendered main.tex should include the paper subtitle",
            )
            ensure(
                main_tex.count("\\begin{problem}") == len(expected["question_ids"]),
                "rendered main.tex should contain one problem block per paper question",
            )
            ensure(
                sample_problem_title not in main_tex,
                "rendered main.tex should not keep the template sample problem",
            )
            ensure(
                "X~X\\quad XXX\\quad XXX" not in main_tex,
                "rendered main.tex should replace the template author placeholder",
            )
            ensure(
                "Y~Y\\quad YYY\\quad YYY" not in main_tex,
                "rendered main.tex should replace the template reviewer placeholder",
            )

            actual_question_ids = [
                question["question_id"] for question in item["questions"]
            ]
            ensure(
                actual_question_ids == expected["question_ids"],
                "paper bundle question order should preserve the paper order",
            )
            for sequence, question in enumerate(item["questions"], start=1):
                ensure(
                    question["sequence"] == sequence,
                    "paper bundle question sequence should be 1-based and ordered",
                )
                ensure(
                    question["asset_prefix"] == f"p{sequence}-",
                    "paper bundle question asset prefix should match the sequence",
                )
                ensure(
                    question["source_tex_path"] == "main.tex",
                    "uploaded real questions should keep main.tex as source path",
                )
                ensure(
                    question["metadata"]["category"] == expected_category,
                    "real paper bundle questions should keep the expected category",
                )

            ensure(
                len(item["assets"]) == expected["asset_total"],
                "paper bundle merged asset count should match all paper question assets",
            )
            for asset in item["assets"]:
                ensure(
                    asset["zip_path"] in names,
                    "rendered asset path should exist in bundle",
                )
                ensure(
                    asset["zip_path"].startswith(f"{item['directory']}/assets/"),
                    "rendered assets should live under the merged assets directory",
                )
                ensure(
                    asset["source_question_id"] in expected["question_ids"],
                    "rendered asset should point back to one of the paper questions",
                )
