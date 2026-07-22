-- 0010: Pavlov v2 — n-gram mining corpus + per-(phrase,answer) cue table
-- (docs/superpowers/specs/2026-07-22-pavlov-cues-v2-design.md).
-- NOTE: pavlov_clue_ngrams builds ~40-60M rows (position-ordered stemmed
-- unigrams + adjacent bigrams over ~530k clues, clue text only) — expect
-- minutes to tens of minutes and a few GB incl. indexes. Apply during low
-- use. The table may be dropped once cue generation stabilizes; it is kept
-- for threshold tuning.
-- DESTRUCTIVE (approved): drops v1 pavlov_cues + pavlov_cards. Guarded so a
-- re-run after v2 generation does NOT wipe v2 data (drop only fires while
-- the v1 cue_phrases column exists; ngram INSERT only fires when empty).

CREATE TABLE IF NOT EXISTS pavlov_clue_ngrams (
  clue_id     INTEGER NOT NULL,
  answer_norm TEXT NOT NULL,
  gram        TEXT NOT NULL,
  n           SMALLINT NOT NULL CHECK (n IN (1, 2))
);

INSERT INTO pavlov_clue_ngrams (clue_id, answer_norm, gram, n)
WITH toks AS (
  SELECT jq.id,
         lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) AS norm,
         l.lexeme, p.pos
  FROM jeopardy_questions jq,
       unnest(to_tsvector('english', coalesce(jq.answer, ''))) AS l(lexeme, positions, weights),
       unnest(l.positions) AS p(pos)
  WHERE jq.archived = false AND jq.question IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM pavlov_clue_ngrams)
), ordered AS (
  SELECT id, norm, lexeme, pos,
         lead(lexeme) OVER (PARTITION BY id ORDER BY pos) AS next_lexeme,
         lead(pos)    OVER (PARTITION BY id ORDER BY pos) AS next_pos
  FROM toks
)
SELECT id, norm, lexeme, 1 FROM toks
UNION ALL
SELECT id, norm, lexeme || ' ' || next_lexeme, 2
FROM ordered
WHERE next_lexeme IS NOT NULL AND next_pos = pos + 1;

CREATE INDEX IF NOT EXISTS idx_pcn_gram   ON pavlov_clue_ngrams (gram);
CREATE INDEX IF NOT EXISTS idx_pcn_answer ON pavlov_clue_ngrams (answer_norm);

-- v1 -> v2 cutover: drop only while the v1 shape is present.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
             WHERE table_name = 'pavlov_cues' AND column_name = 'cue_phrases') THEN
    DROP TABLE IF EXISTS pavlov_cards;
    DROP TABLE IF EXISTS pavlov_cues;
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS pavlov_cues (
  id               SERIAL PRIMARY KEY,
  answer           TEXT NOT NULL,          -- display form of the response
  answer_norm      TEXT NOT NULL,          -- 0008 normalization
  meta_category    TEXT NOT NULL,
  cue_stem         TEXT NOT NULL,          -- mined stemmed gram, e.g. 'welsh poet'
  cue_display      TEXT NOT NULL DEFAULT '', -- natural surface form (LLM-rendered)
  support          INTEGER NOT NULL,       -- distinct clues of this answer containing the gram
  total            INTEGER NOT NULL,       -- distinct clues corpus-wide containing the gram
  prec             REAL NOT NULL,          -- support::float / total
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'active', 'dropped')),
  model            TEXT NOT NULL DEFAULT '',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (answer_norm, cue_stem)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_status   ON pavlov_cues (status);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_category ON pavlov_cues (meta_category);

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
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_due     ON pavlov_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_created ON pavlov_cards (user_id, created_at);
