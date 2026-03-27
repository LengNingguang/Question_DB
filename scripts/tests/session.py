from __future__ import annotations

import io
import json
import shutil
import subprocess
import urllib.error
import urllib.request
import uuid
import zipfile
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .config import (
    API_LOG_PATH,
    API_PORT,
    CONTAINER_NAME,
    DB_URL,
    DOWNLOADS_DIR,
    EXPORT_PATH,
    INVALID_PAPER_UPLOAD_PATH,
    POSTGRES_IMAGE,
    POSTGRES_PORT,
    QUALITY_PATH,
    REPORT_PATH,
    ROOT_DIR,
    SAMPLES_DIR,
    TMP_DIR,
)


def parse_json(body: str) -> Any:
    return json.loads(body) if body else None


def question_ids_from_body(body: str) -> list[str]:
    return [item["question_id"] for item in parse_json(body)]


def normalize_headers(items) -> dict[str, str]:
    return {key.lower(): value for key, value in items}


def pretty_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)


def format_body(value: Any) -> tuple[str, str]:
    if value is None:
        return "text", "(empty)"
    if isinstance(value, (dict, list)):
        return "json", pretty_json(value)
    return "text", str(value)


def markdown_code_block(value: Any) -> str:
    language, text = format_body(value)
    suffix = "json" if language == "json" else "text"
    return f"```{suffix}\n{text}\n```"


@dataclass
class TestSession:
    request_logs: list[dict[str, Any]] = field(default_factory=list)
    validation_notes: list[str] = field(default_factory=list)
    sample_inputs: list[dict[str, Any]] = field(default_factory=list)
    api_process: subprocess.Popen | None = None
    api_log_file: Any = None

    @property
    def root_dir(self) -> Path:
        return ROOT_DIR

    @property
    def tmp_dir(self) -> Path:
        return TMP_DIR

    @property
    def samples_dir(self) -> Path:
        return SAMPLES_DIR

    @property
    def downloads_dir(self) -> Path:
        return DOWNLOADS_DIR

    @property
    def api_log_path(self) -> Path:
        return API_LOG_PATH

    @property
    def export_path(self) -> Path:
        return EXPORT_PATH

    @property
    def quality_path(self) -> Path:
        return QUALITY_PATH

    @property
    def report_path(self) -> Path:
        return REPORT_PATH

    @property
    def invalid_paper_upload_path(self) -> Path:
        return INVALID_PAPER_UPLOAD_PATH

    def print_step(self, label: str) -> None:
        print(label, flush=True)

    def ensure(self, condition: bool, message: str) -> None:
        if not condition:
            raise AssertionError(message)

    def register_input(self, item: dict[str, Any]) -> None:
        self.sample_inputs.append(item)

    def run_command(
        self,
        cmd: list[str],
        *,
        input_bytes: bytes | None = None,
        check: bool = True,
    ) -> subprocess.CompletedProcess:
        return subprocess.run(
            cmd,
            cwd=self.root_dir,
            input=input_bytes,
            check=check,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def prepare_workspace(self) -> None:
        shutil.rmtree(self.tmp_dir, ignore_errors=True)
        self.samples_dir.mkdir(parents=True, exist_ok=True)
        self.downloads_dir.mkdir(parents=True, exist_ok=True)

    def cleanup(self) -> None:
        if self.api_process is not None and self.api_process.poll() is None:
            self.api_process.terminate()
            try:
                self.api_process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.api_process.kill()
                self.api_process.wait(timeout=5)

        if self.api_log_file is not None and not self.api_log_file.closed:
            self.api_log_file.close()

        existing = self.run_command(
            ["docker", "ps", "-a", "--format", "{{.Names}}"],
            check=False,
        )
        if CONTAINER_NAME in existing.stdout.decode().splitlines():
            self.run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)

    def append_request_log(
        self,
        *,
        label: str,
        method: str,
        path: str,
        expected_status: int,
        actual_status: int,
        request_headers: dict[str, str],
        request_body: Any,
        response_headers: dict[str, str],
        response_body: Any,
    ) -> None:
        self.request_logs.append(
            {
                "label": label,
                "method": method,
                "path": path,
                "expected_status": expected_status,
                "actual_status": actual_status,
                "request_headers": request_headers,
                "request_body": request_body,
                "response_headers": response_headers,
                "response_body": response_body,
            }
        )

    def perform_request(
        self,
        label: str,
        expected_status: int,
        *,
        method: str = "GET",
        path: str,
        headers: dict[str, str] | None = None,
        body: bytes | None = None,
        request_body: Any = None,
    ) -> tuple[int, str, dict[str, str]]:
        url = f"http://127.0.0.1:{API_PORT}{path}"
        request_headers = headers or {}
        request = urllib.request.Request(
            url, data=body, method=method, headers=request_headers
        )

        try:
            with urllib.request.urlopen(request) as response:
                status = response.status
                response_headers = normalize_headers(response.headers.items())
                response_body = response.read().decode("utf-8")
        except urllib.error.HTTPError as err:
            status = err.code
            response_headers = normalize_headers(err.headers.items())
            response_body = err.read().decode("utf-8", errors="replace")

        self.append_request_log(
            label=label,
            method=method,
            path=path,
            expected_status=expected_status,
            actual_status=status,
            request_headers=request_headers,
            request_body=request_body,
            response_headers=response_headers,
            response_body=response_body,
        )

        if status != expected_status:
            raise RuntimeError(
                f"Unexpected status for {label}: expected {expected_status}, got {status}"
            )
        return status, response_body, response_headers

    def json_request(
        self,
        label: str,
        expected_status: int,
        *,
        method: str,
        path: str,
        payload: dict[str, Any],
    ) -> tuple[int, str, dict[str, str]]:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        return self.perform_request(
            label,
            expected_status,
            method=method,
            path=path,
            headers={"content-type": "application/json"},
            body=body,
            request_body=payload,
        )

    def multipart_request(
        self,
        label: str,
        expected_status: int,
        *,
        method: str = "POST",
        path: str,
        text_fields: dict[str, str] | None,
        field_name: str | None = None,
        file_path: Path | None = None,
        content_type: str | None = None,
    ) -> tuple[int, str, dict[str, str]]:
        boundary = f"----QBApiBoundary{uuid.uuid4().hex}"
        body = bytearray()
        for name, value in (text_fields or {}).items():
            body.extend(
                (
                    f"--{boundary}\r\n"
                    f'Content-Disposition: form-data; name="{name}"\r\n\r\n'
                    f"{value}\r\n"
                ).encode("utf-8")
            )
        if file_path is not None:
            if field_name is None or content_type is None:
                raise ValueError(
                    "field_name and content_type are required when file_path is provided"
                )
            file_bytes = file_path.read_bytes()
            body.extend(
                (
                    f"--{boundary}\r\n"
                    f'Content-Disposition: form-data; name="{field_name}"; filename="{file_path.name}"\r\n'
                    f"Content-Type: {content_type}\r\n\r\n"
                ).encode("utf-8")
            )
            body.extend(file_bytes)
            body.extend(b"\r\n")
        body.extend(f"--{boundary}--\r\n".encode("utf-8"))

        return self.perform_request(
            label,
            expected_status,
            method=method,
            path=path,
            headers={"content-type": f"multipart/form-data; boundary={boundary}"},
            body=bytes(body),
            request_body={
                **({"file": str(file_path)} if file_path is not None else {}),
                **(text_fields or {}),
            },
        )

    def inspect_zip_file(self, file_path: Path) -> dict[str, Any]:
        with zipfile.ZipFile(file_path, "r") as archive:
            names = archive.namelist()
            manifest = None
            if "manifest.json" in names:
                manifest = json.loads(archive.read("manifest.json").decode("utf-8"))
        return {
            "entries": names,
            "manifest": manifest,
        }

    def inspect_zip_bytes(self, data: bytes) -> list[str]:
        with zipfile.ZipFile(io.BytesIO(data), "r") as archive:
            return archive.namelist()

    def binary_json_request(
        self,
        label: str,
        expected_status: int,
        *,
        path: str,
        payload: dict[str, Any],
        output_path: Path,
    ) -> tuple[dict[str, Any], list[str]]:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        request_headers = {"content-type": "application/json"}
        request = urllib.request.Request(
            f"http://127.0.0.1:{API_PORT}{path}",
            data=body,
            method="POST",
            headers=request_headers,
        )

        try:
            with urllib.request.urlopen(request) as response:
                status = response.status
                response_headers = normalize_headers(response.headers.items())
                response_bytes = response.read()
        except urllib.error.HTTPError as err:
            status = err.code
            response_headers = normalize_headers(err.headers.items())
            response_body = err.read().decode("utf-8", errors="replace")
            self.append_request_log(
                label=label,
                method="POST",
                path=path,
                expected_status=expected_status,
                actual_status=status,
                request_headers=request_headers,
                request_body=payload,
                response_headers=response_headers,
                response_body=response_body,
            )
            raise RuntimeError(
                f"Unexpected status for {label}: expected {expected_status}, got {status}"
            ) from err

        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_bytes(response_bytes)
        zip_details = self.inspect_zip_file(output_path)
        self.ensure(
            zip_details["manifest"] is not None,
            f"{label} should include manifest.json",
        )

        self.append_request_log(
            label=label,
            method="POST",
            path=path,
            expected_status=expected_status,
            actual_status=status,
            request_headers=request_headers,
            request_body=payload,
            response_headers=response_headers,
            response_body={
                "saved_path": str(output_path),
                "entries": zip_details["entries"],
                "manifest": zip_details["manifest"],
            },
        )

        return zip_details["manifest"], zip_details["entries"]

    def wait_for_postgres(self) -> None:
        for _ in range(60):
            result = self.run_command(
                [
                    "docker",
                    "exec",
                    CONTAINER_NAME,
                    "pg_isready",
                    "-U",
                    "postgres",
                    "-d",
                    "qb",
                ],
                check=False,
            )
            if result.returncode == 0:
                return
            import time

            time.sleep(1)
        self.run_command(
            [
                "docker",
                "exec",
                CONTAINER_NAME,
                "pg_isready",
                "-U",
                "postgres",
                "-d",
                "qb",
            ]
        )

    def wait_for_api(self) -> None:
        for _ in range(60):
            try:
                with urllib.request.urlopen(
                    f"http://127.0.0.1:{API_PORT}/health"
                ) as response:
                    if response.status == 200:
                        return
            except Exception:
                import time

                time.sleep(1)
        with urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}/health") as response:
            self.ensure(response.status == 200, "health check should be 200")

    def start_postgres_container(self) -> None:
        existing = self.run_command(
            ["docker", "ps", "-a", "--format", "{{.Names}}"],
            check=False,
        )
        if CONTAINER_NAME in existing.stdout.decode().splitlines():
            self.run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)

        self.run_command(
            [
                "docker",
                "run",
                "-d",
                "--name",
                CONTAINER_NAME,
                "-e",
                "POSTGRES_USER=postgres",
                "-e",
                "POSTGRES_PASSWORD=postgres",
                "-e",
                "POSTGRES_DB=qb",
                "-p",
                f"{POSTGRES_PORT}:5432",
                POSTGRES_IMAGE,
            ]
        )
        self.wait_for_postgres()

    def apply_migration(self) -> None:
        migration_bytes = (
            self.root_dir / "migrations" / "0001_init_pg.sql"
        ).read_bytes()
        self.run_command(
            [
                "docker",
                "exec",
                "-i",
                CONTAINER_NAME,
                "psql",
                "-U",
                "postgres",
                "-d",
                "qb",
            ],
            input_bytes=migration_bytes,
        )

    def start_api(self) -> None:
        self.api_log_file = self.api_log_path.open("wb")
        import os

        env = dict(**os.environ)
        env["QB_DATABASE_URL"] = DB_URL
        env["QB_BIND_ADDR"] = f"127.0.0.1:{API_PORT}"
        self.api_process = subprocess.Popen(
            ["cargo", "run"],
            cwd=self.root_dir,
            env=env,
            stdout=self.api_log_file,
            stderr=subprocess.STDOUT,
        )
        self.wait_for_api()

    def write_report(self, status: str, error_text: str | None) -> None:
        generated_at = datetime.now(timezone.utc).isoformat()
        lines = [
            "# QB E2E Report",
            "",
            f"- Generated at: `{generated_at}`",
            f"- Status: `{status}`",
            f"- Report path: `{self.report_path}`",
            f"- Artifacts directory: `{self.tmp_dir}`",
            f"- API log: `{self.api_log_path}`",
            f"- Export output: `{self.export_path}`",
            f"- Quality output: `{self.quality_path}`",
            f"- Downloaded zips directory: `{self.downloads_dir}`",
            "",
            "## Sample Inputs",
            "",
            markdown_code_block(self.sample_inputs),
            "",
            "## Validation Notes",
            "",
        ]

        if self.validation_notes:
            lines.extend([f"- {note}" for note in self.validation_notes])
        else:
            lines.append("- No validation notes recorded.")

        if error_text:
            lines.extend(
                [
                    "",
                    "## Failure",
                    "",
                    "```text",
                    error_text.rstrip(),
                    "```",
                ]
            )

        lines.extend(["", "## HTTP Exchanges", ""])

        for index, entry in enumerate(self.request_logs, start=1):
            lines.extend(
                [
                    f"### {index}. {entry['label']}",
                    "",
                    f"- Request: `{entry['method']} {entry['path']}`",
                    f"- Expected status: `{entry['expected_status']}`",
                    f"- Actual status: `{entry['actual_status']}`",
                    "",
                    "#### Request Headers",
                    "",
                    markdown_code_block(entry["request_headers"] or {}),
                    "",
                    "#### Request Body",
                    "",
                    markdown_code_block(entry["request_body"]),
                    "",
                    "#### Response Headers",
                    "",
                    markdown_code_block(entry["response_headers"] or {}),
                    "",
                    "#### Response Body",
                    "",
                    markdown_code_block(entry["response_body"]),
                    "",
                ]
            )

        self.report_path.write_text("\n".join(lines), encoding="utf-8")
