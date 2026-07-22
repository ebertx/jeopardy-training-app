# Pavlov Cues v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the v1 frequency/TF-IDF cue miner with recurrence + precision mining of (phrase → answer) pairs, so the deck contains only archetypal Pavlov cues ("Welsh poet" → Dylan Thomas).

**Architecture:** Migration 0010 builds a one-time n-gram corpus (`pavlov_clue_ngrams`, position-ordered stemmed unigrams/bigrams per clue, clue text only) and rebuilds `pavlov_cues` as one row per (phrase, answer) pair. The miner in `backend/src/pavlov.rs` selects candidates by support (recurrence within the answer's clues) and precision (corpus-wide), prunes redundant grams, and a small LLM pass renders surface forms only. Drill/browse routes and pages switch from `cuePhrases[]` to a single `cue`. A user-gated threshold preview sits between the migration and generation.

**Tech Stack:** Rust (axum, sqlx), Postgres 15+ (tsvector positions, window functions), SvelteKit (Svelte 5 runes, Tailwind), OpenAI JSON mode (`gpt-4o`).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-22-pavlov-cues-v2-design.md`. v1 drill/SRS/auth machinery is retained; only mining, cue shape, and cue-facing UI change.
- Answer normalization everywhere = 0008 expression verbatim: `lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))` (`question` = response, `answer` = clue text).
- N-gram corpus covers the FULL corpus (`archived = false AND question IS NOT NULL`) — precision denominators must count every occurrence. Candidate aggregation only is restricted to answers with `answer_freq >= 4`.
- Default thresholds (subject to the Task 3 preview gate; the controller passes the final values into Task 5's dispatch): bigrams `support >= 4 AND prec >= 0.5`; unigrams `support >= 6 AND prec >= 0.6`. Render batch size 15; model `gpt-4o`, temperature 0.3.
- `phrase_leaks_answer` (existing) is applied to the mined gram at selection AND to the rendered display at parse.
- No quotas or category seats — `blend::TARGET_WEIGHTS` is not used for selection. `seat_plan`/`SeatPlan`/`TOTAL_SEATS`/`MIN_FREQ`/`filter_self_terms` and their tests are removed from `pavlov.rs` (blend.rs itself is untouched — the mock test still uses it).
- Drill card API shape becomes `{cueId, cue, category}`. Cue listing rows: `{id, answer, category, cue, support, total, precision, suspended}`.
- New-cue introduction order: `-ln(random()) / ln(1 + support * prec)`.
- Drill handlers still never write `question_attempts`/`quiz_sessions`; honesty-mode reveal (`typed` optional) unchanged; SM-2/`answer_match`/allowance rules unchanged.
- Migrations applied manually: `scripts/apply-migration.sh backend/migrations/0010_pavlov_v2.sql`. Migration 0010 is destructive by design (drops v1 `pavlov_cues`/`pavlov_cards` — approved) but guarded so re-running after v2 generation does not wipe v2 data.
- Tests: `cd backend && cargo test`; frontend: `cd frontend && npm run build` (never commit `frontend/build/` changes — restore with `git checkout -- frontend/build`).
- Commit after every task, `feat(pavlov)`/`fix(pavlov)`/`test(pavlov)` style, with the repo's Claude Code co-author trailer.

---

### Task 1: Migration 0010 — n-gram corpus + v2 cue table

**Files:**
- Create: `backend/migrations/0010_pavlov_v2.sql`

**Interfaces:**
- Produces: `pavlov_clue_ngrams(clue_id, answer_norm, gram, n)` with indexes on `(gram)` and `(answer_norm)`; v2 `pavlov_cues` (per-pair, `UNIQUE (answer_norm, cue_stem)`); fresh empty `pavlov_cards` (schema identical to 0009).

- [ ] **Step 1: Write the migration**

```sql
-- 0010: Pavlov v2 — n-gram mining corpus + per-(phrase,answer) cue table
-- (docs/superpowers/specs/2026-07-22-pavlov-cues-v2-design.md).
-- NOTE: pavlov_clue_ngrams builds ~40-60M rows (position-ordered stemmed
-- unigrams + adjacent bigrams over ~530k clues, clue text only) — expect
-- minutes to tens of minutes and a few GB incl. indexes. Apply during low
-- use. The table may be dropped once cue generation stabilizes; it is kept
-- for threshold tuning.
-- DESTRUCTIVE (approved): drops v1 pavlov_cues + pavlov_cards. Guarded so a
-- re-run after v2 generation does NOT wipe v2 data (drop only fires while
-- the v1 cue_phrases column exists; ngram INSERT only fires when empty).

CREATE TABLE IF NOT EXISTS pavlov_clue_ngrams (
  clue_id     INTEGER NOT NULL,
  answer_norm TEXT NOT NULL,
  gram        TEXT NOT NULL,
  n           SMALLINT NOT NULL CHECK (n IN (1, 2))
);

INSERT INTO pavlov_clue_ngrams (clue_id, answer_norm, gram, n)
WITH toks AS (
  SELECT jq.id,
         lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) AS norm,
         l.lexeme, p.pos
  FROM jeopardy_questions jq,
       unnest(to_tsvector('english', coalesce(jq.answer, ''))) AS l(lexeme, positions, weights),
       unnest(l.positions) AS p(pos)
  WHERE jq.archived = false AND jq.question IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM pavlov_clue_ngrams)
), ordered AS (
  SELECT id, norm, lexeme, pos,
         lead(lexeme) OVER (PARTITION BY id ORDER BY pos) AS next_lexeme,
         lead(pos)    OVER (PARTITION BY id ORDER BY pos) AS next_pos
  FROM toks
)
SELECT id, norm, lexeme, 1 FROM toks
UNION ALL
SELECT id, norm, lexeme || ' ' || next_lexeme, 2
FROM ordered
WHERE next_lexeme IS NOT NULL AND next_pos = pos + 1;

CREATE INDEX IF NOT EXISTS idx_pcn_gram   ON pavlov_clue_ngrams (gram);
CREATE INDEX IF NOT EXISTS idx_pcn_answer ON pavlov_clue_ngrams (answer_norm);

-- v1 -> v2 cutover: drop only while the v1 shape is present.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
             WHERE table_name = 'pavlov_cues' AND column_name = 'cue_phrases') THEN
    DROP TABLE IF EXISTS pavlov_cards;
    DROP TABLE IF EXISTS pavlov_cues;
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS pavlov_cues (
  id               SERIAL PRIMARY KEY,
  answer           TEXT NOT NULL,          -- display form of the response
  answer_norm      TEXT NOT NULL,          -- 0008 normalization
  meta_category    TEXT NOT NULL,
  cue_stem         TEXT NOT NULL,          -- mined stemmed gram, e.g. 'welsh poet'
  cue_display      TEXT NOT NULL DEFAULT '', -- natural surface form (LLM-rendered)
  support          INTEGER NOT NULL,       -- distinct clues of this answer containing the gram
  total            INTEGER NOT NULL,       -- distinct clues corpus-wide containing the gram
  prec             REAL NOT NULL,          -- support::float / total
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'active', 'dropped')),
  model            TEXT NOT NULL DEFAULT '',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (answer_norm, cue_stem)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_status   ON pavlov_cues (status);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_category ON pavlov_cues (meta_category);

CREATE TABLE IF NOT EXISTS pavlov_cards (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  cue_id        INTEGER NOT NULL REFERENCES pavlov_cues(id) ON DELETE CASCADE,
  state         TEXT NOT NULL DEFAULT 'learning',
  interval_days DOUBLE PRECISION NOT NULL DEFAULT 0,
  ease          DOUBLE PRECISION NOT NULL DEFAULT 2.5,
  due           TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_review   TIMESTAMPTZ,
  reps          INTEGER NOT NULL DEFAULT 0,
  lapses        INTEGER NOT NULL DEFAULT 0,
  step_index    SMALLINT NOT NULL DEFAULT 0,
  suspended     BOOLEAN NOT NULL DEFAULT false,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, cue_id)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_due     ON pavlov_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_created ON pavlov_cards (user_id, created_at);
```

- [ ] **Step 2: Apply it**

Run: `scripts/apply-migration.sh backend/migrations/0010_pavlov_v2.sql`
Expected: exits 0. The ngram INSERT is the slow step (minutes+). Do not interrupt.

- [ ] **Step 3: Verify**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -c "SELECT n, count(*) FROM pavlov_clue_ngrams GROUP BY n" -c "\d pavlov_cues" -c "SELECT count(*) FROM pavlov_cues" -c "SELECT count(*) FROM pavlov_cards"`
Expected: n=1 and n=2 each in the tens of millions; `pavlov_cues` has `cue_stem`/`cue_display`/`support`/`total`/`prec` columns and 0 rows; `pavlov_cards` 0 rows.

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0010_pavlov_v2.sql
git commit -m "feat(pavlov): migration 0010 — ngram mining corpus, v2 per-pair cue table"
```

---

### Task 2: Threshold preview script

**Files:**
- Create: `scripts/preview-pavlov-v2.sql`

**Interfaces:**
- Consumes: `pavlov_clue_ngrams` (Task 1).
- Produces: counts, category mix, and samples at strict/default/loose settings. The controller runs it and presents the results at the Task 3 gate.

- [ ] **Step 1: Write the script**

```sql
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
```

- [ ] **Step 2: Run it**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -v ON_ERROR_STOP=1 -f - < scripts/preview-pavlov-v2.sql`
Expected: exits 0; all five result sets print. The sanity anchor: "welsh poet → dylan thomas" (or equivalent canonical cues) should appear in result 3.

- [ ] **Step 3: Commit**

```bash
git add scripts/preview-pavlov-v2.sql
git commit -m "test(pavlov): v2 threshold preview script"
```

---

### Task 3: USER GATE — threshold selection (controller, not a subagent)

No files. The controller presents Task 2's output to the user (deck sizes, category mix, top + marginal samples, what loosening buys) and the user picks strict/default/loose or custom values. The controller then:
- records the chosen values in the spec's §3 (edit + commit),
- passes them explicitly into the Task 5 dispatch (miner constants) and Task 7 dispatch (verify script thresholds).

Execution does not proceed past this task without the user's choice.

---

### Task 4: `pavlov.rs` v2 pure logic (TDD)

**Files:**
- Modify: `backend/src/pavlov.rs`

**Interfaces:**
- Consumes: existing `phrase_leaks_answer(answer, phrase) -> bool` (kept as-is with its tests).
- Removes: `seat_plan`, `SeatPlan`, `TOTAL_SEATS`, `MIN_FREQ`, `filter_self_terms`, `PolishInput`, `PolishOutcome`, `polish_prompts`, `parse_polish_response`, and their tests (the four seat-plan tests, two filter_self_terms tests, and four polish tests). `use crate::blend::...;` and `use crate::routes::mock_test::apportion;` imports go too.
- Produces:
  - `pub struct CueCandidate { pub answer_norm: String, pub gram: String, pub n: i16, pub support: i64, pub total: i64, pub prec: f64 }`
  - `pub fn prune_redundant(cands: Vec<CueCandidate>) -> Vec<CueCandidate>`
  - `pub struct RenderInput { pub answer: String, pub gram: String, pub sample_clues: Vec<String> }`
  - `pub struct RenderOutcome { pub answer: String, pub gram: String, pub keep: bool, pub display: String }`
  - `pub fn render_prompts(batch: &[RenderInput]) -> (String, String)`
  - `pub fn parse_render_response(v: &serde_json::Value) -> Vec<RenderOutcome>`

- [ ] **Step 1: Delete the removed items and their tests**

Delete from `backend/src/pavlov.rs`: the `use crate::blend::...` and `use crate::routes::mock_test::apportion;` imports; `TOTAL_SEATS`, `MIN_FREQ`, `SeatPlan`, `seat_plan`; `filter_self_terms`; `PolishInput`, `PolishOutcome`, `polish_prompts`, `parse_polish_response`; the pure-section `pub const POLISH_MODEL` (Task 5's DB-stage block re-declares it); and these tests: `seat_plan_covers_all_categories_and_sums_to_total`, `canon_categories_get_only_canon_seats`, `recency_categories_get_only_recency_seats`, `music_splits_seats_with_canon_taking_the_odd_one`, `filter_self_terms_drops_stems_of_the_answer`, `filter_self_terms_is_case_insensitive_and_keeps_order`, `polish_prompts_mention_every_answer_and_demand_json`, `parse_polish_response_accepts_wellformed_and_enforces_phrase_floor`, `parse_polish_response_caps_phrases_at_four_and_skips_nameless_items`, `parse_polish_response_of_garbage_is_empty`, `parse_polish_response_strips_leaking_phrases_and_demotes_below_floor`, and the `plan_for` helper. Keep `phrase_leaks_answer` and its three tests. (The DB stages will not compile until Task 5 — that is expected; comment out the bodies of `mine_stage`/`polish_stage`/`run_generation` with `todo!()` stubs only if needed to keep `cargo test pavlov::` runnable, and note it; Task 5 replaces them wholesale.)

- [ ] **Step 2: Write the failing tests**

Append inside the `tests` module:

```rust
    fn cand(answer: &str, gram: &str, n: i16, support: i64, total: i64) -> CueCandidate {
        CueCandidate {
            answer_norm: answer.to_string(),
            gram: gram.to_string(),
            n,
            support,
            total,
            prec: support as f64 / total as f64,
        }
    }

    #[test]
    fn prune_drops_token_subset_with_lower_score() {
        // "wood" (7/12 = 4.08 score) is a token-subset of "milk wood"
        // (6/7 = 5.14 score) for the same answer -> keep "milk wood".
        let out = prune_redundant(vec![
            cand("dylan thomas", "milk wood", 2, 6, 7),
            cand("dylan thomas", "wood", 1, 7, 12),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].gram, "milk wood");
    }

    #[test]
    fn prune_keeps_more_specific_gram_on_score_tie() {
        let out = prune_redundant(vec![
            cand("solomon", "wise", 1, 6, 12),
            cand("solomon", "wise king", 2, 6, 12),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].gram, "wise king");
    }

    #[test]
    fn prune_keeps_unrelated_grams_and_other_answers() {
        let out = prune_redundant(vec![
            cand("dylan thomas", "welsh poet", 2, 19, 25),
            cand("dylan thomas", "fern hill", 2, 6, 6),
            cand("solomon", "wise king", 2, 15, 17),
            // same-token unigram but for a DIFFERENT answer: not pruned
            cand("robert frost", "poet", 1, 9, 14),
        ]);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn render_prompts_carry_gram_answer_and_clues_and_demand_json() {
        let batch = vec![RenderInput {
            answer: "Dylan Thomas".into(),
            gram: "welsh poet".into(),
            sample_clues: vec!["This Welsh poet wrote 'Fern Hill'".into()],
        }];
        let (system, user) = render_prompts(&batch);
        assert!(system.contains("JSON"));
        assert!(system.to_lowercase().contains("never include the answer"));
        assert!(user.contains("welsh poet"));
        assert!(user.contains("Dylan Thomas"));
        assert!(user.contains("Fern Hill"));
    }

    #[test]
    fn parse_render_accepts_wellformed_and_drops_leaky_or_empty() {
        let v = serde_json::json!({
            "results": [
                { "answer": "Dylan Thomas", "gram": "welsh poet",
                  "keep": true, "display": "Welsh poet" },
                { "answer": "Dylan Thomas", "gram": "go gentl",
                  "keep": true, "display": "Dylan's go gentle" }, // leaks answer word
                { "answer": "Solomon", "gram": "wise king",
                  "keep": true, "display": "  " },                // empty render
                { "gram": "orphan", "keep": true, "display": "x" } // no answer: skipped
            ]
        });
        let out = parse_render_response(&v);
        assert_eq!(out.len(), 3);
        assert!(out[0].keep);
        assert_eq!(out[0].display, "Welsh poet");
        assert!(!out[1].keep, "display containing an answer word is demoted");
        assert!(!out[2].keep, "blank display is demoted");
    }

    #[test]
    fn parse_render_of_garbage_is_empty() {
        assert!(parse_render_response(&serde_json::json!({"nope": 1})).is_empty());
        assert!(parse_render_response(&serde_json::json!("string")).is_empty());
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd backend && cargo test pavlov::`
Expected: compile errors (types/functions not defined).

- [ ] **Step 4: Implement**

Add above the tests module:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct CueCandidate {
    pub answer_norm: String,
    pub gram: String,
    pub n: i16,
    pub support: i64,
    pub total: i64,
    pub prec: f64,
}

/// Redundancy pruning within each answer: when one gram's token set is a
/// subset of another's (e.g. "wood" vs "milk wood"), keep the higher score
/// (support * prec); on a tie keep the gram with more tokens (more specific).
/// Grams of different answers never prune each other.
pub fn prune_redundant(cands: Vec<CueCandidate>) -> Vec<CueCandidate> {
    use std::collections::HashSet;
    let toks: Vec<HashSet<&str>> = cands
        .iter()
        .map(|c| c.gram.split(' ').collect())
        .collect();
    let score = |c: &CueCandidate| c.support as f64 * c.prec;
    let mut dropped = vec![false; cands.len()];
    for i in 0..cands.len() {
        for j in 0..cands.len() {
            if i == j || dropped[i] || dropped[j] {
                continue;
            }
            if cands[i].answer_norm != cands[j].answer_norm {
                continue;
            }
            if !toks[i].is_subset(&toks[j]) && !toks[j].is_subset(&toks[i]) {
                continue;
            }
            // i and j are token-related: drop the weaker.
            let (si, sj) = (score(&cands[i]), score(&cands[j]));
            let drop_i = if si != sj {
                si < sj
            } else {
                toks[i].len() < toks[j].len()
            };
            if drop_i {
                dropped[i] = true;
            } else {
                dropped[j] = true;
            }
        }
    }
    cands
        .into_iter()
        .zip(dropped)
        .filter(|(_, d)| !d)
        .map(|(c, _)| c)
        .collect()
}

#[derive(Debug, Clone)]
pub struct RenderInput {
    pub answer: String,
    pub gram: String,
    pub sample_clues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RenderOutcome {
    pub answer: String,
    pub gram: String,
    pub keep: bool,
    pub display: String,
}

/// (system, user) prompts for one render batch. Cosmetic-only contract: the
/// LLM restores a stemmed phrase to its natural surface form, nothing more.
pub fn render_prompts(batch: &[RenderInput]) -> (String, String) {
    let system = "You restore stemmed Jeopardy! clue phrases to their natural surface form. \
For each item you receive a stemmed phrase, the answer it cues, and real clues containing \
the phrase. Return the phrase as it naturally appears in the clues \
(e.g. 'welsh poet' -> 'Welsh poet', 'go gentl' -> 'go gentle'). Render ONLY the given \
phrase — do not add other words or information, and NEVER include the answer or any word \
of the answer in the rendering. Set keep=false only when no natural rendering exists. \
Respond with JSON only: {\"results\": [{\"answer\": string (echoed verbatim), \
\"gram\": string (echoed verbatim), \"keep\": boolean, \"display\": string}]}"
        .to_string();
    let items: Vec<serde_json::Value> = batch
        .iter()
        .map(|b| {
            serde_json::json!({
                "answer": b.answer,
                "gram": b.gram,
                "sample_clues": b.sample_clues,
            })
        })
        .collect();
    let user = serde_json::to_string_pretty(&serde_json::json!({ "phrases": items }))
        .expect("serializable");
    (system, user)
}

/// Lenient parse of the render response: items missing answer or gram are
/// skipped; a blank display or one that leaks the answer demotes to dropped.
pub fn parse_render_response(v: &serde_json::Value) -> Vec<RenderOutcome> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|item| {
            let answer = item.get("answer")?.as_str()?.trim().to_string();
            let gram = item.get("gram")?.as_str()?.trim().to_string();
            if answer.is_empty() || gram.is_empty() {
                return None;
            }
            let display = item
                .get("display")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let keep = item.get("keep").and_then(|k| k.as_bool()).unwrap_or(true)
                && !display.is_empty()
                && !phrase_leaks_answer(&answer, &display);
            Some(RenderOutcome { answer, gram, keep, display })
        })
        .collect()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd backend && cargo test pavlov::`
Expected: 9 passed (3 kept `phrase_leaks_answer` tests + 6 new). DB-stage stubs may emit dead-code warnings; that's fine until Task 5.

- [ ] **Step 6: Commit**

```bash
git add backend/src/pavlov.rs
git commit -m "feat(pavlov): v2 pure logic — candidate pruning and render prompt/parse"
```

---

### Task 5: `pavlov.rs` v2 DB stages

**Files:**
- Modify: `backend/src/pavlov.rs`

**Interfaces:**
- Consumes: Task 4's types/functions; `crate::openai::chat_json`; tables from Task 1. Threshold constants come from the Task 3 gate — the controller's dispatch states the final values; the code below shows the defaults.
- Produces: `pub async fn run_generation(state: &Arc<AppState>) -> Result<(), AppError>` (same signature the admin route already calls — `routes/pavlov.rs` needs no change in this task).

- [ ] **Step 1: Replace the v1 DB stages wholesale**

Replace everything from `use std::sync::Arc;` down to (but excluding) the `#[cfg(test)]` module with:

```rust
use std::sync::Arc;

use crate::error::AppError;
use crate::AppState;

/// 0008's normalization of the response text, verbatim.
const NORM_EXPR: &str = "lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i')))";

// Thresholds fixed at the 2026-07-22 preview gate (spec §3).
pub const BIGRAM_MIN_SUPPORT: i64 = 4;
pub const BIGRAM_MIN_PREC: f64 = 0.5;
pub const UNIGRAM_MIN_SUPPORT: i64 = 6;
pub const UNIGRAM_MIN_PREC: f64 = 0.6;
const MIN_ANSWER_FREQ: i32 = 4;
const RENDER_BATCH: i64 = 15;
pub const POLISH_MODEL: &str = "gpt-4o";

#[derive(sqlx::FromRow)]
struct CandidateRow {
    answer_norm: String,
    gram: String,
    n: i16,
    support: i64,
    total: i64,
    prec: f64,
}

/// All qualifying (gram, answer) pairs. Support is counted within eligible
/// answers; totals (precision denominators) over the full corpus.
async fn candidate_rows(state: &Arc<AppState>) -> Result<Vec<CueCandidate>, AppError> {
    let sql = format!(
        "WITH eligible AS (
           SELECT {NORM_EXPR} AS norm
           FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.question IS NOT NULL
           GROUP BY 1 HAVING max(jq.answer_freq) >= $5
         ), sup AS (
           SELECT g.answer_norm, g.gram, g.n, count(DISTINCT g.clue_id) AS support
           FROM pavlov_clue_ngrams g
           WHERE g.answer_norm IN (SELECT norm FROM eligible)
           GROUP BY 1, 2, 3
           HAVING count(DISTINCT g.clue_id) >= LEAST($1, $3)
         ), tot AS (
           SELECT g.gram, count(DISTINCT g.clue_id) AS total
           FROM pavlov_clue_ngrams g
           WHERE g.gram IN (SELECT DISTINCT gram FROM sup)
           GROUP BY 1
         )
         SELECT s.answer_norm, s.gram, s.n, s.support, t.total,
                s.support::float8 / t.total AS prec
         FROM sup s JOIN tot t USING (gram)
         WHERE (s.n = 2 AND s.support >= $1 AND s.support::float8 / t.total >= $2)
            OR (s.n = 1 AND s.support >= $3 AND s.support::float8 / t.total >= $4)"
    );
    let rows: Vec<CandidateRow> = sqlx::query_as(&sql)
        .bind(BIGRAM_MIN_SUPPORT)
        .bind(BIGRAM_MIN_PREC)
        .bind(UNIGRAM_MIN_SUPPORT)
        .bind(UNIGRAM_MIN_PREC)
        .bind(MIN_ANSWER_FREQ)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| CueCandidate {
            answer_norm: r.answer_norm,
            gram: r.gram,
            n: r.n,
            support: r.support,
            total: r.total,
            prec: r.prec,
        })
        .collect())
}

/// Stage A: select candidates, filter leaks, prune redundancy, insert
/// 'pending' rows. Idempotent via ON CONFLICT DO NOTHING.
async fn mine_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    let raw = candidate_rows(state).await?;
    let unleaky: Vec<CueCandidate> = raw
        .into_iter()
        .filter(|c| !phrase_leaks_answer(&c.answer_norm, &c.gram))
        .collect();
    let kept = prune_redundant(unleaky);
    tracing::info!("pavlov v2 mine: {} cues after leak filter + pruning", kept.len());

    for c in kept {
        let (display, category): (String, Option<String>) = {
            let sql = format!(
                "SELECT mode() WITHIN GROUP (ORDER BY jq.question),
                        mode() WITHIN GROUP (ORDER BY jq.classifier_category)
                 FROM jeopardy_questions jq
                 WHERE jq.archived = false AND jq.question IS NOT NULL
                   AND {NORM_EXPR} = $1"
            );
            sqlx::query_as(&sql).bind(&c.answer_norm).fetch_one(&state.pool).await?
        };
        let examples: Vec<(i32,)> = sqlx::query_as(
            "SELECT g.clue_id FROM pavlov_clue_ngrams g
             JOIN jeopardy_questions jq ON jq.id = g.clue_id
             WHERE g.answer_norm = $1 AND g.gram = $2
             GROUP BY g.clue_id, jq.air_date
             ORDER BY jq.air_date DESC NULLS LAST LIMIT 3",
        )
        .bind(&c.answer_norm)
        .bind(&c.gram)
        .fetch_all(&state.pool)
        .await?;
        let example_ids: Vec<i32> = examples.into_iter().map(|(i,)| i).collect();

        sqlx::query(
            "INSERT INTO pavlov_cues
               (answer, answer_norm, meta_category, cue_stem, support, total, prec, example_clue_ids)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (answer_norm, cue_stem) DO NOTHING",
        )
        .bind(&display)
        .bind(&c.answer_norm)
        .bind(category.unwrap_or_else(|| "Miscellaneous".to_string()))
        .bind(&c.gram)
        .bind(c.support as i32)
        .bind(c.total as i32)
        .bind(c.prec as f32)
        .bind(&example_ids)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

/// Stage B: render surface forms for pending cues in batches; upserts per
/// batch so an interrupted run resumes where it left off.
async fn render_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    loop {
        let batch: Vec<(i32, String, String, Vec<i32>)> = sqlx::query_as(
            "SELECT id, answer, cue_stem, example_clue_ids
             FROM pavlov_cues WHERE status = 'pending' ORDER BY id LIMIT $1",
        )
        .bind(RENDER_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }

        let mut inputs = Vec::with_capacity(batch.len());
        for (_, answer, gram, ex_ids) in &batch {
            let clues: Vec<(String,)> = sqlx::query_as(
                "SELECT coalesce(answer, '') FROM jeopardy_questions WHERE id = ANY($1) LIMIT 2",
            )
            .bind(&ex_ids[..])
            .fetch_all(&state.pool)
            .await?;
            inputs.push(RenderInput {
                answer: answer.clone(),
                gram: gram.clone(),
                sample_clues: clues.into_iter().map(|(c,)| c).collect(),
            });
        }

        let (system, user) = render_prompts(&inputs);
        let response =
            crate::openai::chat_json(&state.config.openai_api_key, POLISH_MODEL, &system, &user, 0.3)
                .await?;
        let outcomes = parse_render_response(&response);

        let mut updated = 0;
        for out in &outcomes {
            let key = (out.answer.to_lowercase(), out.gram.as_str());
            let Some((id, ..)) = batch
                .iter()
                .find(|(_, a, g, _)| (a.to_lowercase(), g.as_str()) == key)
            else {
                continue;
            };
            let status = if out.keep { "active" } else { "dropped" };
            sqlx::query(
                "UPDATE pavlov_cues SET status = $2, cue_display = $3, model = $4
                 WHERE id = $1 AND status = 'pending'",
            )
            .bind(id)
            .bind(status)
            .bind(&out.display)
            .bind(POLISH_MODEL)
            .execute(&state.pool)
            .await?;
            updated += 1;
        }
        if updated == 0 {
            let ids: Vec<i32> = batch.iter().map(|(id, ..)| *id).collect();
            sqlx::query(
                "UPDATE pavlov_cues SET status = 'dropped', model = $2
                 WHERE id = ANY($1) AND status = 'pending'",
            )
            .bind(&ids)
            .bind(POLISH_MODEL)
            .execute(&state.pool)
            .await?;
            tracing::warn!("pavlov render: batch of {} unmatched, dropped", ids.len());
        }
    }
}

/// Full v2 generation: mine then render. Both stages idempotent/resumable.
pub async fn run_generation(state: &Arc<AppState>) -> Result<(), AppError> {
    mine_stage(state).await?;
    render_stage(state).await
}
```

- [ ] **Step 2: Compile + full suite**

Run: `cd backend && cargo test`
Expected: compiles; all tests pass (the pavlov module's 9 plus the rest of the suite).

- [ ] **Step 3: Commit**

```bash
git add backend/src/pavlov.rs
git commit -m "feat(pavlov): v2 miner — recurrence+precision candidates, render stage"
```

---

### Task 6: Routes + frontend for per-pair cues

**Files:**
- Modify: `backend/src/routes/pavlov.rs`
- Modify: `frontend/src/routes/pavlov/+page.svelte`
- Modify: `frontend/src/routes/pavlov/list/+page.svelte`

**Interfaces:**
- Consumes: v2 `pavlov_cues` columns (`cue_stem`, `cue_display`, `support`, `total`, `prec`).
- Produces: `GET /api/pavlov/cues` rows `{id, answer, category, cue, support, total, precision, suspended}`; drill card `{cueId, cue, category}`. All route paths unchanged.

- [ ] **Step 1: Update `routes/pavlov.rs`**

Replace `CueListRow` and the `cues` handler's row-mapping with:

```rust
#[derive(sqlx::FromRow)]
struct CueListRow {
    id: i32,
    answer: String,
    meta_category: String,
    cue_display: String,
    support: i32,
    total: i32,
    prec: f32,
    suspended: bool,
}
```

and the query/sort/json in `cues`:

```rust
    let mut rows: Vec<CueListRow> = sqlx::query_as(
        "SELECT pc.id, pc.answer, pc.meta_category, pc.cue_display,
                pc.support, pc.total, pc.prec,
                COALESCE(ca.suspended, false) AS suspended
         FROM pavlov_cues pc
         LEFT JOIN pavlov_cards ca ON ca.cue_id = pc.id AND ca.user_id = $1
         WHERE pc.status = 'active'",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;
    rows.sort_by(|a, b| {
        category_rank(&a.meta_category)
            .cmp(&category_rank(&b.meta_category))
            .then(
                (b.support as f32 * b.prec)
                    .partial_cmp(&(a.support as f32 * a.prec))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let cues: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id, "answer": r.answer, "category": r.meta_category,
                "cue": r.cue_display, "support": r.support, "total": r.total,
                "precision": r.prec, "suspended": r.suspended,
            })
        })
        .collect();
```

Replace `DrillCueRow`/`drill_card_json`:

```rust
#[derive(sqlx::FromRow)]
struct DrillCueRow {
    id: i32,
    cue_display: String,
    meta_category: String,
}

fn drill_card_json(r: DrillCueRow) -> Value {
    json!({ "cueId": r.id, "cue": r.cue_display, "category": r.meta_category })
}
```

In `drill_next`, update the two cue queries:

```rust
    let pick_new = "SELECT id, cue_display, meta_category FROM pavlov_cues
         WHERE status = 'active'
           AND id NOT IN (SELECT cue_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY -ln(random()) / ln(1 + support * prec) LIMIT 1";
    let fetch_due = "SELECT pc.id, pc.cue_display, pc.meta_category
         FROM pavlov_cards ca
         JOIN pavlov_cues pc ON pc.id = ca.cue_id AND pc.status = 'active'
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()
         ORDER BY ca.due ASC LIMIT 1";
```

`drill_check`, `drill_grade`, `suspend`, `generate`, `status` are unchanged (their columns still exist in v2).

- [ ] **Step 2: Update the drill page**

In `frontend/src/routes/pavlov/+page.svelte`: change the card state type and the phrase chips block:

```typescript
  let card = $state<{ cueId: number; cue: string; category: string } | null>(null);
```

and replace the `{#each card.cuePhrases ...}` block with:

```svelte
        <div class="mb-6">
          <span class="px-4 py-2 rounded-full border border-gray-300 text-xl text-gray-900 inline-block">{card.cue}</span>
        </div>
```

- [ ] **Step 3: Update the list page**

In `frontend/src/routes/pavlov/list/+page.svelte`: change the `Cue` type, search filter, and row template:

```typescript
  type Cue = {
    id: number; answer: string; category: string;
    cue: string; support: number; total: number; precision: number; suspended: boolean;
  };
```

search predicate:

```typescript
      return (
        c.answer.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        c.cue.toLowerCase().includes(q)
      );
```

row body (replacing the answer + phrase-chips block inside the per-cue div, keeping the suspend button as-is):

```svelte
            <div class="flex-1 min-w-0">
              <div class="text-gray-900">
                <span class="font-medium">{cue.cue}</span>
                <span class="text-gray-400 mx-1">→</span>
                <span>{cue.answer}</span>
              </div>
              <div class="text-xs text-gray-500 mt-0.5">
                in {cue.support} of its clues · {Math.round(cue.precision * 100)}% precise corpus-wide ({cue.support}/{cue.total})
              </div>
            </div>
```

- [ ] **Step 4: Build + test**

Run: `cd backend && cargo test` then `cd frontend && npm run build && git checkout -- build`
Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/pavlov.rs frontend/src/routes/pavlov/+page.svelte frontend/src/routes/pavlov/list/+page.svelte
git commit -m "feat(pavlov): per-pair cue routes and UI (cue -> answer with evidence)"
```

---

### Task 7: verify-pavlov.sql v2

**Files:**
- Modify: `scripts/verify-pavlov.sql` (full rewrite)

**Interfaces:**
- Consumes: populated v2 `pavlov_cues` + `pavlov_clue_ngrams`. Thresholds below are the defaults — the controller's dispatch states the gate-chosen values to substitute.

- [ ] **Step 1: Rewrite the script**

```sql
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
```

- [ ] **Step 2: Syntax-check against the DB (pre-generation: B–F return 0, G returns 1 — expected until generation runs)**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -v ON_ERROR_STOP=1 -f - < scripts/verify-pavlov.sql`
Expected: exits 0. Note in the report that G legitimately fails (1) before generation.

- [ ] **Step 3: Commit**

```bash
git add scripts/verify-pavlov.sql
git commit -m "test(pavlov): v2 sanity checks — thresholds, support recount, leak, canary"
```

---

### Task 8: End-to-end verification + deploy (controller)

No new files. Same runbook as v1's Task 10, run by the controller:

- [ ] `cd backend && cargo test` and `cd frontend && npm run build` clean.
- [ ] Local release backend on port 3100 against the dev DB; temp admin user; `POST /api/admin/pavlov/generate`; monitor to completion.
- [ ] `scripts/verify-pavlov.sql`: A plausible mix; B–F zero; G zero (canary present).
- [ ] Quality spot-check: 15 random active cues read as genuine Pavlov cues; paste a sample for the user.
- [ ] Browser QA: drill shows single phrase → reveal → self-rate; list shows phrase → answer with evidence numbers; suspend round-trips.
- [ ] Confirm no `question_attempts`/`quiz_sessions` writes from drilling; delete temp user; stop servers.
- [ ] finishing-a-development-branch: merge, push (ebertx account dance), CI, Tower pull/recreate, verify, changelog.
