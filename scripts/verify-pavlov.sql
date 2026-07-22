-- Sanity checks for Pavlov v2 cue generation (PG15-compatible).
-- Run after generation. "expect 0" checks are failures when nonzero.
-- Thresholds here must match the gate-chosen values in backend/src/pavlov.rs.

-- A. Informational: per-category active cues and distinct answers.
SELECT meta_category,
       count(*) FILTER (WHERE status = 'active')  AS active_cues,
       count(DISTINCT answer_norm) FILTER (WHERE status = 'active') AS answers,
       count(*) FILTER (WHERE status = 'dropped') AS dropped,
       count(*) FILTER (WHERE status = 'pending') AS pending
FROM pavlov_cues GROUP BY 1 ORDER BY 2 DESC;

-- B. expect 0: active cues with a blank display.
SELECT 'active_blank_display' AS check, count(*) AS fail_rows
FROM pavlov_cues WHERE status = 'active' AND length(trim(cue_display)) = 0;

-- C. expect 0: duplicate (answer_norm, cue_stem) (belt-and-braces on UNIQUE).
SELECT 'duplicate_pair' AS check, count(*) AS fail_rows
FROM (SELECT answer_norm, cue_stem FROM pavlov_cues GROUP BY 1, 2 HAVING count(*) > 1) d;

-- D. expect 0: active cues below the gate thresholds.
SELECT 'below_thresholds' AS check, count(*) AS fail_rows
FROM pavlov_cues
WHERE status = 'active' AND NOT (
  (array_length(regexp_split_to_array(cue_stem, ' '), 1) = 2 AND support >= 4 AND prec >= 0.5)
  OR
  (array_length(regexp_split_to_array(cue_stem, ' '), 1) = 1 AND support >= 6 AND prec >= 0.6)
);

-- E. expect 0 (sampled 50): stored support disagrees with a recount from the
--    ngram corpus.
SELECT 'support_mismatch' AS check, count(*) AS fail_rows
FROM (
  SELECT id, answer_norm, cue_stem, support FROM pavlov_cues
  WHERE status <> 'dropped' ORDER BY random() LIMIT 50
) s
WHERE s.support <> (
  SELECT count(DISTINCT g.clue_id) FROM pavlov_clue_ngrams g
  WHERE g.answer_norm = s.answer_norm AND g.gram = s.cue_stem
);

-- F. expect 0: rendered display leaks the answer (whole answer word-boundary
--    or any >=4-char answer word).
WITH words AS (
  SELECT pc.id, w.word
  FROM pavlov_cues pc,
       regexp_split_to_table(lower(pc.answer_norm), '[^a-z0-9]+') AS w(word)
  WHERE pc.status = 'active' AND length(w.word) >= 4
)
SELECT 'display_leaks_answer' AS check, count(*) AS fail_rows
FROM pavlov_cues pc
WHERE pc.status = 'active'
  AND (
    pc.cue_display ~* ('\m' || regexp_replace(pc.answer_norm, '([.^$*+?()\[\]{}\\|])', '\\\1', 'g') || '\M')
    OR EXISTS (SELECT 1 FROM words w WHERE w.id = pc.id AND pc.cue_display ~* ('\m' || w.word || '\M'))
  );

-- G. expect 0: canary — the archetypal cue class must exist.
SELECT 'canary_welsh_poet_missing' AS check,
       CASE WHEN EXISTS (
         SELECT 1 FROM pavlov_cues
         WHERE answer_norm = 'dylan thomas' AND cue_stem = 'welsh poet' AND status = 'active'
       ) THEN 0 ELSE 1 END AS fail_rows;
