#!/usr/bin/env python3

import atexit
import json
import os
import shutil
import signal
import subprocess
import traceback
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile
from datetime import datetime, timezone
from pathlib import Path


ROOT_DIR = Path(__file__).resolve().parent.parent
TMP_DIR = Path("tmp")
SAMPLES_DIR = TMP_DIR / "samples"
DOWNLOADS_DIR = TMP_DIR / "downloads"
API_LOG_PATH = TMP_DIR / "qb_api_e2e.log"
EXPORT_PATH = TMP_DIR / "qb_e2e_internal.jsonl"
QUALITY_PATH = TMP_DIR / "qb_e2e_quality.json"
REPORT_PATH = TMP_DIR / "qb_e2e_report.md"

CONTAINER_NAME = os.environ.get("CONTAINER_NAME", "qb-postgres-e2e")
POSTGRES_IMAGE = os.environ.get("POSTGRES_IMAGE", "postgres:14.1")
POSTGRES_PORT = os.environ.get("POSTGRES_PORT", "55433")
API_PORT = os.environ.get("API_PORT", "18080")
DB_URL = f"postgres://postgres:postgres@127.0.0.1:{POSTGRES_PORT}/qb"

api_process: subprocess.Popen | None = None
api_log_file = None
request_logs: list[dict] = []
validation_notes: list[str] = []


QUESTION_SPECS = [
    {
        "slug": "mechanics",
        "zip_name": "question_mechanics.zip",
        "tex_name": "mechanics.tex",
        "tex_body": "\\section{Mechanics calibration}\nA cart slides on an incline.\n",
        "create_description": "mechanics benchmark alpha",
        "create_difficulty": {
            "human": {
                "score": 2,
                "notes": "import baseline",
            }
        },
        "assets": {
            "assets/diagram.txt": "incline-figure",
            "assets/data.csv": "time,velocity\n0,0\n1,3\n",
        },
        "patch": {
            "category": "T",
            "description": "mechanics benchmark alpha",
            "tags": ["mechanics", "kinematics"],
            "status": "reviewed",
            "difficulty": {
                "human": {"score": 4, "notes": "warm-up"},
                "heuristic": {"score": 5, "notes": "fast estimate"},
                "ml": {"score": 3},
            },
        },
    },
    {
        "slug": "optics",
        "zip_name": "question_optics.zip",
        "tex_name": "optics.tex",
        "tex_body": "\\section{Optics setup}\nA lens forms an image on a screen.\n",
        "create_description": "optics bundle beta",
        "create_difficulty": {
            "human": {
                "score": 6,
                "notes": "import triage",
            }
        },
        "assets": {
            "assets/lens.txt": "thin-lens",
            "assets/ray-path.txt": "ray-diagram",
        },
        "patch": {
            "category": "E",
            "description": "optics bundle beta",
            "tags": ["optics", "lenses"],
            "status": "used",
            "difficulty": {
                "human": {"score": 7, "notes": "competition-ready"},
                "heuristic": {"score": 6, "notes": "geometry-heavy"},
                "ml": {"score": 8, "notes": "vision model struggle"},
                "symbolic": {"score": 9},
            },
        },
    },
    {
        "slug": "thermal",
        "zip_name": "question_thermal.zip",
        "tex_name": "thermal.tex",
        "tex_body": "\\section{Thermal equilibration}\nTwo bodies exchange heat.\n",
        "create_description": "热学标定 gamma",
        "create_difficulty": {
            "human": {
                "score": 5,
            }
        },
        "assets": {
            "assets/table.txt": "material,c\nCu,385\nAl,900\n",
            "assets/reference.txt": "thermal-reference",
        },
        "patch": {
            "category": "none",
            "description": "热学标定 gamma",
            "tags": ["thermal", "calorimetry"],
            "status": "none",
            "difficulty": {
                "human": {"score": 5, "notes": ""},
                "heuristic": {"score": 4, "notes": "direct model"},
                "simulator": {"score": 6},
            },
        },
    },
]


def run_command(
    cmd: list[str],
    *,
    input_bytes: bytes | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd,
        cwd=ROOT_DIR,
        input=input_bytes,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def prepare_workspace() -> None:
    shutil.rmtree(TMP_DIR, ignore_errors=True)
    SAMPLES_DIR.mkdir(parents=True, exist_ok=True)
    DOWNLOADS_DIR.mkdir(parents=True, exist_ok=True)


def cleanup() -> None:
    global api_process
    global api_log_file

    if api_process is not None and api_process.poll() is None:
        api_process.terminate()
        try:
            api_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            api_process.kill()
            api_process.wait(timeout=5)

    if api_log_file is not None and not api_log_file.closed:
        api_log_file.close()

    existing = run_command(
        ["docker", "ps", "-a", "--format", "{{.Names}}"],
        check=False,
    )
    if CONTAINER_NAME in existing.stdout.decode().splitlines():
        run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)


def handle_signal(signum: int, _frame) -> None:
    cleanup()
    raise SystemExit(128 + signum)


atexit.register(cleanup)
signal.signal(signal.SIGINT, handle_signal)
signal.signal(signal.SIGTERM, handle_signal)


def print_step(label: str) -> None:
    print(label, flush=True)


def ensure(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def parse_json(body: str):
    return json.loads(body) if body else None


def question_ids_from_body(body: str) -> list[str]:
    return [item["question_id"] for item in parse_json(body)]


def normalize_headers(items) -> dict[str, str]:
    return {key.lower(): value for key, value in items}


def pretty_json(value) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)


def format_body(value) -> tuple[str, str]:
    if value is None:
        return "text", "(empty)"
    if isinstance(value, (dict, list)):
        return "json", pretty_json(value)
    return "text", str(value)


def assert_question_query(
    label: str,
    path: str,
    expected_ids: list[str],
) -> None:
    _, body, _ = perform_request(label, 200, path=path)
    actual_ids = question_ids_from_body(body)
    ensure(
        sorted(actual_ids) == sorted(expected_ids),
        f"{label} should return {expected_ids}, got {actual_ids}",
    )
    validation_notes.append(f"{label} -> {actual_ids}")


def append_request_log(
    *,
    label: str,
    method: str,
    path: str,
    expected_status: int,
    actual_status: int,
    request_headers: dict[str, str],
    request_body,
    response_headers: dict[str, str],
    response_body,
) -> None:
    request_logs.append(
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
    label: str,
    expected_status: int,
    *,
    method: str = "GET",
    path: str,
    headers: dict[str, str] | None = None,
    body: bytes | None = None,
    request_body=None,
) -> tuple[int, str, dict[str, str]]:
    url = f"http://127.0.0.1:{API_PORT}{path}"
    request_headers = headers or {}
    request = urllib.request.Request(url, data=body, method=method, headers=request_headers)

    try:
        with urllib.request.urlopen(request) as response:
            status = response.status
            response_headers = normalize_headers(response.headers.items())
            response_body = response.read().decode("utf-8")
    except urllib.error.HTTPError as err:
        status = err.code
        response_headers = normalize_headers(err.headers.items())
        response_body = err.read().decode("utf-8", errors="replace")

    append_request_log(
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
    label: str,
    expected_status: int,
    *,
    method: str,
    path: str,
    payload: dict,
) -> tuple[int, str, dict[str, str]]:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    return perform_request(
        label,
        expected_status,
        method=method,
        path=path,
        headers={"content-type": "application/json"},
        body=body,
        request_body=payload,
    )


def inspect_zip_file(file_path: Path) -> dict:
    with zipfile.ZipFile(file_path, "r") as archive:
        names = archive.namelist()
        manifest = None
        if "manifest.json" in names:
            manifest = json.loads(archive.read("manifest.json").decode("utf-8"))
    return {
        "entries": names,
        "manifest": manifest,
    }


def multipart_request(
    label: str,
    expected_status: int,
    *,
    path: str,
    text_fields: dict[str, str] | None,
    field_name: str,
    file_path: Path,
    content_type: str,
) -> tuple[int, str, dict[str, str]]:
    boundary = f"----QBApiBoundary{uuid.uuid4().hex}"
    file_bytes = file_path.read_bytes()
    body = bytearray()
    for name, value in (text_fields or {}).items():
        body.extend(
            (
                f"--{boundary}\r\n"
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'
                f"{value}\r\n"
            ).encode("utf-8")
        )
    body.extend(
        (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="{field_name}"; filename="{file_path.name}"\r\n'
            f"Content-Type: {content_type}\r\n\r\n"
        ).encode("utf-8")
    )
    body.extend(file_bytes)
    body.extend(f"\r\n--{boundary}--\r\n".encode("utf-8"))

    return perform_request(
        label,
        expected_status,
        method="POST",
        path=path,
        headers={"content-type": f"multipart/form-data; boundary={boundary}"},
        body=bytes(body),
        request_body={"file": str(file_path), **(text_fields or {})},
    )


def binary_json_request(
    label: str,
    expected_status: int,
    *,
    path: str,
    payload: dict,
    output_path: Path,
) -> tuple[dict, list[str]]:
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
        append_request_log(
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
    zip_details = inspect_zip_file(output_path)
    ensure(
        zip_details["manifest"] is not None,
        f"{label} should include manifest.json",
    )

    append_request_log(
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


def build_sample_zips() -> list[Path]:
    zip_paths: list[Path] = []
    for spec in QUESTION_SPECS:
        zip_path = SAMPLES_DIR / spec["zip_name"]
        with zipfile.ZipFile(zip_path, "w") as archive:
            archive.writestr(spec["tex_name"], spec["tex_body"])
            for asset_path, content in spec["assets"].items():
                archive.writestr(asset_path, content)
        zip_paths.append(zip_path)
    return zip_paths


def wait_for_postgres() -> None:
    for _ in range(60):
        result = run_command(
            ["docker", "exec", CONTAINER_NAME, "pg_isready", "-U", "postgres", "-d", "qb"],
            check=False,
        )
        if result.returncode == 0:
            return
        import time

        time.sleep(1)
    run_command(["docker", "exec", CONTAINER_NAME, "pg_isready", "-U", "postgres", "-d", "qb"])


def wait_for_api() -> None:
    for _ in range(60):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}/health") as response:
                if response.status == 200:
                    return
        except Exception:
            import time

            time.sleep(1)
    with urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}/health") as response:
        ensure(response.status == 200, "health check should be 200")


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
    global api_log_file

    api_log_file = API_LOG_PATH.open("wb")
    env = os.environ.copy()
    env["QB_DATABASE_URL"] = DB_URL
    env["QB_BIND_ADDR"] = f"127.0.0.1:{API_PORT}"
    api_process = subprocess.Popen(
        ["cargo", "run"],
        cwd=ROOT_DIR,
        env=env,
        stdout=api_log_file,
        stderr=subprocess.STDOUT,
    )
    wait_for_api()


def validate_question_bundle(manifest: dict, names: list[str], question_ids: list[str]) -> None:
    ensure(manifest["kind"] == "question_bundle", "question bundle manifest kind mismatch")
    ensure(manifest["question_count"] == len(question_ids), "question bundle count mismatch")
    bundled_ids = [item["question_id"] for item in manifest["questions"]]
    ensure(bundled_ids == question_ids, "question bundle ids should preserve request order")
    for item in manifest["questions"]:
        expected_prefix = f"{item['metadata']['description']}_"
        ensure(item["directory"].startswith(expected_prefix), "question bundle directory should start with description")
        ensure(
            item["directory"] != item["question_id"],
            "question bundle directory should not use raw question id",
        )
        file_paths = {entry["zip_path"] for entry in item["files"]}
        ensure(
            all(path.startswith(f"{item['directory']}/") for path in file_paths),
            "question bundle files should live under the description directory",
        )
        ensure(any(path.endswith(".tex") for path in file_paths), "question bundle should include tex")
        ensure(any("/assets/" in path for path in file_paths), "question bundle should include assets")
        ensure(all(path in names for path in file_paths), "question bundle manifest paths must exist in zip")


def validate_paper_bundle(manifest: dict, names: list[str], paper_ids: list[str]) -> None:
    ensure(manifest["kind"] == "paper_bundle", "paper bundle manifest kind mismatch")
    ensure(manifest["paper_count"] == len(paper_ids), "paper bundle count mismatch")
    bundled_ids = [item["paper_id"] for item in manifest["papers"]]
    ensure(bundled_ids == paper_ids, "paper bundle ids should preserve request order")
    for item in manifest["papers"]:
        paper_prefix = f"{item['metadata']['description']}_"
        ensure(item["directory"].startswith(paper_prefix), "paper bundle directory should start with description")
        ensure(item["directory"] != item["paper_id"], "paper bundle directory should not use raw paper id")
        ensure(item["questions"], "paper bundle should include at least one question")
        for question in item["questions"]:
            question_dir = question["directory"]
            ensure(
                question_dir.startswith(f"{item['directory']}/"),
                "paper question directory should live under the paper directory",
            )
            expected_prefix = f"{question_dir}/"
            ensure(
                any(name.startswith(expected_prefix) for name in names),
                "paper bundle should include question folder contents",
            )


def markdown_code_block(value) -> str:
    language, text = format_body(value)
    suffix = "json" if language == "json" else "text"
    return f"```{suffix}\n{text}\n```"


def summarize_sample_inputs() -> list[dict]:
    summaries = []
    for spec in QUESTION_SPECS:
        summaries.append(
            {
                "slug": spec["slug"],
                "upload_file": str(SAMPLES_DIR / spec["zip_name"]),
                "zip_entries": [spec["tex_name"], *spec["assets"].keys()],
                "create_difficulty": spec["create_difficulty"],
                "metadata_patch": spec["patch"],
            }
        )
    return summaries


def write_report(status: str, error_text: str | None) -> None:
    generated_at = datetime.now(timezone.utc).isoformat()
    lines = [
        "# QB E2E Report",
        "",
        f"- Generated at: `{generated_at}`",
        f"- Status: `{status}`",
        f"- Report path: `{REPORT_PATH}`",
        f"- Artifacts directory: `{TMP_DIR}`",
        f"- API log: `{API_LOG_PATH}`",
        f"- Export output: `{EXPORT_PATH}`",
        f"- Quality output: `{QUALITY_PATH}`",
        f"- Downloaded zips directory: `{DOWNLOADS_DIR}`",
        "",
        "## Sample Inputs",
        "",
        markdown_code_block(summarize_sample_inputs()),
        "",
        "## Validation Notes",
        "",
    ]

    if validation_notes:
        lines.extend([f"- {note}" for note in validation_notes])
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

    for index, entry in enumerate(request_logs, start=1):
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

    REPORT_PATH.write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    prepare_workspace()
    run_status = "passed"
    run_error = None

    try:
        print_step("[1/8] Build multiple sample question zips")
        zip_paths = build_sample_zips()
        validation_notes.append(f"Built {len(zip_paths)} sample question zips under {SAMPLES_DIR}.")

        print_step("[2/8] Start PostgreSQL container")
        start_postgres_container()

        print_step("[3/8] Apply migration")
        apply_migration()

        print_step("[4/8] Start API")
        start_api()

        perform_request("GET /health", 200, path="/health")

        print_step("[5/8] Create and query multiple questions")
        question_ids: list[str] = []
        question_by_slug: dict[str, str] = {}
        multipart_request(
            "POST /questions missing description",
            400,
            path="/questions",
            text_fields=None,
            field_name="file",
            file_path=zip_paths[0],
            content_type="application/zip",
        )
        multipart_request(
            "POST /questions missing difficulty",
            400,
            path="/questions",
            text_fields={"description": QUESTION_SPECS[0]["create_description"]},
            field_name="file",
            file_path=zip_paths[0],
            content_type="application/zip",
        )
        multipart_request(
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
        multipart_request(
            "POST /questions invalid difficulty missing human",
            400,
            path="/questions",
            text_fields={
                "description": QUESTION_SPECS[0]["create_description"],
                "difficulty": json.dumps(
                    {"heuristic": {"score": 5}},
                    ensure_ascii=False,
                ),
            },
            field_name="file",
            file_path=zip_paths[0],
            content_type="application/zip",
        )
        multipart_request(
            "POST /questions invalid difficulty score",
            400,
            path="/questions",
            text_fields={
                "description": QUESTION_SPECS[0]["create_description"],
                "difficulty": json.dumps(
                    {"human": {"score": 11}},
                    ensure_ascii=False,
                ),
            },
            field_name="file",
            file_path=zip_paths[0],
            content_type="application/zip",
        )
        for spec, zip_path in zip(QUESTION_SPECS, zip_paths):
            _, body, _ = multipart_request(
                f"POST /questions ({spec['slug']})",
                200,
                path="/questions",
                text_fields={
                    "description": spec["create_description"],
                    "difficulty": json.dumps(
                        spec["create_difficulty"], ensure_ascii=False
                    ),
                },
                field_name="file",
                file_path=zip_path,
                content_type="application/zip",
            )
            question_id = parse_json(body)["question_id"]
            question_ids.append(question_id)
            question_by_slug[spec["slug"]] = question_id

        validation_notes.append(f"Created question ids: {question_by_slug}.")

        for spec in QUESTION_SPECS:
            question_id = question_by_slug[spec["slug"]]
            json_request(
                f"PATCH /questions/{question_id}",
                200,
                method="PATCH",
                path=f"/questions/{question_id}",
                payload=spec["patch"],
            )

        json_request(
            f"PATCH /questions/{question_by_slug['mechanics']} invalid difficulty",
            400,
            method="PATCH",
            path=f"/questions/{question_by_slug['mechanics']}",
            payload={"difficulty": {"heuristic": {"score": 5}}},
        )

        _, body, _ = perform_request(
            "GET /questions",
            200,
            path="/questions?limit=10&offset=0",
        )
        ensure(len(parse_json(body)) == 3, "question list should contain three questions")
        assert_question_query(
            "GET /questions?q=热学&difficulty_tag=human&difficulty_min=5&difficulty_max=5",
            "/questions?q=%E7%83%AD%E5%AD%A6&difficulty_tag=human&difficulty_min=5&difficulty_max=5",
            [question_by_slug["thermal"]],
        )
        assert_question_query(
            "GET /questions?category=T&tag=mechanics&difficulty_tag=human&difficulty_max=4",
            "/questions?category=T&tag=mechanics&difficulty_tag=human&difficulty_max=4",
            [question_by_slug["mechanics"]],
        )
        assert_question_query(
            "GET /questions?difficulty_tag=heuristic&difficulty_max=5",
            "/questions?difficulty_tag=heuristic&difficulty_max=5",
            [question_by_slug["mechanics"], question_by_slug["thermal"]],
        )
        assert_question_query(
            "GET /questions?tag=optics&difficulty_tag=symbolic&difficulty_min=8",
            "/questions?tag=optics&difficulty_tag=symbolic&difficulty_min=8",
            [question_by_slug["optics"]],
        )
        assert_question_query(
            "GET /questions?difficulty_tag=ml&difficulty_min=8&tag=optics&category=E",
            "/questions?difficulty_tag=ml&difficulty_min=8&tag=optics&category=E",
            [question_by_slug["optics"]],
        )

        perform_request(
            "GET /questions invalid difficulty range without tag",
            400,
            path="/questions?difficulty_min=5",
        )
        perform_request(
            "GET /questions invalid difficulty range order",
            400,
            path="/questions?difficulty_tag=human&difficulty_min=8&difficulty_max=3",
        )

        _, body, _ = perform_request(
            "GET /questions/{mechanics}",
            200,
            path=f"/questions/{question_by_slug['mechanics']}",
        )
        mechanics_detail = parse_json(body)
        ensure(
            mechanics_detail["difficulty"]["human"]["score"] == 4,
            "mechanics human difficulty should be updated to 4",
        )
        ensure(
            mechanics_detail["difficulty"]["heuristic"]["notes"] == "fast estimate",
            "mechanics heuristic notes should round-trip",
        )

        _, body, _ = perform_request(
            "GET /questions/{optics}",
            200,
            path=f"/questions/{question_by_slug['optics']}",
        )
        optics_detail = parse_json(body)
        ensure(
            optics_detail["difficulty"]["symbolic"]["score"] == 9,
            "optics symbolic difficulty should be present",
        )
        ensure(
            optics_detail["difficulty"]["ml"]["notes"] == "vision model struggle",
            "optics ml difficulty notes should round-trip",
        )

        validation_notes.append(
            "Question filters covered search, tag, category, difficulty tag, difficulty ranges, and invalid range validation."
        )

        print_step("[6/8] Create papers and validate bundle downloads")
        json_request(
            "POST /papers invalid description",
            400,
            method="POST",
            path="/papers",
            payload={
                "edition": "2026",
                "paper_type": "regular",
                "description": "bad/name",
                "question_ids": [
                    question_by_slug["mechanics"],
                    question_by_slug["optics"],
                ],
            },
        )
        _, body, _ = json_request(
            "POST /papers (mock-a)",
            200,
            method="POST",
            path="/papers",
            payload={
                "edition": "2026",
                "paper_type": "regular",
                "description": "综合训练试卷 A",
                "question_ids": [
                    question_by_slug["mechanics"],
                    question_by_slug["optics"],
                ],
            },
        )
        paper_a_id = parse_json(body)["paper_id"]

        _, body, _ = json_request(
            "POST /papers (mock-b)",
            200,
            method="POST",
            path="/papers",
            payload={
                "edition": "2026",
                "paper_type": "final",
                "description": "热学决赛卷",
                "question_ids": [
                    question_by_slug["optics"],
                    question_by_slug["thermal"],
                ],
            },
        )
        paper_b_id = parse_json(body)["paper_id"]
        paper_ids = [paper_a_id, paper_b_id]
        validation_notes.append(f"Created paper ids: {paper_ids}.")

        _, body, _ = perform_request("GET /papers", 200, path="/papers")
        ensure(len(parse_json(body)) == 2, "paper list should contain two papers")

        _, body, _ = perform_request(
            "GET /papers?q=热学",
            200,
            path="/papers?q=%E7%83%AD%E5%AD%A6",
        )
        ensure(paper_b_id in body, "paper description search should return paper B")

        _, body, _ = perform_request(
            "GET /papers?paper_type=final&category=E&tag=optics&q=热学",
            200,
            path="/papers?paper_type=final&category=E&tag=optics&q=%E7%83%AD%E5%AD%A6",
        )
        ensure(paper_b_id in body, "combined paper filters should return paper B")

        perform_request(
            "GET /papers/{paper_a}",
            200,
            path=f"/papers/{paper_a_id}",
        )

        _, body, _ = json_request(
            f"PATCH /papers/{paper_a_id}",
            200,
            method="PATCH",
            path=f"/papers/{paper_a_id}",
            payload={
                "description": "综合训练重排卷",
                "question_ids": [
                    question_by_slug["thermal"],
                    question_by_slug["mechanics"],
                    question_by_slug["optics"],
                ],
            },
        )
        ensure("综合训练重排卷" in body, "paper patch should update description")

        json_request(
            f"PATCH /papers/{paper_a_id} invalid description",
            400,
            method="PATCH",
            path=f"/papers/{paper_a_id}",
            payload={"description": "bad/name"},
        )

        assert_question_query(
            "GET /questions?paper_id={paper_a}",
            f"/questions?paper_id={urllib.parse.quote(paper_a_id)}",
            [
                question_by_slug["thermal"],
                question_by_slug["mechanics"],
                question_by_slug["optics"],
            ],
        )
        assert_question_query(
            "GET /questions?paper_id={paper_a}&difficulty_tag=human&difficulty_min=5",
            f"/questions?paper_id={urllib.parse.quote(paper_a_id)}&difficulty_tag=human&difficulty_min=5",
            [question_by_slug["thermal"], question_by_slug["optics"]],
        )
        assert_question_query(
            "GET /questions?paper_type=final&difficulty_tag=ml&difficulty_min=8&tag=optics",
            "/questions?paper_type=final&difficulty_tag=ml&difficulty_min=8&tag=optics",
            [question_by_slug["optics"]],
        )

        question_bundle_path = DOWNLOADS_DIR / "questions_bundle.zip"
        question_manifest, question_names = binary_json_request(
            "POST /questions/bundles",
            200,
            path="/questions/bundles",
            payload={"question_ids": question_ids},
            output_path=question_bundle_path,
        )
        validate_question_bundle(question_manifest, question_names, question_ids)
        validation_notes.append(f"Saved question bundle zip to {question_bundle_path}.")

        paper_bundle_path = DOWNLOADS_DIR / "papers_bundle.zip"
        paper_manifest, paper_names = binary_json_request(
            "POST /papers/bundles",
            200,
            path="/papers/bundles",
            payload={"paper_ids": paper_ids},
            output_path=paper_bundle_path,
        )
        validate_paper_bundle(paper_manifest, paper_names, paper_ids)
        validation_notes.append(f"Saved paper bundle zip to {paper_bundle_path}.")

        print_step("[7/8] Run ops APIs and delete created data")
        _, body, _ = json_request(
            "POST /exports/run",
            200,
            method="POST",
            path="/exports/run",
            payload={
                "format": "jsonl",
                "public": False,
                "output_path": str(EXPORT_PATH),
            },
        )
        ensure('"exported_questions": 3' in body or '"exported_questions":3' in body, "export should include three questions")

        _, body, _ = json_request(
            "POST /quality-checks/run",
            200,
            method="POST",
            path="/quality-checks/run",
            payload={"output_path": str(QUALITY_PATH)},
        )
        ensure("empty_papers" in body, "quality report should include empty_papers")

        perform_request(
            f"DELETE /papers/{paper_b_id}",
            200,
            method="DELETE",
            path=f"/papers/{paper_b_id}",
        )
        perform_request(
            f"DELETE /papers/{paper_a_id}",
            200,
            method="DELETE",
            path=f"/papers/{paper_a_id}",
        )
        perform_request(
            f"GET /papers/{paper_a_id} after delete",
            404,
            path=f"/papers/{paper_a_id}",
        )

        perform_request(
            f"DELETE /questions/{question_by_slug['thermal']}",
            200,
            method="DELETE",
            path=f"/questions/{question_by_slug['thermal']}",
        )
        perform_request(
            f"DELETE /questions/{question_by_slug['optics']}",
            200,
            method="DELETE",
            path=f"/questions/{question_by_slug['optics']}",
        )
        perform_request(
            f"DELETE /questions/{question_by_slug['mechanics']}",
            200,
            method="DELETE",
            path=f"/questions/{question_by_slug['mechanics']}",
        )
        perform_request(
            f"GET /questions/{question_by_slug['optics']} after delete",
            404,
            path=f"/questions/{question_by_slug['optics']}",
        )

        validation_notes.append("CRUD, filtering, export, quality-check, and bundle download assertions all passed.")
    except Exception:
        run_status = "failed"
        run_error = traceback.format_exc()
        raise
    finally:
        print_step("[8/8] Write markdown report")
        write_report(run_status, run_error)


if __name__ == "__main__":
    main()
