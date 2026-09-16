-- backend/migrations/0016_jeopardy_objects.sql
-- Jeopardy objects: entities, hooks, vetted import; removes answer sheets / fact cards.
-- Spec: docs/superpowers/specs/2026-09-16-jeopardy-objects-design.md

ALTER TABLE jeopardy_questions ADD COLUMN IF NOT EXISTS entity_norm TEXT;
CREATE INDEX IF NOT EXISTS idx_jq_entity_norm ON jeopardy_questions (entity_norm);

ALTER TABLE pavlov_answers
  ADD COLUMN IF NOT EXISTS forms  TEXT[]  NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS vetted BOOLEAN NOT NULL DEFAULT false,
  ADD COLUMN IF NOT EXISTS hooks_built_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS pavlov_hooks (
  id         SERIAL PRIMARY KEY,
  answer_id  INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
  key_gram   TEXT    NOT NULL,
  rank       INTEGER NOT NULL DEFAULT 1,
  cue        TEXT,
  grams      TEXT[]    NOT NULL DEFAULT '{}',
  clue_ids   INTEGER[] NOT NULL DEFAULT '{}',
  support    INTEGER NOT NULL DEFAULT 0,
  source     TEXT NOT NULL DEFAULT 'mined' CHECK (source IN ('mined','vetted','cue','model','both')),
  status     TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','dropped')),
  model      TEXT NOT NULL DEFAULT '',
  label_attempted_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (answer_id, key_gram)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_hooks_answer ON pavlov_hooks (answer_id, status);

ALTER TABLE pavlov_reviews
  ADD COLUMN IF NOT EXISTS hook_id INTEGER REFERENCES pavlov_hooks(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_pavlov_reviews_hook ON pavlov_reviews (user_id, answer_id, hook_id);

-- Removals: answer sheets / fact cards (spec §5). Fact rows cascade to cards + reviews.
DELETE FROM pavlov_answers WHERE kind = 'fact';
DROP TABLE IF EXISTS answer_sheets;
DROP INDEX IF EXISTS idx_pavlov_answers_kind_parent;
ALTER TABLE pavlov_answers DROP COLUMN IF EXISTS kind, DROP COLUMN IF EXISTS parent_norm;
ALTER TABLE users DROP COLUMN IF EXISTS pavlov_auto_facts;

-- Corpus document frequency per gram, filled by the hooks job on first run
-- (one GROUP BY over pavlov_clue_ngrams, minutes); read per entity after that.
CREATE TABLE IF NOT EXISTS pavlov_gram_df (
  gram TEXT PRIMARY KEY,
  df   INTEGER NOT NULL
);
