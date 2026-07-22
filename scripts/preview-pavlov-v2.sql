-- Threshold preview for Pavlov v2 cue mining. Read-only apart from temp
-- tables. Settings: strict (bigram s>=5,p>=0.6; unigram s>=8,p>=0.7),
-- default (4/0.5; 6/0.6), loose (3/0.4; 5/0.5).
-- Leak filtering and redundancy pruning happen in Rust, so real deck counts
-- land slightly below these numbers.

CREATE TEMP TABLE eligible AS
SELECT lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) AS norm
FROM jeopardy_questions
WHERE archived = false AND question IS NOT NULL
GROUP BY 1 HAVING max(answer_freq) >= 4;

CREATE TEMP TABLE cand AS
WITH sup AS (
  SELECT g.answer_norm, g.gram, g.n, count(DISTINCT g.clue_id) AS support
  FROM pavlov_clue_ngrams g
  WHERE g.answer_norm IN (SELECT norm FROM eligible)
  GROUP BY 1, 2, 3
  HAVING count(DISTINCT g.clue_id) >= 3
), tot AS (
  SELECT g.gram, count(DISTINCT g.clue_id) AS total
  FROM pavlov_clue_ngrams g
  WHERE g.gram IN (SELECT DISTINCT gram FROM sup)
  GROUP BY 1
)
SELECT s.answer_norm, s.gram, s.n, s.support, t.total,
       s.support::float8 / t.total AS prec
FROM sup s JOIN tot t USING (gram)
WHERE s.support::float8 / t.total >= 0.35;

CREATE TEMP TABLE ans_cat AS
SELECT lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) AS norm,
       mode() WITHIN GROUP (ORDER BY classifier_category) AS meta_category
FROM jeopardy_questions
WHERE archived = false AND question IS NOT NULL AND classifier_category IS NOT NULL
GROUP BY 1;

-- 1) Deck size at each setting.
SELECT 'strict'  AS setting,
       count(*) FILTER (WHERE (n=2 AND support>=5 AND prec>=0.6) OR (n=1 AND support>=8 AND prec>=0.7)) AS cues,
       count(DISTINCT answer_norm) FILTER (WHERE (n=2 AND support>=5 AND prec>=0.6) OR (n=1 AND support>=8 AND prec>=0.7)) AS answers
FROM cand
UNION ALL
SELECT 'default',
       count(*) FILTER (WHERE (n=2 AND support>=4 AND prec>=0.5) OR (n=1 AND support>=6 AND prec>=0.6)),
       count(DISTINCT answer_norm) FILTER (WHERE (n=2 AND support>=4 AND prec>=0.5) OR (n=1 AND support>=6 AND prec>=0.6))
FROM cand
UNION ALL
SELECT 'loose',
       count(*) FILTER (WHERE (n=2 AND support>=3 AND prec>=0.4) OR (n=1 AND support>=5 AND prec>=0.5)),
       count(DISTINCT answer_norm) FILTER (WHERE (n=2 AND support>=3 AND prec>=0.4) OR (n=1 AND support>=5 AND prec>=0.5))
FROM cand;

-- 2) Category mix at the default setting.
SELECT coalesce(ac.meta_category, '(uncategorized)') AS category, count(*) AS cues
FROM cand c LEFT JOIN ans_cat ac ON ac.norm = c.answer_norm
WHERE (c.n=2 AND c.support>=4 AND c.prec>=0.5) OR (c.n=1 AND c.support>=6 AND c.prec>=0.6)
GROUP BY 1 ORDER BY 2 DESC;

-- 3) Top 15 by evidence at the default setting.
SELECT gram, answer_norm, n, support, total, round(prec::numeric, 2) AS prec
FROM cand
WHERE (n=2 AND support>=4 AND prec>=0.5) OR (n=1 AND support>=6 AND prec>=0.6)
ORDER BY support * prec DESC LIMIT 15;

-- 4) 15 random default-setting cues from the bottom half by evidence
--    (the marginal quality the default bar admits).
WITH qual AS (
  SELECT gram, answer_norm, n, support, total, prec,
         percent_rank() OVER (ORDER BY support * prec) AS pr
  FROM cand
  WHERE (n=2 AND support>=4 AND prec>=0.5) OR (n=1 AND support>=6 AND prec>=0.6)
)
SELECT gram, answer_norm, n, support, total, round(prec::numeric, 2) AS prec
FROM qual WHERE pr < 0.5 ORDER BY random() LIMIT 15;

-- 5) Cues admitted by loose but rejected by default (what loosening buys).
SELECT gram, answer_norm, n, support, total, round(prec::numeric, 2) AS prec
FROM cand
WHERE ((n=2 AND support>=3 AND prec>=0.4) OR (n=1 AND support>=5 AND prec>=0.5))
  AND NOT ((n=2 AND support>=4 AND prec>=0.5) OR (n=1 AND support>=6 AND prec>=0.6))
ORDER BY random() LIMIT 15;
