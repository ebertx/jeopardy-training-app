-- 0006: shared primer library (LLM-generated long-form study guides).
CREATE TABLE IF NOT EXISTS primers (
  id           SERIAL PRIMARY KEY,
  slug         TEXT NOT NULL UNIQUE,
  topic        TEXT NOT NULL,
  content_md   TEXT NOT NULL,
  model        TEXT NOT NULL,
  source       TEXT NOT NULL DEFAULT 'custom' CHECK (source IN ('canon', 'blindspot', 'custom')),
  requested_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
