-- PostgreSQL initial schema for the question-first model.

CREATE TABLE IF NOT EXISTS objects (
    object_id UUID PRIMARY KEY,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    mime_type TEXT,
    storage_class TEXT NOT NULL DEFAULT 'hot',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by TEXT,
    encryption TEXT NOT NULL DEFAULT 'sse'
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_objects_bucket_key ON objects(bucket, object_key);
CREATE UNIQUE INDEX IF NOT EXISTS ux_objects_hash_size ON objects(sha256, size_bytes);

CREATE TABLE IF NOT EXISTS object_blobs (
    object_id UUID PRIMARY KEY REFERENCES objects(object_id) ON DELETE CASCADE,
    content BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS questions (
    question_id TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    question_tex_object_id UUID REFERENCES objects(object_id),
    answer_tex_object_id UUID REFERENCES objects(object_id),
    search_text TEXT,
    status TEXT NOT NULL DEFAULT 'raw',
    tags_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS question_assets (
    asset_id TEXT PRIMARY KEY,
    question_id TEXT NOT NULL REFERENCES questions(question_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    object_id UUID NOT NULL REFERENCES objects(object_id),
    caption TEXT,
    sort_order INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (question_id, kind, object_id)
);

CREATE TABLE IF NOT EXISTS papers (
    paper_id TEXT PRIMARY KEY,
    edition TEXT NOT NULL,
    paper_type TEXT NOT NULL,
    title TEXT NOT NULL,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS paper_questions (
    paper_id TEXT NOT NULL REFERENCES papers(paper_id) ON DELETE CASCADE,
    question_id TEXT NOT NULL REFERENCES questions(question_id) ON DELETE CASCADE,
    sort_order INT NOT NULL,
    question_label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (paper_id, question_id),
    UNIQUE (paper_id, sort_order)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_paper_questions_label
    ON paper_questions (paper_id, question_label)
    WHERE question_label IS NOT NULL;

CREATE TABLE IF NOT EXISTS import_runs (
    run_id BIGSERIAL PRIMARY KEY,
    run_label TEXT NOT NULL,
    bundle_path TEXT NOT NULL,
    dry_run BOOLEAN NOT NULL,
    status TEXT NOT NULL,
    item_count INT NOT NULL DEFAULT 0,
    warning_count INT NOT NULL DEFAULT 0,
    error_count INT NOT NULL DEFAULT 0,
    details_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_questions_status ON questions(status);
CREATE INDEX IF NOT EXISTS idx_questions_tags_json ON questions USING GIN (tags_json);
CREATE INDEX IF NOT EXISTS idx_question_assets_question_id ON question_assets(question_id);
CREATE INDEX IF NOT EXISTS idx_paper_questions_paper_id ON paper_questions(paper_id);
CREATE INDEX IF NOT EXISTS idx_paper_questions_question_id ON paper_questions(question_id);
CREATE INDEX IF NOT EXISTS idx_import_runs_started_at ON import_runs(started_at);
