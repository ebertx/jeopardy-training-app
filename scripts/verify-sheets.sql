-- Read-only checks for answer sheets / fact cards / activity (PG15). Run after
-- migration 0015: docker run --rm -i postgres:16 psql "$DB_URL" -f - < scripts/verify-sheets.sql
-- "expect 0" checks fail when nonzero. user_id = 1.

-- A. Tables and columns exist.
SELECT 'answer_sheets' AS t, count(*) AS rows FROM answer_sheets;
SELECT kind, count(*) FROM pavlov_answers GROUP BY 1;
SELECT pavlov_auto_facts FROM users WHERE id = 1;

-- B. expect 0: fact rows with a missing/unknown parent.
SELECT 'fact_without_parent' AS check, count(*) AS fail_rows
FROM pavlov_answers f WHERE f.kind = 'fact'
  AND NOT EXISTS (SELECT 1 FROM pavlov_answers p WHERE p.answer_norm = f.parent_norm AND p.kind = 'answer')
  AND NOT EXISTS (SELECT 1 FROM answer_sheets s WHERE s.answer_norm = f.parent_norm);

-- C. Fact rows are well-formed (expect 0) + informational counts by kind.
SELECT 'malformed_fact_norm' AS check, count(*) AS fail_rows
FROM pavlov_answers WHERE kind = 'fact' AND (answer_norm NOT LIKE '%::_' OR parent_norm IS NULL);
SELECT kind, count(*) FROM pavlov_answers GROUP BY 1;
-- deckTotal/touched/pace/allowance exclusion of facts is verified in the live smoke:
-- progress.deckTotal and progress.touched must not change after "Drill these 4".

-- D. Fact-card upsert + card insert shape, rolled back (uses a synthetic parent).
BEGIN;
INSERT INTO answer_sheets (answer_norm, answer, content, model) VALUES
 ('__verify_parent', 'Verify Parent',
  '{"identity":"x","facts":[{"prompt":"a","response":"1"},{"prompt":"b","response":"2"},{"prompt":"c","response":"3"},{"prompt":"d","response":"4"}]}', 'test')
ON CONFLICT DO NOTHING;
INSERT INTO pavlov_answers (answer_norm, answer, meta_category, phrases, phrase_tiers, score, example_clue_ids, answer_freq, kind, parent_norm)
VALUES ('__verify_parent::1', '1', 'Miscellaneous', '{a}', '{standard}', 0, '{}', 1, 'fact', '__verify_parent')
ON CONFLICT (answer_norm) DO UPDATE SET answer = EXCLUDED.answer RETURNING id, kind, parent_norm;
INSERT INTO pavlov_cards (user_id, answer_id)
SELECT 1, id FROM pavlov_answers WHERE answer_norm = '__verify_parent::1'
ON CONFLICT (user_id, answer_id) DO NOTHING;
SELECT 'verify_card_due_now' AS check, count(*) FILTER (WHERE due > now() OR state <> 'learning' OR last_review IS NOT NULL) AS fail_rows
FROM pavlov_cards ca JOIN pavlov_answers pa ON pa.id = ca.answer_id WHERE pa.answer_norm = '__verify_parent::1';
ROLLBACK;

-- E. Activity series shape (mirror of routes/activity.rs).
WITH d AS (SELECT generate_series((now() AT TIME ZONE 'America/Los_Angeles')::date - 27, (now() AT TIME ZONE 'America/Los_Angeles')::date, '1 day')::date AS day),
act AS (
  SELECT (answered_at AT TIME ZONE 'America/Los_Angeles')::date AS day FROM question_attempts WHERE user_id = 1
  UNION SELECT (reviewed_at AT TIME ZONE 'America/Los_Angeles')::date FROM pavlov_reviews WHERE user_id = 1
  UNION SELECT (created_at AT TIME ZONE 'America/Los_Angeles')::date FROM pavlov_cards WHERE user_id = 1
  UNION SELECT (last_review AT TIME ZONE 'America/Los_Angeles')::date FROM pavlov_cards WHERE user_id = 1 AND last_review IS NOT NULL)
SELECT d.day, EXISTS (SELECT 1 FROM act WHERE act.day = d.day) AS active FROM d ORDER BY d.day;

-- F. Sheet-by-question norm resolution matches pavlov_answers for a deck answer (expect > 0 matches).
SELECT count(*) AS deck_answers_resolvable
FROM pavlov_answers pa WHERE pa.kind = 'answer' AND EXISTS (
  SELECT 1 FROM jeopardy_questions jq WHERE jq.id = pa.example_clue_ids[1]
    AND lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = pa.answer_norm);
