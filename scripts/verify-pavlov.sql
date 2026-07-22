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

-- D. expect 0: active standard-tier cues below the gate thresholds. (Hint-tier
--    cues are legitimately below these thresholds; excluded here.)
SELECT 'below_thresholds' AS check, count(*) AS fail_rows
FROM pavlov_cues
WHERE status = 'active' AND tier = 'standard' AND NOT (
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

-- H. expect 0: cards with empty or >3 phrases, or mismatched tier array.
SELECT 'card_phrase_shape' AS check, count(*) AS fail_rows
FROM pavlov_answers
WHERE cardinality(phrases) = 0 OR cardinality(phrases) > 3
   OR cardinality(phrases) <> cardinality(phrase_tiers);

-- I. expect 0: cards whose answer has no active standard cue.
SELECT 'card_without_standard_cue' AS check, count(*) AS fail_rows
FROM pavlov_answers pa
WHERE NOT EXISTS (
  SELECT 1 FROM pavlov_cues pc
  WHERE pc.answer_norm = pa.answer_norm
    AND pc.status = 'active' AND pc.tier = 'standard'
);

-- J. expect 0: hint cues outside the hint band (below hint floor or at/above
--    standard bar — those should have been tier 'standard').
SELECT 'hint_out_of_band' AS check, count(*) AS fail_rows
FROM pavlov_cues
WHERE tier = 'hint' AND status = 'active' AND NOT (
  (array_length(regexp_split_to_array(cue_stem, ' '), 1) = 2
     AND support >= 3 AND prec >= 0.4 AND NOT (support >= 4 AND prec >= 0.5))
  OR
  (array_length(regexp_split_to_array(cue_stem, ' '), 1) = 1
     AND support >= 5 AND prec >= 0.5 AND NOT (support >= 6 AND prec >= 0.6))
);

-- K. expect 0: card phrases leaking the answer; canary: Dylan Thomas card
--    contains 'Welsh poet'.
WITH words AS (
  SELECT pa.id, w.word
  FROM pavlov_answers pa,
       regexp_split_to_table(lower(pa.answer_norm), '[^a-z0-9]+') AS w(word)
  WHERE length(w.word) >= 4
)
SELECT 'card_phrase_leaks_answer' AS check, count(*) AS fail_rows
FROM pavlov_answers pa, unnest(pa.phrases) AS p(phrase)
WHERE p.phrase ~* ('\m' || regexp_replace(pa.answer_norm, '([.^$*+?()\[\]{}\\|])', '\\\1', 'g') || '\M')
   OR EXISTS (SELECT 1 FROM words w WHERE w.id = pa.id AND p.phrase ~* ('\m' || w.word || '\M'));

SELECT 'canary_dylan_thomas_card' AS check,
       CASE WHEN EXISTS (
         SELECT 1 FROM pavlov_answers
         WHERE answer_norm = 'dylan thomas' AND 'Welsh poet' = ANY(phrases)
       ) THEN 0 ELSE 1 END AS fail_rows;

-- L. expect 0: same-answer active cue pairs whose punctuation-normalized
--    token sets are subset-related (spelling-variant near-duplicates).
WITH toks AS (
  SELECT id, answer_norm,
         (SELECT array_agg(DISTINCT w ORDER BY w)
          FROM regexp_split_to_table(lower(regexp_replace(cue_display,'[^a-zA-Z0-9]+',' ','g')),' ') AS w
          WHERE w <> '') AS tokset
  FROM pavlov_cues WHERE status='active'
)
SELECT 'normalized_dup_pair' AS check, count(*) AS fail_rows
FROM toks a JOIN toks b
  ON a.answer_norm = b.answer_norm AND a.id < b.id
  AND (a.tokset <@ b.tokset OR b.tokset <@ a.tokset);

-- M. expect 0: card phrase pairs with normalized-token Jaccard >= 0.5
--    (same-fact duplicates the assembly filter should have removed).
WITH card_toks AS (
  SELECT pa.id, p.ord,
         (SELECT array_agg(DISTINCT w ORDER BY w)
          FROM regexp_split_to_table(lower(regexp_replace(p.phrase,'[^a-zA-Z0-9]+',' ','g')),' ') AS w
          WHERE w <> '') AS tokset
  FROM pavlov_answers pa, unnest(pa.phrases) WITH ORDINALITY AS p(phrase, ord)
)
SELECT 'card_same_fact_overlap' AS check, count(*) AS fail_rows
FROM card_toks a JOIN card_toks b ON a.id = b.id AND a.ord < b.ord
WHERE (SELECT count(*) FROM unnest(a.tokset) x WHERE x = ANY(b.tokset))::float
      / (cardinality(a.tokset) + cardinality(b.tokset)
         - (SELECT count(*) FROM unnest(a.tokset) x WHERE x = ANY(b.tokset))) >= 0.5;
