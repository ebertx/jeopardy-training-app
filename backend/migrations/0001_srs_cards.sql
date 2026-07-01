-- Spaced-repetition card state, one row per (user, clue), created on first sight.
CREATE TABLE IF NOT EXISTS srs_cards (
    id            SERIAL PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    question_id   INTEGER NOT NULL REFERENCES jeopardy_questions(id),
    state         TEXT NOT NULL DEFAULT 'learning',   -- learning | review | relearning
    interval_days DOUBLE PRECISION NOT NULL DEFAULT 0, -- current review interval (memory strength)
    ease          DOUBLE PRECISION NOT NULL DEFAULT 2.5, -- SM-2 ease factor
    due           TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_review   TIMESTAMPTZ,
    reps          INTEGER NOT NULL DEFAULT 0,
    lapses        INTEGER NOT NULL DEFAULT 0,
    step_index    SMALLINT NOT NULL DEFAULT 0,
    suspended     BOOLEAN NOT NULL DEFAULT false,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, question_id)
);

CREATE INDEX IF NOT EXISTS idx_srs_cards_user_due ON srs_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_srs_cards_user_suspended_due ON srs_cards (user_id, suspended, due);
CREATE INDEX IF NOT EXISTS idx_srs_cards_user_created ON srs_cards (user_id, created_at);

ALTER TABLE users ADD COLUMN IF NOT EXISTS new_cards_per_day INTEGER NOT NULL DEFAULT 20;
ALTER TABLE users ADD COLUMN IF NOT EXISTS timezone TEXT;
