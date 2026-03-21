#!/usr/bin/env python3

import atexit
import json
import mimetypes
import os
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile
from pathlib import Path


ROOT_DIR = Path(__file__).resolve().parent.parent
TMP_DIR = ROOT_DIR / "samples"
ZIP_PATH = TMP_DIR / "question.zip"
API_LOG_PATH = Path("/tmp/qb_api_e2e.log")

CONTAINER_NAME = os.environ.get("CONTAINER_NAME", "qb-postgres-e2e")
POSTGRES_IMAGE = os.environ.get("POSTGRES_IMAGE", "postgres:14.1")
POSTGRES_PORT = os.environ.get("POSTGRES_PORT", "55433")
API_PORT = os.environ.get("API_PORT", "18080")
DB_URL = f"postgres://postgres:postgres@127.0.0.1:{POSTGRES_PORT}/qb"
PAUSE_BETWEEN_REQUESTS = os.environ.get("PAUSE_BETWEEN_REQUESTS", "1") != "0"

api_process: subprocess.Popen | None = None


def run_command(cmd: list[str], *, input_bytes: bytes | None = None, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd,
        cwd=ROOT_DIR,
        input=input_bytes,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def cleanup() -> None:
    global api_process

    if api_process is not None and api_process.poll() is None:
        api_process.terminate()
        try:
            api_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            api_process.kill()
            api_process.wait(timeout=5)

    existing = run_command(
        ["docker", "ps", "-a", "--format", "{{.Names}}"],
        check=False,
    )
    names = existing.stdout.decode().splitlines()
    if CONTAINER_NAME in names:
        run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)

    shutil.rmtree(TMP_DIR, ignore_errors=True)


def handle_signal(signum: int, _frame) -> None:
    cleanup()
    raise SystemExit(128 + signum)


atexit.register(cleanup)
signal.signal(signal.SIGINT, handle_signal)
signal.signal(signal.SIGTERM, handle_signal)


def print_step(label: str) -> None:
    print(label, flush=True)


def pretty_print_body(body: str) -> None:
    if not body:
        print("(empty body)")
        return

    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        print(body)
        return

    print(json.dumps(parsed, ensure_ascii=False, indent=2))


def pause_after_request() -> None:
    if not PAUSE_BETWEEN_REQUESTS or not sys.stdin.isatty():
        return
    input("Press Enter to continue...")


def perform_request(
    label: str,
    expected_status: int,
    *,
    method: str = "GET",
    path: str,
    headers: dict[str, str] | None = None,
    body: bytes | None = None,
) -> tuple[int, str]:
    url = f"http://127.0.0.1:{API_PORT}{path}"
    request = urllib.request.Request(url, data=body, method=method, headers=headers or {})

    try:
        with urllib.request.urlopen(request) as response:
            status = response.status
            response_body = response.read().decode("utf-8")
    except urllib.error.HTTPError as err:
        status = err.code
        response_body = err.read().decode("utf-8")

    print()
    print(f"=== {label} ===")
    print(f"HTTP {status}")
    pretty_print_body(response_body)

    if status != expected_status:
        raise RuntimeError(
            f"Unexpected status for {label}: expected {expected_status}, got {status}"
        )

    pause_after_request()
    return status, response_body


def json_request(
    label: str,
    expected_status: int,
    *,
    method: str,
    path: str,
    payload: dict,
) -> tuple[int, str]:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    return perform_request(
        label,
        expected_status,
        method=method,
        path=path,
        headers={"Content-Type": "application/json"},
        body=body,
    )


def multipart_request(
    label: str,
    expected_status: int,
    *,
    path: str,
    field_name: str,
    file_path: Path,
    content_type: str,
) -> tuple[int, str]:
    boundary = f"----QBApiBoundary{uuid.uuid4().hex}"
    filename = file_path.name
    file_bytes = file_path.read_bytes()
    body = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="{field_name}"; filename="{filename}"\r\n'
        f"Content-Type: {content_type}\r\n\r\n"
    ).encode("utf-8") + file_bytes + f"\r\n--{boundary}--\r\n".encode("utf-8")

    return perform_request(
        label,
        expected_status,
        method="POST",
        path=path,
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        body=body,
    )


def ensure(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def parse_json(body: str):
    return json.loads(body) if body else None


def wait_for_postgres() -> None:
    for _ in range(60):
        result = run_command(
            ["docker", "exec", CONTAINER_NAME, "pg_isready", "-U", "postgres", "-d", "qb"],
            check=False,
        )
        if result.returncode == 0:
            return
        time.sleep(1)
    run_command(["docker", "exec", CONTAINER_NAME, "pg_isready", "-U", "postgres", "-d", "qb"])


def wait_for_api() -> None:
    for _ in range(60):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}/health") as response:
                if response.status == 200:
                    return
        except Exception:
            time.sleep(1)
    with urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}/health") as response:
        ensure(response.status == 200, "health check should be 200")


def build_demo_zip() -> None:
    TMP_DIR.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(ZIP_PATH, "w") as archive:
        archive.writestr("problem.tex", "\\section{Demo question}\n")
        archive.writestr("assets/figure.txt", "demo-asset")


def start_postgres_container() -> None:
    existing = run_command(
        ["docker", "ps", "-a", "--format", "{{.Names}}"],
        check=False,
    )
    if CONTAINER_NAME in existing.stdout.decode().splitlines():
        run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)

    run_command(
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
    wait_for_postgres()


def apply_migration() -> None:
    migration_bytes = (ROOT_DIR / "migrations" / "0001_init_pg.sql").read_bytes()
    run_command(
        ["docker", "exec", "-i", CONTAINER_NAME, "psql", "-U", "postgres", "-d", "qb"],
        input_bytes=migration_bytes,
    )


def start_api() -> None:
    global api_process

    log_file = API_LOG_PATH.open("wb")
    env = os.environ.copy()
    env["QB_DATABASE_URL"] = DB_URL
    env["QB_BIND_ADDR"] = f"127.0.0.1:{API_PORT}"
    api_process = subprocess.Popen(
        ["cargo", "run"],
        cwd=ROOT_DIR,
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    wait_for_api()


def main() -> None:
    print_step("[1/6] Build a demo zip bundle")
    build_demo_zip()

    print_step("[2/6] Start PostgreSQL container")
    start_postgres_container()

    print_step("[3/6] Apply migration")
    apply_migration()

    print_step("[4/6] Start API")
    start_api()

    perform_request("GET /health", 200, path="/health")

    print_step("[5/6] Exercise question CRUD")
    _, body = multipart_request(
        "POST /questions",
        200,
        path="/questions",
        field_name="file",
        file_path=ZIP_PATH,
        content_type="application/zip",
    )
    question_id = parse_json(body)["question_id"]
    ensure(bool(question_id), "question_id should not be empty")

    _, body = perform_request("GET /questions", 200, path="/questions")
    ensure(question_id in body, "question list should contain the uploaded question")

    _, body = perform_request("GET /questions?q=problem", 200, path="/questions?q=problem")
    ensure(question_id in body, "question search should contain the uploaded question")

    _, body = perform_request("GET /questions/{question_id}", 200, path=f"/questions/{question_id}")
    ensure(question_id in body, "question detail should contain question_id")

    _, body = json_request(
        f"PATCH /questions/{question_id}",
        200,
        method="PATCH",
        path=f"/questions/{question_id}",
        payload={
            "category": "T",
            "notes": "demo question",
            "tags": ["optics", "mechanics"],
            "status": "reviewed",
            "difficulty": {
                "human": 7,
                "algorithm": {"algo1": 6},
                "notes": "sample",
            },
        },
    )
    ensure('"category": "T"' in body or '"category":"T"' in body, "question patch should update category")
    ensure('"status": "reviewed"' in body or '"status":"reviewed"' in body, "question patch should update status")
    ensure("demo question" in body, "question patch should update notes")

    print_step("[6/6] Exercise paper CRUD and ops APIs")
    _, body = json_request(
        "POST /papers",
        200,
        method="POST",
        path="/papers",
        payload={
            "edition": "2026",
            "paper_type": "regular",
            "title": "Demo paper",
            "notes": "generated by e2e",
            "question_ids": [question_id],
        },
    )
    paper_id = parse_json(body)["paper_id"]
    ensure(bool(paper_id), "paper_id should not be empty")

    _, body = perform_request("GET /papers", 200, path="/papers")
    ensure(paper_id in body, "paper list should contain the created paper")

    _, body = perform_request("GET /papers/{paper_id}", 200, path=f"/papers/{paper_id}")
    ensure(question_id in body, "paper detail should contain linked question")

    _, body = perform_request(
        "GET /questions/{question_id} linked paper",
        200,
        path=f"/questions/{question_id}",
    )
    ensure(paper_id in body, "question detail should contain linked paper")

    _, body = perform_request(
        "GET /questions?paper_id={paper_id}",
        200,
        path=f"/questions?paper_id={urllib.parse.quote(paper_id)}",
    )
    ensure(question_id in body, "questions filtered by paper_id should contain the question")

    _, body = json_request(
        f"PATCH /papers/{paper_id}",
        200,
        method="PATCH",
        path=f"/papers/{paper_id}",
        payload={
            "edition": "2027",
            "title": "Demo paper updated",
            "notes": "updated by e2e",
            "question_ids": [question_id],
        },
    )
    ensure("Demo paper updated" in body, "paper patch should update title")

    _, body = json_request(
        "POST /exports/run",
        200,
        method="POST",
        path="/exports/run",
        payload={
            "format": "jsonl",
            "public": False,
            "output_path": "/tmp/qb_e2e_internal.jsonl",
        },
    )
    ensure('"exported_questions": 1' in body or '"exported_questions":1' in body, "export should include one question")

    _, body = json_request(
        "POST /quality-checks/run",
        200,
        method="POST",
        path="/quality-checks/run",
        payload={"output_path": "/tmp/qb_e2e_quality.json"},
    )
    ensure('"empty_papers": []' in body or '"empty_papers":[]' in body, "quality report should not contain empty papers")

    _, body = perform_request(
        f"DELETE /questions/{question_id}",
        200,
        method="DELETE",
        path=f"/questions/{question_id}",
    )
    ensure(question_id in body, "question delete should return deleted question_id")

    perform_request(
        f"GET /questions/{question_id} after delete",
        404,
        path=f"/questions/{question_id}",
    )

    _, body = json_request(
        "POST /quality-checks/run after question delete",
        200,
        method="POST",
        path="/quality-checks/run",
        payload={"output_path": "/tmp/qb_e2e_quality.json"},
    )
    ensure("empty_papers" in body, "quality report should still be returned after question delete")

    _, body = perform_request(
        f"DELETE /papers/{paper_id}",
        200,
        method="DELETE",
        path=f"/papers/{paper_id}",
    )
    ensure(paper_id in body, "paper delete should return deleted paper_id")

    perform_request(
        f"GET /papers/{paper_id} after delete",
        404,
        path=f"/papers/{paper_id}",
    )

    print("E2E full flow passed.")


if __name__ == "__main__":
    main()
