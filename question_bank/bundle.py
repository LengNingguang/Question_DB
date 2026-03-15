from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from .utils import load_json, sha256_file

ALLOWED_QUESTION_KEYS = {
    "question_id",
    "question_no",
    "category",
    "latex_body",
    "plain_text",
    "answer_latex",
    "answer_text",
    "status",
    "source_pages",
    "tags",
    "assets",
}


@dataclass(slots=True)
class ValidationResult:
    errors: list[str]
    warnings: list[str]

    @property
    def ok(self) -> bool:
        return not self.errors


def load_bundle(bundle_path: Path) -> tuple[dict, list[dict]]:
    manifest = load_json(bundle_path / "manifest.json")
    question_dir = bundle_path / "questions"
    questions = [load_json(path) for path in sorted(question_dir.glob("*.json"))]
    return manifest, questions


def validate_bundle(bundle_path: Path) -> ValidationResult:
    errors: list[str] = []
    warnings: list[str] = []
    manifest, questions = load_bundle(bundle_path)
    required_manifest_keys = {"bundle_name", "run_label", "paper"}
    missing_manifest = required_manifest_keys - set(manifest)
    if missing_manifest:
        errors.append(f"manifest.json 缺少字段: {sorted(missing_manifest)}")
    if not questions:
        errors.append("questions/ 下没有题目 JSON 文件。")

    seen_ids: set[str] = set()
    seen_numbers: set[str] = set()

    for idx, question in enumerate(questions, start=1):
        file_label = f"题目 #{idx}"
        required_keys = {"question_id", "question_no", "category", "latex_body", "plain_text", "status", "source_pages", "assets", "tags"}
        missing = required_keys - set(question)
        if missing:
            errors.append(f"{file_label} 缺少字段: {sorted(missing)}")
        unknown = set(question) - ALLOWED_QUESTION_KEYS
        if unknown:
            warnings.append(f"{file_label} 包含未识别字段: {sorted(unknown)}")
        question_id = question.get("question_id")
        if question_id:
            if question_id in seen_ids:
                errors.append(f"重复的 question_id: {question_id}")
            seen_ids.add(question_id)
        question_no = question.get("question_no")
        if question_no:
            if question_no in seen_numbers:
                warnings.append(f"同一个 bundle 中出现重复题号: {question_no}")
            seen_numbers.add(question_no)
        if question.get("category") not in {"theory", "experiment"}:
            errors.append(f"{question_id or file_label} 的 category 必须为 theory 或 experiment。")
        if question.get("status") not in {"raw", "reviewed", "published"}:
            errors.append(f"{question_id or file_label} 的 status 必须为 raw/reviewed/published。")
        pages = question.get("source_pages", {})
        if not isinstance(pages, dict) or "start" not in pages or "end" not in pages:
            errors.append(f"{question_id or file_label} 的 source_pages 必须包含 start 和 end。")
        for asset in question.get("assets", []):
            rel_path = asset.get("file_path")
            if not rel_path:
                errors.append(f"{question_id or file_label} 的 asset 缺少 file_path。")
                continue
            asset_path = bundle_path / rel_path
            if not asset_path.exists():
                errors.append(f"{question_id or file_label} 的 asset 不存在: {rel_path}")
            else:
                expected_sha = asset.get("sha256")
                actual_sha = sha256_file(asset_path)
                if expected_sha and expected_sha.lower() != actual_sha.lower():
                    errors.append(f"{question_id or file_label} 的 asset 校验失败: {rel_path}")
                if not expected_sha:
                    warnings.append(f"{question_id or file_label} 的 asset 未填写 sha256: {rel_path}")

    return ValidationResult(errors=errors, warnings=warnings)
