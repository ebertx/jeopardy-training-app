-- Sanity checks for mock test weighted sampling (spec 2026-07-20).
-- Read-only. Run: tower-ssh "docker exec -i postgresql15 psql -U ebertx -d jeopardy" < scripts/verify-mock-blend.sql

-- 1. Canon draw: mean answer_freq of 200 weighted Literature picks vs pool mean.
WITH pool AS (
  SELECT jq.answer_freq FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Literature & Language'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
), draw AS (
  SELECT jq.answer_freq FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Literature & Language'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
  ORDER BY -ln(random()) / ln(1 + jq.answer_freq) LIMIT 200
)
SELECT 'canon: pool avg freq' AS metric, round(avg(answer_freq), 1) AS value FROM pool
UNION ALL
SELECT 'canon: draw avg freq', round(avg(answer_freq), 1) FROM draw;

-- 2. Recency draw: median air_date of 200 weighted Film/TV picks vs pool median.
WITH pool AS (
  SELECT jq.air_date FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Film, TV & Pop Culture'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
), draw AS (
  SELECT jq.air_date FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Film, TV & Pop Culture'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
  ORDER BY -ln(random()) * exp(0.11552 * EXTRACT(EPOCH FROM (now() - jq.air_date)) / 31557600.0) LIMIT 200
)
SELECT 'recency: pool median air_date' AS metric, percentile_cont(0.5) WITHIN GROUP (ORDER BY air_date)::text AS value FROM pool
UNION ALL
SELECT 'recency: draw median air_date', percentile_cont(0.5) WITHIN GROUP (ORDER BY air_date)::text FROM draw;
