-- Read-only checks for the Jeopardy objects rollout (PG15-compatible).
-- Run: docker run --rm -i -v "$PWD/backend/data:/data" postgres:16 psql "$DB_URL" -f - < scripts/verify-objects.sql
-- "expect 0" checks are failures when nonzero. Uses user_id = 1.

-- A. Resolution coverage: every non-archived clue has an entity key (expect 0 after resolve).
SELECT 'clues_without_entity_norm' AS check, count(*) AS fail_rows
FROM jeopardy_questions WHERE archived = false AND question IS NOT NULL AND entity_norm IS NULL;

-- B. JBoard coverage before/after (informational). "in_deck" is the number to report.
CREATE TEMP TABLE vetted_pairs (cue TEXT, response TEXT, domain TEXT, source_url TEXT);
\copy vetted_pairs FROM '/data/pavlovs-jboard.tsv' WITH (FORMAT text, HEADER)
WITH keyed AS (
  SELECT response,
         lower(trim(regexp_replace(regexp_replace(regexp_replace(regexp_replace(regexp_replace(response,
           '\([^)]*\)', '', 'g'), '"[^"]*"', '', 'g'), '\s+', ' ', 'g'), '^\s*(sir|dame) ', '', 'i'), '^(the|a|an) ', '', 'i'))) AS key
  FROM vetted_pairs
), resolved AS (
  SELECT k.response, k.key,
         (SELECT jq.entity_norm FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.entity_norm IS NOT NULL
             AND (jq.entity_norm = k.key
                  OR lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = k.key)
           LIMIT 1) AS entity_norm
  FROM keyed k
)
SELECT count(*) AS pairs,
       count(*) FILTER (WHERE entity_norm IS NOT NULL) AS in_corpus,
       count(*) FILTER (WHERE EXISTS (SELECT 1 FROM pavlov_answers pa WHERE pa.answer_norm = resolved.entity_norm)) AS in_deck
FROM resolved;
-- Unmatched vetted responses, for hand review.
SELECT response, key FROM (
  SELECT k.response, k.key, (SELECT 1 FROM jeopardy_questions jq WHERE jq.archived = false
     AND (jq.entity_norm = k.key OR lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = k.key) LIMIT 1) AS hit
  FROM (SELECT response, lower(trim(regexp_replace(regexp_replace(regexp_replace(regexp_replace(regexp_replace(response,
           '\([^)]*\)', '', 'g'), '"[^"]*"', '', 'g'), '\s+', ' ', 'g'), '^\s*(sir|dame) ', '', 'i'), '^(the|a|an) ', '', 'i'))) AS key FROM vetted_pairs) k
) u WHERE hit IS NULL ORDER BY response;

-- C. The 50 highest-frequency merges, for hand review of false positives.
SELECT answer, answer_norm, answer_freq, forms
FROM pavlov_answers WHERE cardinality(forms) > 1 ORDER BY answer_freq DESC LIMIT 50;

-- D. Deck integrity (expect 0 each).
SELECT 'orphan_cards' AS check, count(*) AS fail_rows
FROM pavlov_cards ca LEFT JOIN pavlov_answers pa ON pa.id = ca.answer_id WHERE pa.id IS NULL
UNION ALL
SELECT 'orphan_reviews', count(*)
FROM pavlov_reviews pr LEFT JOIN pavlov_answers pa ON pa.id = pr.answer_id WHERE pa.id IS NULL
UNION ALL
SELECT 'deck_rows_without_forms', count(*) FROM pavlov_answers WHERE forms = '{}'
UNION ALL
SELECT 'review_hook_on_other_entity', count(*)
FROM pavlov_reviews pr JOIN pavlov_hooks h ON h.id = pr.hook_id WHERE h.answer_id <> pr.answer_id
UNION ALL
SELECT 'undrillable_entities', count(*)
FROM pavlov_answers pa
WHERE cardinality(pa.phrases) = 0
  AND NOT EXISTS (SELECT 1 FROM pavlov_hooks h WHERE h.answer_id = pa.id AND h.status = 'active' AND h.cue IS NOT NULL);

-- E. Hooks: per-entity distribution, label sources, unlabeled count (informational).
SELECT n_hooks, count(*) AS entities FROM (
  SELECT pa.id, count(h.id) FILTER (WHERE h.status = 'active' AND h.cue IS NOT NULL) AS n_hooks
  FROM pavlov_answers pa LEFT JOIN pavlov_hooks h ON h.answer_id = pa.id GROUP BY pa.id
) d GROUP BY n_hooks ORDER BY n_hooks;
SELECT source, status, count(*) FILTER (WHERE cue IS NOT NULL) AS labeled, count(*) FILTER (WHERE cue IS NULL) AS unlabeled
FROM pavlov_hooks GROUP BY 1, 2 ORDER BY 1, 2;
SELECT 'entities_pending_hooks' AS check, count(*) FROM pavlov_answers WHERE hooks_built_at IS NULL;

-- F. Spot check: the Visigoths entity and its hooks.
SELECT pa.answer, pa.forms, pa.answer_freq, h.rank, h.key_gram, h.cue, h.support, h.source, h.status
FROM pavlov_answers pa LEFT JOIN pavlov_hooks h ON h.answer_id = pa.id
WHERE pa.answer_norm = 'visigoths' ORDER BY h.rank;

-- G. Hook coverage for user 1 (mirrors routes/pavlov_stats.rs).
SELECT
  (SELECT count(DISTINCT pr.hook_id) FROM pavlov_reviews pr JOIN pavlov_hooks h ON h.id = pr.hook_id
    WHERE pr.user_id = 1 AND h.status = 'active' AND h.cue IS NOT NULL) AS hooks_seen,
  (SELECT count(*) FROM pavlov_hooks h JOIN pavlov_cards ca ON ca.answer_id = h.answer_id
    WHERE ca.user_id = 1 AND ca.last_review IS NOT NULL AND h.status = 'active' AND h.cue IS NOT NULL) AS hooks_total;
