-- 0015: answer sheets (LLM "identity + four facts" per answer, corpus-grounded,
-- global cache), fact cards as flagged pavlov_answers rows, and the user's
-- auto-add-facts preference. Additive only.

CREATE TABLE IF NOT EXISTS answer_sheets (
  answer_norm TEXT PRIMARY KEY,
  answer      TEXT NOT NULL,
  content     JSONB NOT NULL,   -- {"identity": "...", "facts": [{"prompt","response"} x4]}
  model       TEXT NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE pavlov_answers
  ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'answer'
    CHECK (kind IN ('answer', 'fact')),
  ADD COLUMN IF NOT EXISTS parent_norm TEXT;
CREATE INDEX IF NOT EXISTS idx_pavlov_answers_kind_parent ON pavlov_answers (kind, parent_norm);

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS pavlov_auto_facts BOOLEAN NOT NULL DEFAULT false;
