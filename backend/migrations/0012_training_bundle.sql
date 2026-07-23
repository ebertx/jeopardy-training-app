-- 0012: training optimization bundle — pavlov throughput setting + mock miss tags
-- (docs/superpowers/specs/2026-07-23-training-optimization-bundle-design.md). Idempotent.

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS pavlov_new_per_day INTEGER NOT NULL DEFAULT 20;

ALTER TABLE question_attempts
  ADD COLUMN IF NOT EXISTS miss_kind TEXT
    CHECK (miss_kind IN ('unknown', 'slow', 'wording'));
