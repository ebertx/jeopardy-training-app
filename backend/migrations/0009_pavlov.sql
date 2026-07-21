-- 0009: Pavlov cues — mined trigger-keyword → answer associations + drill deck
-- (docs/superpowers/specs/2026-07-21-pavlov-cues-design.md).
-- NOTE: pavlov_term_df is built via ts_stat over ~530k tsvectors and
-- idx_jq_answer_norm is an expression index over the same table — expect the
-- first apply to take tens of seconds (like 0002). Apply during low use.
-- Idempotent: safe to re-run (term_df only populates when empty).

CREATE TABLE IF NOT EXISTS pavlov_cues (
  id               SERIAL PRIMARY KEY,
  answer           TEXT NOT NULL,          -- display form of the response
  answer_norm      TEXT NOT NULL UNIQUE,   -- 0008 normalization
  meta_category    TEXT NOT NULL,          -- classifier_category / blend meta-category
  cue_phrases      TEXT[] NOT NULL DEFAULT '{}',  -- LLM-polished (2-4 when active)
  mined_terms      TEXT[] NOT NULL DEFAULT '{}',  -- raw TF-IDF lexemes, kept for audit
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  answer_freq      INTEGER NOT NULL,
  status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'active', 'dropped')),
  model            TEXT NOT NULL DEFAULT '',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_status ON pavlov_cues (status);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_category ON pavlov_cues (meta_category);

-- Per-user drill state; same shape as srs_cards but keyed to cues.
CREATE TABLE IF NOT EXISTS pavlov_cards (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  cue_id        INTEGER NOT NULL REFERENCES pavlov_cues(id) ON DELETE CASCADE,
  state         TEXT NOT NULL DEFAULT 'learning',
  interval_days DOUBLE PRECISION NOT NULL DEFAULT 0,
  ease          DOUBLE PRECISION NOT NULL DEFAULT 2.5,
  due           TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_review   TIMESTAMPTZ,
  reps          INTEGER NOT NULL DEFAULT 0,
  lapses        INTEGER NOT NULL DEFAULT 0,
  step_index    SMALLINT NOT NULL DEFAULT 0,
  suspended     BOOLEAN NOT NULL DEFAULT false,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, cue_id)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_due ON pavlov_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_created ON pavlov_cards (user_id, created_at);

-- Corpus-wide document frequency of search_tsv lexemes, for TF-IDF term mining.
-- The one-time filter on the uncorrelated EXISTS lets Postgres skip the ts_stat
-- scan entirely on re-apply.
CREATE TABLE IF NOT EXISTS pavlov_term_df (
  word TEXT PRIMARY KEY,
  ndoc INTEGER NOT NULL
);
INSERT INTO pavlov_term_df (word, ndoc)
SELECT word, ndoc
FROM ts_stat('SELECT search_tsv FROM jeopardy_questions WHERE archived = false')
WHERE NOT EXISTS (SELECT 1 FROM pavlov_term_df);

-- Term mining and example lookup filter by normalized answer per candidate;
-- without this expression index each of ~1500 lookups is a seq scan.
CREATE INDEX IF NOT EXISTS idx_jq_answer_norm ON jeopardy_questions
  ((lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))))
  WHERE question IS NOT NULL;
