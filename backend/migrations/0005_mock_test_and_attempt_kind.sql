-- 0005: attempt_kind on question_attempts (cold-vs-review tracking) + mock test tables.
-- Idempotent: safe to re-run (also re-run at deploy cutover to reclassify any
-- attempts the old binary inserted with the default between migration and deploy).

ALTER TABLE question_attempts
  ADD COLUMN IF NOT EXISTS attempt_kind TEXT NOT NULL DEFAULT 'review'
  CHECK (attempt_kind IN ('new', 'review', 'mock'));

-- Backfill: earliest non-mock attempt per (user, question) = 'new'; the rest 'review'.
UPDATE question_attempts SET attempt_kind = 'review' WHERE attempt_kind = 'new';
WITH firsts AS (
  SELECT DISTINCT ON (user_id, question_id) id
  FROM question_attempts
  WHERE attempt_kind <> 'mock'
  ORDER BY user_id, question_id, answered_at ASC, id ASC
)
UPDATE question_attempts SET attempt_kind = 'new' WHERE id IN (SELECT id FROM firsts);

CREATE INDEX IF NOT EXISTS idx_qa_user_kind_time
  ON question_attempts (user_id, attempt_kind, answered_at);

CREATE TABLE IF NOT EXISTS mock_tests (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  session_id    INTEGER NOT NULL REFERENCES quiz_sessions(id),
  started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  completed_at  TIMESTAMPTZ,
  question_ids  INTEGER[] NOT NULL,
  current_index INTEGER NOT NULL DEFAULT 0,
  score         INTEGER
);
CREATE INDEX IF NOT EXISTS idx_mock_tests_user ON mock_tests (user_id, completed_at DESC);

CREATE TABLE IF NOT EXISTS mock_test_answers (
  id            SERIAL PRIMARY KEY,
  mock_test_id  INTEGER NOT NULL REFERENCES mock_tests(id) ON DELETE CASCADE,
  question_id   INTEGER NOT NULL REFERENCES jeopardy_questions(id),
  position      INTEGER NOT NULL,
  typed_answer  TEXT NOT NULL DEFAULT '',
  response_ms   INTEGER NOT NULL DEFAULT 0,
  auto_correct  BOOLEAN NOT NULL,
  overridden    BOOLEAN NOT NULL DEFAULT false,
  final_correct BOOLEAN NOT NULL,
  UNIQUE (mock_test_id, position)
);
