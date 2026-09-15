-- Sanity checks for /api/pavlov/stats queries (PG15-compatible, read-only).
-- Run after migration 0013: docker run --rm -i postgres:16 psql "$DB_URL" -f - < scripts/verify-pavlov-stats.sql
-- Uses user_id = 1. "expect 0" checks are failures when nonzero.

-- A. Informational: tables exist and column shapes are right.
SELECT 'pavlov_reviews' AS t, count(*) AS rows FROM pavlov_reviews
UNION ALL SELECT 'pavlov_deck_snapshots', count(*) FROM pavlov_deck_snapshots;
SELECT pavlov_target_date FROM users WHERE id = 1;

-- B. Split aggregates (mirror SPLIT_COLS in routes/pavlov_stats.rs).
SELECT COUNT(*)::bigint AS total,
       COUNT(*) FILTER (WHERE rating <> 'wrong')::bigint AS correct,
       COUNT(*) FILTER (WHERE first_grade)::bigint AS cold_total,
       COUNT(*) FILTER (WHERE first_grade AND rating <> 'wrong')::bigint AS cold_correct,
       COUNT(*) FILTER (WHERE NOT first_grade)::bigint AS review_total,
       COUNT(*) FILTER (WHERE NOT first_grade AND rating <> 'wrong')::bigint AS review_correct
FROM pavlov_reviews WHERE user_id = 1;

-- C. Daily buckets in the user's zone.
SELECT (reviewed_at AT TIME ZONE 'America/Los_Angeles')::date AS date, count(*)
FROM pavlov_reviews WHERE user_id = 1 AND reviewed_at >= now() - interval '30 days'
GROUP BY 1 ORDER BY 1;

-- D. Category rollup joins cleanly (expect 0 orphan reviews).
SELECT 'orphan_reviews' AS check, count(*) AS fail_rows
FROM pavlov_reviews pr LEFT JOIN pavlov_answers pa ON pa.id = pr.answer_id
WHERE pr.user_id = 1 AND pa.id IS NULL;

-- E. Deck buckets are exhaustive and exclusive (expect 0).
WITH b AS (
  SELECT COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state <> 'review') AS learning,
         COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days < 21) AS maturing,
         COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days >= 21) AS mastered,
         COUNT(*) FILTER (WHERE NOT suspended AND lapses >= 4) AS struggling,
         COUNT(*) FILTER (WHERE suspended) AS banished,
         COUNT(*) AS total
  FROM pavlov_cards WHERE user_id = 1)
SELECT 'bucket_sum_mismatch' AS check,
       CASE WHEN learning + maturing + mastered + struggling + banished = total THEN 0 ELSE 1 END AS fail_rows,
       learning, maturing, mastered, struggling, banished, total
FROM b;

-- F. Forecast shape (informational).
SELECT GREATEST((due AT TIME ZONE 'America/Los_Angeles')::date, (now() AT TIME ZONE 'America/Los_Angeles')::date) AS d, COUNT(*)
FROM pavlov_cards
WHERE user_id = 1 AND suspended = false
  AND (due AT TIME ZONE 'America/Los_Angeles')::date <= (now() AT TIME ZONE 'America/Los_Angeles')::date + 13
GROUP BY d ORDER BY d;

-- G. After the first post-deploy grade: every review row's card exists and
--    first_grade rows are unique per card (expect 0 for both).
SELECT 'review_without_card' AS check, count(*) AS fail_rows
FROM pavlov_reviews pr LEFT JOIN pavlov_cards pc ON pc.user_id = pr.user_id AND pc.answer_id = pr.answer_id
WHERE pr.user_id = 1 AND pc.user_id IS NULL;
SELECT 'duplicate_first_grade' AS check, count(*) AS fail_rows FROM (
  SELECT answer_id FROM pavlov_reviews WHERE user_id = 1 AND first_grade GROUP BY answer_id HAVING count(*) > 1
) d;
