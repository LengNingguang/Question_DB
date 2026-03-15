from __future__ import annotations

import os
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = PROJECT_ROOT / "data"
SAMPLES_DIR = PROJECT_ROOT / "samples"

EXPORTS_DIR = Path(os.getenv("QUESTION_BANK_EXPORTS_DIR", str(PROJECT_ROOT / "exports"))).expanduser()
RAW_DIR = Path(os.getenv("QUESTION_BANK_RAW_DIR", str(PROJECT_ROOT / "raw"))).expanduser()
ASSETS_DIR = Path(os.getenv("QUESTION_BANK_ASSETS_DIR", str(PROJECT_ROOT / "assets"))).expanduser()
DEFAULT_DB_PATH = Path(
    os.getenv("QUESTION_BANK_DB_PATH", str(DATA_DIR / "question_bank.db"))
).expanduser()
