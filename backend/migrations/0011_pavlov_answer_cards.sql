-- 0011: Pavlov v2.1 — per-answer cards + hint tier
-- (docs/superpowers/specs/2026-07-22-pavlov-answer-cards-design.md).
-- Idempotent. Destructive only to pavlov_cards rows (drill-state reset, approved).

ALTER TABLE pavlov_cues
  ADD COLUMN IF NOT EXISTS tier TEXT NOT NULL DEFAULT 'standard'
    CHECK (tier IN ('standard', 'hint'));

-- The card table: one row per answer, phrases denormalized at generation.
CREATE TABLE IF NOT EXISTS pavlov_answers (
  id               SERIAL PRIMARY KEY,
  answer_norm      TEXT NOT NULL UNIQUE,
  answer           TEXT NOT NULL,
  meta_category    TEXT NOT NULL,
  phrases          TEXT[] NOT NULL DEFAULT '{}',   -- display forms, standard-first
  phrase_tiers     TEXT[] NOT NULL DEFAULT '{}',   -- parallel: 'standard' | 'hint'
  score            REAL NOT NULL DEFAULT 0,        -- max support*prec over standard cues
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_pavlov_answers_category ON pavlov_answers (meta_category);

-- Re-key drill state to answers. Guarded: only fires while the old cue_id
-- shape exists; re-runs are no-ops.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
             WHERE table_name = 'pavlov_cards' AND column_name = 'cue_id') THEN
    DROP TABLE pavlov_cards;
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS pavlov_cards (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  answer_id     INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
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
  UNIQUE (user_id, answer_id)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_due     ON pavlov_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_created ON pavlov_cards (user_id, created_at);
