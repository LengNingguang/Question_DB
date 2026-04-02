from pathlib import Path
import os


ROOT_DIR = Path(__file__).resolve().parents[2]
TMP_DIR = ROOT_DIR / "tmp"
SAMPLES_DIR = TMP_DIR / "samples"
DOWNLOADS_DIR = TMP_DIR / "downloads"
API_LOG_PATH = TMP_DIR / "qb_api_e2e.log"
EXPORT_PATH = TMP_DIR / "qb_e2e_internal.jsonl"
QUALITY_PATH = TMP_DIR / "qb_e2e_quality.json"
REPORT_PATH = TMP_DIR / "qb_e2e_report.md"
INVALID_PAPER_UPLOAD_PATH = SAMPLES_DIR / "paper_invalid_upload.bin"
REAL_TEST_ZIP_PATH = ROOT_DIR / "scripts" / "test.zip"
REAL_TEST2_ZIP_PATH = ROOT_DIR / "scripts" / "test2.zip"

CONTAINER_NAME = os.environ.get("CONTAINER_NAME", "qb-postgres-e2e")
POSTGRES_IMAGE = os.environ.get("POSTGRES_IMAGE", "postgres:14.1")
POSTGRES_PORT = os.environ.get("POSTGRES_PORT", "55433")
API_PORT = os.environ.get("API_PORT", "18080")
DB_URL = f"postgres://postgres:postgres@127.0.0.1:{POSTGRES_PORT}/qb"
