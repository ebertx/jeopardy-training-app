-- Full-text search index over clue text + response + show-category name,
-- powering keyword topic drilling (websearch_to_tsquery).
--
-- NOTE: a STORED generated column triggers a one-time full-table rewrite
-- (ACCESS EXCLUSIVE lock) on ~530k rows plus the GIN build — order of tens of
-- seconds. Apply during low use.
ALTER TABLE jeopardy_questions ADD COLUMN IF NOT EXISTS search_tsv tsvector
  GENERATED ALWAYS AS (
    to_tsvector('english',
      coalesce(answer, '') || ' ' || coalesce(question, '') || ' ' || coalesce(category, ''))
  ) STORED;

CREATE INDEX IF NOT EXISTS idx_jq_search_tsv ON jeopardy_questions USING GIN (search_tsv);
