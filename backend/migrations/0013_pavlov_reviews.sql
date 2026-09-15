-- 0013: Pavlov per-grade review log (accuracy over time / cold-vs-review),
-- daily deck-composition snapshots (week-over-week deltas), and the user's
-- deck-completion target date for the dashboard progress card.
-- Additive only; safe to apply while the previous image is running.

CREATE TABLE IF NOT EXISTS pavlov_reviews (
  id          BIGSERIAL PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  answer_id   INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
  rating      TEXT NOT NULL CHECK (rating IN ('wrong', 'got_it', 'too_easy')),
  first_grade BOOLEAN NOT NULL,
  reviewed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS pavlov_reviews_user_time   ON pavlov_reviews (user_id, reviewed_at);
CREATE INDEX IF NOT EXISTS pavlov_reviews_user_answer ON pavlov_reviews (user_id, answer_id);

CREATE TABLE IF NOT EXISTS pavlov_deck_snapshots (
  user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  snap_date  DATE NOT NULL,
  learning   INTEGER NOT NULL,
  maturing   INTEGER NOT NULL,
  mastered   INTEGER NOT NULL,
  struggling INTEGER NOT NULL,
  PRIMARY KEY (user_id, snap_date)
);

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS pavlov_target_date DATE NOT NULL DEFAULT '2026-12-31';
