-- PostgreSQL initial schema for ZIP-uploaded questions and assembled papers.

CREATE TABLE IF NOT EXISTS objects (
    object_id UUID PRIMARY KEY,
    file_name TEXT NOT NULL,
    mime_type TEXT,
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    content BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS questions (
    question_id UUID PRIMARY KEY,
    source_tex_path TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'none' CHECK (category IN ('none', 'T', 'E')),
    status TEXT NOT NULL DEFAULT 'none' CHECK (status IN ('none', 'reviewed', 'used')),
    description TEXT NOT NULL CHECK (btrim(description) <> ''),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS question_files (
    question_file_id UUID PRIMARY KEY,
    question_id UUID NOT NULL REFERENCES questions(question_id) ON DELETE CASCADE,
    object_id UUID NOT NULL REFERENCES objects(object_id) ON DELETE CASCADE,
    file_kind TEXT NOT NULL CHECK (file_kind IN ('tex', 'asset')),
    file_path TEXT NOT NULL,
    mime_type TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (question_id, file_kind, file_path)
);

CREATE TABLE IF NOT EXISTS question_tags (
    question_id UUID NOT NULL REFERENCES questions(question_id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    sort_order INT NOT NULL,
    PRIMARY KEY (question_id, tag),
    UNIQUE (question_id, sort_order)
);

CREATE TABLE IF NOT EXISTS question_difficulties (
    question_id UUID NOT NULL REFERENCES questions(question_id) ON DELETE CASCADE,
    algorithm_tag TEXT NOT NULL,
    score INT NOT NULL CHECK (score BETWEEN 1 AND 10),
    notes TEXT,
    PRIMARY KEY (question_id, algorithm_tag)
);

CREATE TABLE IF NOT EXISTS papers (
    paper_id UUID PRIMARY KEY,
    description TEXT NOT NULL CHECK (btrim(description) <> ''),
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    subtitle TEXT NOT NULL CHECK (btrim(subtitle) <> ''),
    authors TEXT[] NOT NULL DEFAULT '{}',
    reviewers TEXT[] NOT NULL DEFAULT '{}',
    append_object_id UUID NOT NULL REFERENCES objects(object_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS paper_questions (
    paper_id UUID NOT NULL REFERENCES papers(paper_id) ON DELETE CASCADE,
    question_id UUID NOT NULL REFERENCES questions(question_id) ON DELETE CASCADE,
    sort_order INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (paper_id, question_id),
    UNIQUE (paper_id, sort_order)
);

CREATE INDEX IF NOT EXISTS idx_questions_status ON questions(status);
CREATE INDEX IF NOT EXISTS idx_question_files_question_id ON question_files(question_id);
CREATE INDEX IF NOT EXISTS idx_question_tags_question_id ON question_tags(question_id);
CREATE INDEX IF NOT EXISTS idx_question_difficulties_question_id
    ON question_difficulties(question_id);
CREATE INDEX IF NOT EXISTS idx_question_difficulties_algorithm_tag_score
    ON question_difficulties(algorithm_tag, score);
CREATE INDEX IF NOT EXISTS idx_paper_questions_paper_id ON paper_questions(paper_id);
CREATE INDEX IF NOT EXISTS idx_paper_questions_question_id ON paper_questions(question_id);
