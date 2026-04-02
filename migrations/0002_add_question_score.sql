-- Add score column to questions table
-- Score is extracted from \begin{problem}[score]{title} in TeX files
-- Range: 0-400 (inclusive), optional (can be NULL), integer only

ALTER TABLE questions
ADD COLUMN score INTEGER
CHECK (score IS NULL OR (score >= 0 AND score <= 400));

-- Add index for score filtering
CREATE INDEX IF NOT EXISTS idx_questions_score ON questions(score);
