-- Blind-spot learning: per-clue insight cache (global, permanent) and
-- per-user blind-spot packs (primer + drill search query).
CREATE TABLE IF NOT EXISTS clue_insights (
    id           SERIAL PRIMARY KEY,
    question_id  INTEGER NOT NULL UNIQUE REFERENCES jeopardy_questions(id),
    content      JSONB NOT NULL,            -- {"insight": "...", "hook": "..."}
    model        TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS blindspot_packs (
    id           SERIAL PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    theme        TEXT NOT NULL,
    diagnosis    TEXT NOT NULL,
    primer       TEXT NOT NULL,
    search_query TEXT NOT NULL,
    match_count  INTEGER NOT NULL,
    miss_count   INTEGER NOT NULL DEFAULT 0,
    superseded   BOOLEAN NOT NULL DEFAULT false,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_blindspot_packs_user_active
  ON blindspot_packs (user_id, superseded, created_at DESC);
