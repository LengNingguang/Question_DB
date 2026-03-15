from __future__ import annotations

import sqlite3
from pathlib import Path
from typing import Iterable

from .schema import SCHEMA_STATEMENTS


def connect(db_path: Path) -> sqlite3.Connection:
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    return conn


def initialize_database(db_path: Path) -> None:
    with connect(db_path) as conn:
        for statement in SCHEMA_STATEMENTS:
            conn.execute(statement)
        conn.commit()


def fetch_all(conn: sqlite3.Connection, query: str, params: Iterable[object] | None = None) -> list[sqlite3.Row]:
    cursor = conn.execute(query, tuple(params or ()))
    return cursor.fetchall()
