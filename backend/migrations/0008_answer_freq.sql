-- 0008: answer_freq — corpus-wide count of each clue's normalized response.
-- Canonicity proxy for mock test sampling (spec 2026-07-20-mock-test-blend-design).
ALTER TABLE jeopardy_questions
  ADD COLUMN IF NOT EXISTS answer_freq INTEGER NOT NULL DEFAULT 1;

WITH freq AS (
  SELECT lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) AS norm,
         count(*) AS n
  FROM jeopardy_questions
  WHERE archived = false AND question IS NOT NULL
  GROUP BY 1
)
UPDATE jeopardy_questions jq
SET answer_freq = f.n
FROM freq f
WHERE jq.question IS NOT NULL
  AND lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = f.norm;
