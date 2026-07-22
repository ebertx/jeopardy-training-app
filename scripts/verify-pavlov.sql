-- Sanity checks for Pavlov cue generation (PG15-compatible).
-- Run after generation: docker run --rm -i postgres:16 psql "$DB_URL" -f - < scripts/verify-pavlov.sql
-- Checks marked "expect 0" are failures when nonzero; others are informational.

-- A. Informational: seats filled per category vs blend weight (weights sum
--    to 100 over 1500 seats, so expect total ≈ weight * 15 per category).
SELECT meta_category,
       count(*)                                   AS total,
       count(*) FILTER (WHERE status = 'active')  AS active,
       count(*) FILTER (WHERE status = 'pending') AS pending,
       count(*) FILTER (WHERE status = 'dropped') AS dropped
FROM pavlov_cues
GROUP BY 1
ORDER BY total DESC;

-- B. expect 0: active cues without 2-4 phrases.
SELECT 'active_phrase_count_out_of_range' AS check, count(*) AS fail_rows
FROM pavlov_cues
WHERE status = 'active'
  AND (coalesce(array_length(cue_phrases, 1), 0) < 2
       OR coalesce(array_length(cue_phrases, 1), 0) > 4);

-- C. expect 0: duplicate normalized answers (belt-and-braces over the UNIQUE).
SELECT 'duplicate_answer_norm' AS check, count(*) AS fail_rows
FROM (SELECT answer_norm FROM pavlov_cues GROUP BY 1 HAVING count(*) > 1) d;

-- D. expect 0: cues below the frequency floor.
SELECT 'below_frequency_floor' AS check, count(*) AS fail_rows
FROM pavlov_cues WHERE answer_freq < 5;

-- E. expect 0 (sampled): top mined term does not occur in any of the answer's
--    clues. Verifies mining is grounded in the corpus.
SELECT 'mined_term_missing_from_clues' AS check, count(*) AS fail_rows
FROM (
  SELECT pc.answer_norm, pc.mined_terms[1] AS term
  FROM pavlov_cues pc
  WHERE pc.status <> 'dropped' AND cardinality(pc.mined_terms) > 0
  ORDER BY random()
  LIMIT 50
) s
WHERE NOT EXISTS (
  SELECT 1
  FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.question IS NOT NULL
    AND lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = s.answer_norm
    AND EXISTS (
      SELECT 1 FROM unnest(jq.search_tsv) AS u(lexeme, positions, weights)
      WHERE u.lexeme = s.term
    )
);

-- F. expect 0: mined term equal to a lexeme of the answer itself
--    (self-referential leak past the SQL + Rust filters), sampled.
SELECT 'self_referential_term' AS check, count(*) AS fail_rows
FROM (
  SELECT pc.answer, pc.mined_terms
  FROM pavlov_cues pc
  WHERE cardinality(pc.mined_terms) > 0
  ORDER BY random()
  LIMIT 200
) s
WHERE EXISTS (
  SELECT 1
  FROM unnest(to_tsvector('english', s.answer)) AS a(lexeme, positions, weights)
  WHERE a.lexeme = ANY (s.mined_terms)
);
