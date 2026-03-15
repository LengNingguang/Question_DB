from __future__ import annotations

SCHEMA_STATEMENTS = [
    """
    CREATE TABLE IF NOT EXISTS papers (
        paper_id TEXT PRIMARY KEY,
        year INTEGER NOT NULL,
        stage TEXT NOT NULL,
        title TEXT NOT NULL,
        source_pdf_path TEXT,
        is_official INTEGER NOT NULL DEFAULT 1,
        notes TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS questions (
        question_id TEXT PRIMARY KEY,
        paper_id TEXT NOT NULL,
        question_no TEXT NOT NULL,
        category TEXT NOT NULL CHECK(category IN ('theory', 'experiment')),
        latex_body TEXT NOT NULL,
        plain_text TEXT NOT NULL,
        answer_latex TEXT,
        answer_text TEXT,
        status TEXT NOT NULL CHECK(status IN ('raw', 'reviewed', 'published')),
        source_page_start INTEGER,
        source_page_end INTEGER,
        tags_json TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY (paper_id) REFERENCES papers(paper_id),
        UNIQUE (paper_id, question_no)
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS question_assets (
        asset_id TEXT PRIMARY KEY,
        question_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('statement_image', 'answer_image', 'figure')),
        file_path TEXT NOT NULL,
        sha256 TEXT NOT NULL,
        caption TEXT,
        sort_order INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        FOREIGN KEY (question_id) REFERENCES questions(question_id) ON DELETE CASCADE
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS question_stats (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        question_id TEXT NOT NULL,
        exam_session TEXT NOT NULL,
        participant_count INTEGER NOT NULL,
        avg_score REAL NOT NULL,
        score_std REAL NOT NULL,
        full_mark_rate REAL NOT NULL,
        zero_score_rate REAL NOT NULL,
        max_score REAL NOT NULL,
        min_score REAL NOT NULL,
        stats_source TEXT NOT NULL,
        stats_version TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY (question_id) REFERENCES questions(question_id),
        UNIQUE (question_id, exam_session, stats_version)
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS difficulty_scores (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        question_id TEXT NOT NULL,
        exam_session TEXT,
        manual_level TEXT,
        derived_score REAL,
        method TEXT NOT NULL,
        method_version TEXT NOT NULL,
        confidence REAL,
        feature_json TEXT NOT NULL DEFAULT '{}',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY (question_id) REFERENCES questions(question_id),
        UNIQUE (question_id, exam_session, method, method_version)
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS import_runs (
        run_id INTEGER PRIMARY KEY AUTOINCREMENT,
        run_label TEXT NOT NULL,
        bundle_path TEXT NOT NULL,
        dry_run INTEGER NOT NULL,
        status TEXT NOT NULL,
        item_count INTEGER NOT NULL DEFAULT 0,
        warning_count INTEGER NOT NULL DEFAULT 0,
        error_count INTEGER NOT NULL DEFAULT 0,
        details_json TEXT NOT NULL DEFAULT '{}',
        started_at TEXT NOT NULL,
        finished_at TEXT NOT NULL
    )
    """,
    "CREATE INDEX IF NOT EXISTS idx_questions_paper ON questions(paper_id)",
    "CREATE INDEX IF NOT EXISTS idx_question_stats_question ON question_stats(question_id)",
    "CREATE INDEX IF NOT EXISTS idx_question_assets_question ON question_assets(question_id)",
]
