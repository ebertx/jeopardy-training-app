# Pavlov Cues Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mine ~1,500 corpus-grounded "Pavlov cue" flashcards (signature trigger keywords → canonical answer), apportioned by Anytime Test category weights, drillable in a separate SRS deck with a browsable list page.

**Architecture:** A pure-logic module `backend/src/pavlov.rs` (seat planning, term filtering, LLM prompt/parse) plus a routes module `backend/src/routes/pavlov.rs` (admin generation job, cue listing, drill loop). Generation is a resumable background task (mirrors `blindspot_inflight`): stage A mines candidates + TF-IDF terms into `pavlov_cues` with `status='pending'`; stage B polishes batches of 15 through `openai::chat_json` into `status='active'|'dropped'`. Drill state lives in `pavlov_cards` (same shape as `srs_cards`) and reuses `srs::schedule`, `answer_match::is_correct`, and `practice::{day_start_utc, serve_new}`.

**Tech Stack:** Rust (axum, sqlx, tokio), Postgres 15+ (tsvector/ts_stat), SvelteKit (Svelte 5 runes, Tailwind), OpenAI Chat Completions (JSON mode, `gpt-4o`).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-21-pavlov-cues-design.md`. Accepted deviations: (1) `pavlov_cues.status` gains a `'pending'` value (needed for resumability); `model` defaults to `''` until polished. (2) The spec's single `POST /pavlov/drill/answer` is split into `POST /api/pavlov/drill/check` (grade typed text + reveal, no state change) and `POST /api/pavlov/drill/grade` (SM-2 rating) so a correct answer can still be rated "too easy" after the reveal.
- Total seats: **1500**; frequency floor: **`answer_freq >= 5`**; mined terms kept: **8**; polish batch size: **15**; polish model: **`gpt-4o`**, temperature **0.3**; cue phrases per active cue: **2–4**.
- Answer normalization everywhere = the 0008 expression, verbatim: `lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))` (in `jeopardy_questions`, `question` = the response, `answer` = the clue text).
- Recency decay = the mock-test constant: `exp(-0.11552 * EXTRACT(EPOCH FROM (now() - air_date)) / 31557600.0)` (6-year half-life).
- Drill attempts must NOT write `question_attempts` or `quiz_sessions`.
- Migrations are applied manually: `scripts/apply-migration.sh backend/migrations/0009_pavlov.sql` (dev DB from `.env`).
- All backend tests: `cd backend && cargo test`. Rust 2021, no new crate dependencies.
- Commit after every task; message style `feat(pavlov): ...` / `test(pavlov): ...` / `docs: ...`, ending with the Claude Code trailer used in this repo.

---

### Task 1: Migration 0009 — tables + term-df + norm index

**Files:**
- Create: `backend/migrations/0009_pavlov.sql`

**Interfaces:**
- Produces: tables `pavlov_cues`, `pavlov_cards`, `pavlov_term_df`; expression index `idx_jq_answer_norm`. All later tasks assume these exist.

- [ ] **Step 1: Write the migration**

```sql
-- 0009: Pavlov cues — mined trigger-keyword → answer associations + drill deck
-- (docs/superpowers/specs/2026-07-21-pavlov-cues-design.md).
-- NOTE: pavlov_term_df is built via ts_stat over ~530k tsvectors and
-- idx_jq_answer_norm is an expression index over the same table — expect the
-- first apply to take tens of seconds (like 0002). Apply during low use.
-- Idempotent: safe to re-run (term_df only populates when empty).

CREATE TABLE IF NOT EXISTS pavlov_cues (
  id               SERIAL PRIMARY KEY,
  answer           TEXT NOT NULL,          -- display form of the response
  answer_norm      TEXT NOT NULL UNIQUE,   -- 0008 normalization
  meta_category    TEXT NOT NULL,          -- classifier_category / blend meta-category
  cue_phrases      TEXT[] NOT NULL DEFAULT '{}',  -- LLM-polished (2-4 when active)
  mined_terms      TEXT[] NOT NULL DEFAULT '{}',  -- raw TF-IDF lexemes, kept for audit
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  answer_freq      INTEGER NOT NULL,
  status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'active', 'dropped')),
  model            TEXT NOT NULL DEFAULT '',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_status ON pavlov_cues (status);
CREATE INDEX IF NOT EXISTS idx_pavlov_cues_category ON pavlov_cues (meta_category);

-- Per-user drill state; same shape as srs_cards but keyed to cues.
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
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_due ON pavlov_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_created ON pavlov_cards (user_id, created_at);

-- Corpus-wide document frequency of search_tsv lexemes, for TF-IDF term mining.
-- The one-time filter on the uncorrelated EXISTS lets Postgres skip the ts_stat
-- scan entirely on re-apply.
CREATE TABLE IF NOT EXISTS pavlov_term_df (
  word TEXT PRIMARY KEY,
  ndoc INTEGER NOT NULL
);
INSERT INTO pavlov_term_df (word, ndoc)
SELECT word, ndoc
FROM ts_stat('SELECT search_tsv FROM jeopardy_questions WHERE archived = false')
WHERE NOT EXISTS (SELECT 1 FROM pavlov_term_df);

-- Term mining and example lookup filter by normalized answer per candidate;
-- without this expression index each of ~1500 lookups is a seq scan.
CREATE INDEX IF NOT EXISTS idx_jq_answer_norm ON jeopardy_questions
  ((lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))))
  WHERE question IS NOT NULL;
```

- [ ] **Step 2: Apply it**

Run: `scripts/apply-migration.sh backend/migrations/0009_pavlov.sql`
Expected: exits 0 (first run takes tens of seconds building term_df + index).

- [ ] **Step 3: Verify schema**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -c "\d pavlov_cues" -c "\d pavlov_cards" -c "SELECT count(*) FROM pavlov_term_df"`
Expected: both tables described with the columns above; `pavlov_term_df` count > 100000.

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0009_pavlov.sql
git commit -m "feat(pavlov): migration 0009 — cues, drill cards, term-df, norm index"
```

---

### Task 2: `pavlov.rs` — seat plan (pure, TDD)

**Files:**
- Create: `backend/src/pavlov.rs` (module start; grows in Tasks 3–4)
- Modify: `backend/src/main.rs` (add `mod pavlov;` next to the existing `mod blend;` line)

**Interfaces:**
- Consumes: `crate::blend::{TARGET_WEIGHTS, sampling_kind, split_seats, SamplingKind}`, `crate::routes::mock_test::apportion`.
- Produces: `pub struct SeatPlan { pub category: String, pub canon: i64, pub recency: i64 }`, `pub fn seat_plan(total: i64) -> Vec<SeatPlan>`, `pub const TOTAL_SEATS: i64 = 1500`, `pub const MIN_FREQ: i32 = 5`.

- [ ] **Step 1: Write the failing tests**

Create `backend/src/pavlov.rs`:

```rust
//! Pavlov cue mining: seat planning, TF-IDF term filtering, and LLM polish
//! (docs/superpowers/specs/2026-07-21-pavlov-cues-design.md).

use crate::blend::{sampling_kind, split_seats, SamplingKind, TARGET_WEIGHTS};
use crate::routes::mock_test::apportion;

pub const TOTAL_SEATS: i64 = 1500;
pub const MIN_FREQ: i32 = 5;

#[derive(Debug, Clone)]
pub struct SeatPlan {
    pub category: String,
    pub canon: i64,
    pub recency: i64,
}

pub fn seat_plan(total: i64) -> Vec<SeatPlan> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_for(cat: &str, plan: &[SeatPlan]) -> (i64, i64) {
        let p = plan.iter().find(|p| p.category == cat).expect("category present");
        (p.canon, p.recency)
    }

    #[test]
    fn seat_plan_covers_all_categories_and_sums_to_total() {
        let plan = seat_plan(1500);
        assert_eq!(plan.len(), TARGET_WEIGHTS.len());
        let sum: i64 = plan.iter().map(|p| p.canon + p.recency).sum();
        assert_eq!(sum, 1500);
    }

    #[test]
    fn canon_categories_get_only_canon_seats() {
        let plan = seat_plan(1500);
        // Literature & Language is 20/100 of 1500 = 300, all canon.
        assert_eq!(plan_for("Literature & Language", &plan), (300, 0));
    }

    #[test]
    fn recency_categories_get_only_recency_seats() {
        let plan = seat_plan(1500);
        // Film, TV & Pop Culture is 10/100 of 1500 = 150, all recency.
        assert_eq!(plan_for("Film, TV & Pop Culture", &plan), (0, 150));
    }

    #[test]
    fn music_splits_seats_with_canon_taking_the_odd_one() {
        let plan = seat_plan(1500);
        // Music & Performing Arts is 6/100 of 1500 = 90 → 45/45.
        let (canon, recency) = plan_for("Music & Performing Arts", &plan);
        assert_eq!(canon + recency, 90);
        assert!(canon >= recency);
        assert!(canon - recency <= 1);
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail**

In `backend/src/main.rs`, next to the existing module declarations (`mod blend;` etc.), add:

```rust
mod pavlov;
```

Run: `cd backend && cargo test pavlov::`
Expected: panics at `todo!()` — 4 failing tests.

- [ ] **Step 3: Implement `seat_plan`**

Replace the `todo!()` body:

```rust
pub fn seat_plan(total: i64) -> Vec<SeatPlan> {
    let dist: Vec<(String, i64)> = TARGET_WEIGHTS
        .iter()
        .map(|(c, w)| (c.to_string(), *w))
        .collect();
    apportion(&dist, total)
        .into_iter()
        .map(|(category, seats)| match sampling_kind(&category) {
            SamplingKind::Canon => SeatPlan { category, canon: seats, recency: 0 },
            SamplingKind::Recency => SeatPlan { category, canon: 0, recency: seats },
            SamplingKind::Split => {
                let (canon, recency) = split_seats(seats);
                SeatPlan { category, canon, recency }
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && cargo test pavlov::`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add backend/src/pavlov.rs backend/src/main.rs
git commit -m "feat(pavlov): seat plan from blend weights (1500 seats, canon/recency split)"
```

---

### Task 3: `pavlov.rs` — self-term filter + polish prompt/parse (pure, TDD)

**Files:**
- Modify: `backend/src/pavlov.rs`

**Interfaces:**
- Produces:
  - `pub fn filter_self_terms(answer: &str, terms: Vec<String>) -> Vec<String>`
  - `pub const POLISH_MODEL: &str = "gpt-4o";`
  - `pub struct PolishInput { pub answer: String, pub terms: Vec<String>, pub sample_clues: Vec<String> }`
  - `pub struct PolishOutcome { pub answer: String, pub keep: bool, pub phrases: Vec<String> }`
  - `pub fn polish_prompts(batch: &[PolishInput]) -> (String, String)` — (system, user)
  - `pub fn parse_polish_response(v: &serde_json::Value) -> Vec<PolishOutcome>`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `backend/src/pavlov.rs`:

```rust
    #[test]
    fn filter_self_terms_drops_stems_of_the_answer() {
        let terms = vec![
            "hemingway".to_string(), // shares ≥4-char prefix with answer word
            "bell".to_string(),      // < 4 chars overlap requirement, kept
            "spanish".to_string(),
        ];
        let kept = filter_self_terms("Ernest Hemingway", terms);
        assert_eq!(kept, vec!["bell".to_string(), "spanish".to_string()]);
    }

    #[test]
    fn filter_self_terms_is_case_insensitive_and_keeps_order() {
        let kept = filter_self_terms(
            "Solomon",
            vec!["wise".into(), "SOLOMONS".into(), "king".into()],
        );
        assert_eq!(kept, vec!["wise".to_string(), "king".to_string()]);
    }

    #[test]
    fn polish_prompts_mention_every_answer_and_demand_json() {
        let batch = vec![PolishInput {
            answer: "Solomon".into(),
            terms: vec!["wise".into(), "king".into(), "ecclesiast".into()],
            sample_clues: vec!["The book of Ecclesiastes is traditionally ascribed to this wise king".into()],
        }];
        let (system, user) = polish_prompts(&batch);
        assert!(system.contains("JSON"));
        assert!(user.contains("Solomon"));
        assert!(user.contains("ecclesiast"));
        assert!(user.contains("wise king")); // sample clue included
    }

    #[test]
    fn parse_polish_response_accepts_wellformed_and_enforces_phrase_floor() {
        let v = serde_json::json!({
            "results": [
                { "answer": "Solomon", "keep": true,
                  "cue_phrases": ["wise king", "Ecclesiastes ascribed to", "Temple builder"] },
                { "answer": "Junk", "keep": true, "cue_phrases": ["only one"] },
                { "answer": "Generic", "keep": false, "cue_phrases": [] }
            ]
        });
        let out = parse_polish_response(&v);
        assert_eq!(out.len(), 3);
        assert!(out[0].keep);
        assert_eq!(out[0].phrases.len(), 3);
        assert!(!out[1].keep, "keep with <2 phrases is demoted to dropped");
        assert!(!out[2].keep);
    }

    #[test]
    fn parse_polish_response_caps_phrases_at_four_and_skips_nameless_items() {
        let v = serde_json::json!({
            "results": [
                { "keep": true, "cue_phrases": ["a", "b"] },
                { "answer": "Nile", "keep": true,
                  "cue_phrases": ["longest river", "Egypt", "Aswan", "Khartoum", "delta"] }
            ]
        });
        let out = parse_polish_response(&v);
        assert_eq!(out.len(), 1, "item without an answer string is skipped");
        assert_eq!(out[0].phrases.len(), 4);
    }

    #[test]
    fn parse_polish_response_of_garbage_is_empty() {
        assert!(parse_polish_response(&serde_json::json!({"nope": 1})).is_empty());
        assert!(parse_polish_response(&serde_json::json!("string")).is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test pavlov::`
Expected: compile error (functions/types not defined).

- [ ] **Step 3: Implement**

Add to `backend/src/pavlov.rs` (above the tests module):

```rust
pub const POLISH_MODEL: &str = "gpt-4o";

/// Drop mined terms that are just stems/variants of the answer itself (the SQL
/// stage already removed exact lexeme matches; this catches near-variants).
/// Rule: a term is self-referential when it shares a common prefix of ≥ 4
/// chars with an answer word AND one is a prefix of the other (case-insensitive).
pub fn filter_self_terms(answer: &str, terms: Vec<String>) -> Vec<String> {
    let answer_words: Vec<String> = answer
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .map(|w| w.to_string())
        .collect();
    terms
        .into_iter()
        .filter(|t| {
            let tl = t.to_lowercase();
            !answer_words.iter().any(|w| {
                (tl.starts_with(w.as_str()) || w.starts_with(tl.as_str()))
                    && tl.len().min(w.len()) >= 4
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct PolishInput {
    pub answer: String,
    pub terms: Vec<String>,
    pub sample_clues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PolishOutcome {
    pub answer: String,
    pub keep: bool,
    pub phrases: Vec<String>,
}

/// (system, user) prompts for one polish batch. The system prompt pins the
/// JSON shape; the user prompt carries the mined evidence per answer.
pub fn polish_prompts(batch: &[PolishInput]) -> (String, String) {
    let system = "You turn mined Jeopardy! writer-habit data into study flashcards. \
For each answer you receive its most distinctive clue keywords (stemmed) and sample real clues. \
Write 2-4 short human-readable cue phrases per answer — the trigger associations a contestant \
should learn (e.g. for Solomon: \"wise king\", \"Ecclesiastes ascribed to\"). \
Every phrase must be grounded in the given keywords or sample clues; never invent associations. \
Set keep=false when the keywords are too generic or self-referential to make useful cues. \
Respond with JSON only: {\"results\": [{\"answer\": string (echoed verbatim), \
\"keep\": boolean, \"cue_phrases\": [string]}]}"
        .to_string();

    let items: Vec<serde_json::Value> = batch
        .iter()
        .map(|b| {
            serde_json::json!({
                "answer": b.answer,
                "mined_keywords": b.terms,
                "sample_clues": b.sample_clues,
            })
        })
        .collect();
    let user = serde_json::to_string_pretty(&serde_json::json!({ "answers": items }))
        .expect("serializable");
    (system, user)
}

/// Lenient parse: items without an answer string are skipped; phrases are
/// trimmed, de-blanked, capped at 4; keep with < 2 phrases demotes to dropped.
pub fn parse_polish_response(v: &serde_json::Value) -> Vec<PolishOutcome> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|item| {
            let answer = item.get("answer")?.as_str()?.trim().to_string();
            if answer.is_empty() {
                return None;
            }
            let mut phrases: Vec<String> = item
                .get("cue_phrases")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            phrases.truncate(4);
            let keep = item.get("keep").and_then(|k| k.as_bool()).unwrap_or(true)
                && phrases.len() >= 2;
            Some(PolishOutcome { answer, keep, phrases })
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && cargo test pavlov::`
Expected: 10 passed (4 from Task 2 + 6 new).

- [ ] **Step 5: Commit**

```bash
git add backend/src/pavlov.rs
git commit -m "feat(pavlov): self-term filter and LLM polish prompt/parse"
```

---

### Task 4: `pavlov.rs` — DB stages: mine + polish (`run_generation`)

**Files:**
- Modify: `backend/src/pavlov.rs`

**Interfaces:**
- Consumes: `crate::openai::chat_json`, `crate::AppState`, seat plan + pure helpers from Tasks 2–3.
- Produces: `pub async fn run_generation(state: &std::sync::Arc<crate::AppState>) -> Result<(), crate::error::AppError>` — used by the admin route in Task 5. Resumable: skips answers already in `pavlov_cues`; polishes only `status='pending'` rows.

DB-bound code in this repo is verified by SQL sanity script (Task 7) and compile + manual run, matching the mock-blend precedent — the pure parts were TDD'd in Tasks 2–3.

- [ ] **Step 1: Implement the mining + polish stages**

Add to `backend/src/pavlov.rs`:

```rust
use std::sync::Arc;

use crate::error::AppError;
use crate::AppState;

/// 0008's normalization of the response text, verbatim.
const NORM_EXPR: &str = "lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i')))";
const TERMS_RAW_LIMIT: i64 = 24; // fetched from SQL before the self-term filter
const TERMS_KEPT: usize = 8;
const POLISH_BATCH: i64 = 15;

#[derive(sqlx::FromRow)]
struct Candidate {
    norm: String,
    display: String,
    freq: i32,
}

/// Top unmined answers for one category. `recency=false` ranks by answer_freq;
/// `recency=true` by summed 6-year-half-life decay (mock-test constant).
async fn select_candidates(
    state: &Arc<AppState>,
    category: &str,
    seats: i64,
    recency: bool,
) -> Result<Vec<Candidate>, AppError> {
    let order = if recency { "recency_wt" } else { "freq" };
    let sql = format!(
        "SELECT norm, display, freq FROM (
           SELECT {NORM_EXPR} AS norm,
                  mode() WITHIN GROUP (ORDER BY jq.question) AS display,
                  max(jq.answer_freq) AS freq,
                  sum(exp(-0.11552 * EXTRACT(EPOCH FROM (now() - jq.air_date)) / 31557600.0)) AS recency_wt
           FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.question IS NOT NULL
             AND jq.air_date IS NOT NULL AND jq.classifier_category = $1
           GROUP BY 1
         ) t
         WHERE freq >= $2 AND norm NOT IN (SELECT answer_norm FROM pavlov_cues)
         ORDER BY {order} DESC
         LIMIT $3"
    );
    Ok(sqlx::query_as::<_, Candidate>(&sql)
        .bind(category)
        .bind(MIN_FREQ)
        .bind(seats)
        .fetch_all(&state.pool)
        .await?)
}

/// Distinctive clue lexemes for one answer: TF within the answer's clues ×
/// log-inverse document frequency corpus-wide, minus the answer's own lexemes.
async fn mine_terms(
    state: &Arc<AppState>,
    norm: &str,
    display: &str,
    total_docs: f64,
) -> Result<Vec<String>, AppError> {
    let sql = format!(
        "WITH clues AS (
           SELECT jq.search_tsv
           FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.question IS NOT NULL
             AND {NORM_EXPR} = $1
         ),
         lex AS (
           SELECT u.lexeme AS word, count(*)::float8 AS tf
           FROM clues, unnest(clues.search_tsv) AS u(lexeme, positions, weights)
           GROUP BY 1
         )
         SELECT l.word
         FROM lex l
         JOIN pavlov_term_df d ON d.word = l.word
         WHERE l.word NOT IN (
           SELECT a.lexeme
           FROM unnest(to_tsvector('english', $2)) AS a(lexeme, positions, weights)
         )
         ORDER BY l.tf * ln($3 / GREATEST(d.ndoc, 1)) DESC
         LIMIT $4"
    );
    let rows: Vec<(String,)> = sqlx::query_as(&sql)
        .bind(norm)
        .bind(display)
        .bind(total_docs)
        .bind(TERMS_RAW_LIMIT)
        .fetch_all(&state.pool)
        .await?;
    let mut terms =
        filter_self_terms(display, rows.into_iter().map(|(w,)| w).collect());
    terms.truncate(TERMS_KEPT);
    Ok(terms)
}

/// The 3 most recent clue ids for an answer (reveal examples).
async fn example_ids(state: &Arc<AppState>, norm: &str) -> Result<Vec<i32>, AppError> {
    let sql = format!(
        "SELECT jq.id FROM jeopardy_questions jq
         WHERE jq.archived = false AND jq.question IS NOT NULL AND {NORM_EXPR} = $1
         ORDER BY jq.air_date DESC NULLS LAST
         LIMIT 3"
    );
    let rows: Vec<(i32,)> = sqlx::query_as(&sql).bind(norm).fetch_all(&state.pool).await?;
    Ok(rows.into_iter().map(|(i,)| i).collect())
}

/// Stage A: fill every category's seats with mined 'pending' rows.
async fn mine_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    let total_docs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jeopardy_questions WHERE archived = false")
            .fetch_one(&state.pool)
            .await?;
    for plan in seat_plan(TOTAL_SEATS) {
        for (seats, recency) in [(plan.canon, false), (plan.recency, true)] {
            if seats <= 0 {
                continue;
            }
            // Deficit-aware: seats minus what previous runs already mined here.
            let have: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pavlov_cues WHERE meta_category = $1",
            )
            .bind(&plan.category)
            .fetch_one(&state.pool)
            .await?;
            let want = (plan.canon + plan.recency - have).min(seats);
            if want <= 0 {
                continue;
            }
            let candidates = select_candidates(state, &plan.category, want, recency).await?;
            for c in candidates {
                let terms = mine_terms(state, &c.norm, &c.display, total_docs as f64).await?;
                if terms.is_empty() {
                    continue;
                }
                let examples = example_ids(state, &c.norm).await?;
                sqlx::query(
                    "INSERT INTO pavlov_cues
                       (answer, answer_norm, meta_category, mined_terms, example_clue_ids, answer_freq)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (answer_norm) DO NOTHING",
                )
                .bind(&c.display)
                .bind(&c.norm)
                .bind(&plan.category)
                .bind(&terms)
                .bind(&examples)
                .bind(c.freq)
                .execute(&state.pool)
                .await?;
            }
        }
    }
    Ok(())
}

/// Stage B: polish pending rows in batches; each batch is upserted before the
/// next call, so an interrupted run resumes where it left off.
async fn polish_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    loop {
        let batch: Vec<(i32, String, Vec<String>, Vec<i32>)> = sqlx::query_as(
            "SELECT id, answer, mined_terms, example_clue_ids
             FROM pavlov_cues WHERE status = 'pending' ORDER BY id LIMIT $1",
        )
        .bind(POLISH_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }

        let mut inputs = Vec::with_capacity(batch.len());
        for (_, answer, terms, ex_ids) in &batch {
            let clues: Vec<(String,)> = sqlx::query_as(
                "SELECT coalesce(answer, '') FROM jeopardy_questions WHERE id = ANY($1) LIMIT 2",
            )
            .bind(&ex_ids[..])
            .fetch_all(&state.pool)
            .await?;
            inputs.push(PolishInput {
                answer: answer.clone(),
                terms: terms.clone(),
                sample_clues: clues.into_iter().map(|(c,)| c).collect(),
            });
        }

        let (system, user) = polish_prompts(&inputs);
        let response = chat_json_with_model(state, &system, &user).await?;
        let outcomes = parse_polish_response(&response);

        // Match outcomes back to batch rows by lowercased answer.
        let mut updated = 0;
        for out in &outcomes {
            let key = out.answer.to_lowercase();
            let Some((id, ..)) = batch
                .iter()
                .find(|(_, a, ..)| a.to_lowercase() == key)
            else {
                continue;
            };
            let status = if out.keep { "active" } else { "dropped" };
            sqlx::query(
                "UPDATE pavlov_cues SET status = $2, cue_phrases = $3, model = $4
                 WHERE id = $1 AND status = 'pending'",
            )
            .bind(id)
            .bind(status)
            .bind(&out.phrases)
            .bind(POLISH_MODEL)
            .execute(&state.pool)
            .await?;
            updated += 1;
        }
        if updated == 0 {
            // LLM echoed nothing usable for this batch — drop it rather than
            // spin forever refetching the same pending rows.
            let ids: Vec<i32> = batch.iter().map(|(id, ..)| *id).collect();
            sqlx::query(
                "UPDATE pavlov_cues SET status = 'dropped', model = $2
                 WHERE id = ANY($1) AND status = 'pending'",
            )
            .bind(&ids)
            .bind(POLISH_MODEL)
            .execute(&state.pool)
            .await?;
            tracing::warn!("pavlov polish: batch of {} unmatched, dropped", ids.len());
        }
    }
}

async fn chat_json_with_model(
    state: &Arc<AppState>,
    system: &str,
    user: &str,
) -> Result<serde_json::Value, AppError> {
    crate::openai::chat_json(&state.config.openai_api_key, POLISH_MODEL, system, user, 0.3).await
}

/// Full generation run: mine then polish. Both stages are idempotent/resumable.
pub async fn run_generation(state: &Arc<AppState>) -> Result<(), AppError> {
    mine_stage(state).await?;
    polish_stage(state).await
}
```

- [ ] **Step 2: Compile and run existing tests**

Run: `cd backend && cargo test pavlov::`
Expected: compiles clean; 10 tests still pass. (Fix any visibility issue by making `blend`/`routes::mock_test::apportion` `pub` — `apportion` already is.)

- [ ] **Step 3: Commit**

```bash
git add backend/src/pavlov.rs
git commit -m "feat(pavlov): resumable generation job — TF-IDF mining + LLM polish"
```

---

### Task 5: Admin routes + AppState flag + registration

**Files:**
- Create: `backend/src/routes/pavlov.rs`
- Modify: `backend/src/routes/mod.rs` (add `pub mod pavlov;`)
- Modify: `backend/src/main.rs` (AppState field, AppState construction, route registrations)

**Interfaces:**
- Consumes: `crate::pavlov::run_generation`, `AppState.pavlov_inflight`.
- Produces: `POST /api/admin/pavlov/generate`, `GET /api/admin/pavlov/status`; `AppState.pavlov_inflight: std::sync::atomic::AtomicBool`. Later tasks add more handlers to this same file.

- [ ] **Step 1: Add the AppState field**

In `backend/src/main.rs` add to `struct AppState` (after `blindspot_inflight`):

```rust
    pub pavlov_inflight: std::sync::atomic::AtomicBool,
```

and in the place where `AppState` is constructed (search for `blindspot_inflight:` in `main.rs`), add alongside it:

```rust
        pavlov_inflight: std::sync::atomic::AtomicBool::new(false),
```

- [ ] **Step 2: Write the routes module**

Create `backend/src/routes/pavlov.rs`:

```rust
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    if state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest("OPENAI_API_KEY not configured".into()));
    }
    if state
        .pavlov_inflight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(Json(json!({ "started": false, "running": true })));
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::pavlov::run_generation(&st).await {
            tracing::error!("pavlov generation failed (resumable — rerun to continue): {e:?}");
        }
        st.pavlov_inflight.store(false, Ordering::SeqCst);
    });
    Ok(Json(json!({ "started": true })))
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, count(*) FROM pavlov_cues GROUP BY status")
            .fetch_all(&state.pool)
            .await?;
    let get = |k: &str| counts.iter().find(|(s, _)| s == k).map(|(_, n)| *n).unwrap_or(0);
    Ok(Json(json!({
        "running": state.pavlov_inflight.load(Ordering::SeqCst),
        "pending": get("pending"),
        "active": get("active"),
        "dropped": get("dropped"),
    })))
}
```

- [ ] **Step 3: Register module and routes**

In `backend/src/routes/mod.rs` add (alphabetical position):

```rust
pub mod pavlov;
```

In `backend/src/main.rs`, in the `api_routes` chain after the `/api/admin/approve` line:

```rust
        .route("/api/admin/pavlov/generate", post(routes::pavlov::generate))
        .route("/api/admin/pavlov/status", get(routes::pavlov::status))
```

- [ ] **Step 4: Compile + tests**

Run: `cd backend && cargo test`
Expected: full suite compiles and passes.

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/pavlov.rs backend/src/routes/mod.rs backend/src/main.rs
git commit -m "feat(pavlov): admin generate/status routes with inflight guard"
```

---

### Task 6: Cue listing + suspend + drill routes

**Files:**
- Modify: `backend/src/routes/pavlov.rs`
- Modify: `backend/src/main.rs` (route registrations)

**Interfaces:**
- Consumes: `crate::srs::{schedule, CardKind, Prev, Rating}`, `crate::answer_match::is_correct`, `crate::routes::practice::{day_start_utc, serve_new}`, `crate::blend::TARGET_WEIGHTS`.
- Produces:
  - `GET /api/pavlov/cues` → `{ "cues": [{id, answer, category, cuePhrases, answerFreq, suspended}] }` sorted by test-weight order then freq desc
  - `POST /api/pavlov/cues/{id}/suspend` body `{"suspended": bool}`
  - `GET /api/pavlov/drill/next` → `{done, isNew?, card?: {cueId, cuePhrases, category}, dueCount, newRemaining, nextDueAt?, dueSoonCount?}`
  - `POST /api/pavlov/drill/check` body `{"cueId", "typed"}` → `{correct, answer, examples: [{clue, category, airDate}]}` (no state change)
  - `POST /api/pavlov/drill/grade` body `{"cueId", "rating"}` → `{state, due, intervalDays, requeueInSession}`

- [ ] **Step 1: Append the handlers**

Append to `backend/src/routes/pavlov.rs`:

```rust
use axum::extract::Path;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::answer_match;
use crate::blend::TARGET_WEIGHTS;
use crate::routes::practice::{day_start_utc, serve_new};
use crate::srs::{schedule, CardKind, Prev, Rating};

const LEECH_LAPSES: i32 = 8; // same threshold as practice.rs

fn category_rank(cat: &str) -> usize {
    TARGET_WEIGHTS
        .iter()
        .position(|(c, _)| *c == cat)
        .unwrap_or(TARGET_WEIGHTS.len())
}

#[derive(sqlx::FromRow)]
struct CueListRow {
    id: i32,
    answer: String,
    meta_category: String,
    cue_phrases: Vec<String>,
    answer_freq: i32,
    suspended: bool,
}

pub async fn cues(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let mut rows: Vec<CueListRow> = sqlx::query_as(
        "SELECT pc.id, pc.answer, pc.meta_category, pc.cue_phrases, pc.answer_freq,
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
            .then(b.answer_freq.cmp(&a.answer_freq))
    });
    let cues: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id, "answer": r.answer, "category": r.meta_category,
                "cuePhrases": r.cue_phrases, "answerFreq": r.answer_freq,
                "suspended": r.suspended,
            })
        })
        .collect();
    Ok(Json(json!({ "cues": cues })))
}

#[derive(Deserialize)]
pub struct SuspendBody {
    pub suspended: bool,
}

pub async fn suspend(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(cue_id): Path<i32>,
    Json(body): Json<SuspendBody>,
) -> Result<Json<Value>, AppError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pavlov_cues WHERE id = $1)")
            .bind(cue_id)
            .fetch_one(&state.pool)
            .await?;
    if !exists {
        return Err(AppError::NotFound("No such cue".into()));
    }
    sqlx::query(
        "INSERT INTO pavlov_cards (user_id, cue_id, suspended) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, cue_id) DO UPDATE SET suspended = EXCLUDED.suspended",
    )
    .bind(auth.user_id)
    .bind(cue_id)
    .bind(body.suspended)
    .execute(&state.pool)
    .await?;
    Ok(Json(json!({ "suspended": body.suspended })))
}

#[derive(sqlx::FromRow)]
struct DrillCueRow {
    id: i32,
    cue_phrases: Vec<String>,
    meta_category: String,
}

fn drill_card_json(r: DrillCueRow) -> Value {
    json!({ "cueId": r.id, "cuePhrases": r.cue_phrases, "category": r.meta_category })
}

pub async fn drill_next(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let (new_per_day, tz): (i32, Option<String>) =
        sqlx::query_as("SELECT new_cards_per_day, timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         JOIN pavlov_cues pc ON pc.id = ca.cue_id AND pc.status = 'active'
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards WHERE user_id = $1 AND created_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    let want_new = {
        use rand::Rng;
        serve_new(new_remaining, due_count, rand::rng().random())
    };

    // New cue: unseen active cue, introduced canon-first via the exponential race.
    let pick_new = "SELECT id, cue_phrases, meta_category FROM pavlov_cues
         WHERE status = 'active'
           AND id NOT IN (SELECT cue_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY -ln(random()) / ln(1 + answer_freq) LIMIT 1";
    let fetch_due = "SELECT pc.id, pc.cue_phrases, pc.meta_category
         FROM pavlov_cards ca
         JOIN pavlov_cues pc ON pc.id = ca.cue_id AND pc.status = 'active'
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()
         ORDER BY ca.due ASC LIMIT 1";

    if want_new {
        if let Some(row) = sqlx::query_as::<_, DrillCueRow>(pick_new)
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
        {
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": drill_card_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }
    if let Some(row) = sqlx::query_as::<_, DrillCueRow>(fetch_due)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
    {
        return Ok(Json(json!({
            "done": false, "isNew": false, "card": drill_card_json(row),
            "dueCount": due_count, "newRemaining": new_remaining,
        })));
    }
    if new_remaining > 0 {
        if let Some(row) = sqlx::query_as::<_, DrillCueRow>(pick_new)
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
        {
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": drill_card_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }

    let next_due_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT min(due) FROM pavlov_cards WHERE user_id = $1 AND suspended = false AND due > now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let due_soon_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards
         WHERE user_id = $1 AND suspended = false
           AND due > now() AND due <= now() + interval '60 minutes'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({
        "done": true, "dueCount": due_count, "newRemaining": new_remaining,
        "nextDueAt": next_due_at, "dueSoonCount": due_soon_count,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckBody {
    pub cue_id: i32,
    pub typed: String,
}

/// Grade the typed answer and reveal — no SRS state change (that's `grade`).
pub async fn drill_check(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<CheckBody>,
) -> Result<Json<Value>, AppError> {
    let row: Option<(String, Vec<i32>)> = sqlx::query_as(
        "SELECT answer, example_clue_ids FROM pavlov_cues WHERE id = $1 AND status = 'active'",
    )
    .bind(body.cue_id)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, example_ids) = row.ok_or_else(|| AppError::NotFound("No such cue".into()))?;
    let correct = answer_match::is_correct(&body.typed, &answer);

    let examples: Vec<(String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT coalesce(answer, ''), category, air_date FROM jeopardy_questions
         WHERE id = ANY($1) ORDER BY air_date DESC",
    )
    .bind(&example_ids[..])
    .fetch_all(&state.pool)
    .await?;
    let examples: Vec<Value> = examples
        .into_iter()
        .map(|(clue, category, air_date)| {
            json!({ "clue": clue, "category": category, "airDate": air_date })
        })
        .collect();
    Ok(Json(json!({ "correct": correct, "answer": answer, "examples": examples })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillGradeBody {
    pub cue_id: i32,
    pub rating: String,
}

#[derive(sqlx::FromRow)]
struct PavlovCardRow {
    state: String,
    interval_days: f64,
    ease: f64,
    reps: i32,
    lapses: i32,
    step_index: i16,
}

/// SM-2 schedule for a cue card. Deliberately does NOT touch question_attempts
/// or quiz_sessions — cue reps are not clue attempts (spec §3).
pub async fn drill_grade(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<DrillGradeBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let rating = Rating::from_wire(&body.rating)
        .ok_or_else(|| AppError::BadRequest("rating must be wrong|got_it|too_easy".into()))?;

    let existing: Option<PavlovCardRow> = sqlx::query_as(
        "SELECT state, interval_days, ease, reps, lapses, step_index
         FROM pavlov_cards WHERE user_id = $1 AND cue_id = $2",
    )
    .bind(user_id)
    .bind(body.cue_id)
    .fetch_optional(&state.pool)
    .await?;
    let prev = existing.map(|r| Prev {
        state: CardKind::from_str(&r.state),
        interval_days: r.interval_days,
        ease: r.ease,
        reps: r.reps,
        lapses: r.lapses,
        step_index: r.step_index,
    });

    let out = schedule(prev, rating);
    let now: DateTime<Utc> = Utc::now();
    let due = now + Duration::seconds(out.interval_secs);
    let suspended = out.lapses >= LEECH_LAPSES;

    sqlx::query(
        "INSERT INTO pavlov_cards
           (user_id, cue_id, state, interval_days, ease, due, last_review, reps, lapses, step_index, suspended)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (user_id, cue_id) DO UPDATE SET
           state = EXCLUDED.state,
           interval_days = EXCLUDED.interval_days,
           ease = EXCLUDED.ease,
           due = EXCLUDED.due,
           last_review = EXCLUDED.last_review,
           reps = EXCLUDED.reps,
           lapses = EXCLUDED.lapses,
           step_index = EXCLUDED.step_index,
           suspended = EXCLUDED.suspended",
    )
    .bind(user_id)
    .bind(body.cue_id)
    .bind(out.state.as_str())
    .bind(out.interval_days)
    .bind(out.ease)
    .bind(due)
    .bind(now)
    .bind(out.reps)
    .bind(out.lapses)
    .bind(out.step_index)
    .bind(suspended)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({
        "state": out.state.as_str(),
        "due": due,
        "intervalDays": out.interval_days,
        "requeueInSession": out.requeue_in_session,
    })))
}
```

- [ ] **Step 2: Register the routes**

In `backend/src/main.rs`, after the admin pavlov routes from Task 5:

```rust
        .route("/api/pavlov/cues", get(routes::pavlov::cues))
        .route("/api/pavlov/cues/{id}/suspend", post(routes::pavlov::suspend))
        .route("/api/pavlov/drill/next", get(routes::pavlov::drill_next))
        .route("/api/pavlov/drill/check", post(routes::pavlov::drill_check))
        .route("/api/pavlov/drill/grade", post(routes::pavlov::drill_grade))
```

- [ ] **Step 3: Compile + full test suite**

Run: `cd backend && cargo test`
Expected: compiles, all tests pass. (`day_start_utc` and `serve_new` are already `pub fn` in `routes::practice`.)

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/pavlov.rs backend/src/main.rs
git commit -m "feat(pavlov): cue listing, suspend, and SRS drill routes"
```

---

### Task 7: SQL sanity script

**Files:**
- Create: `scripts/verify-pavlov.sql`

**Interfaces:**
- Consumes: populated `pavlov_cues` (run AFTER a generation). Checks marked `-- expect 0` are failures when nonzero.

- [ ] **Step 1: Write the script**

```sql
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
```

- [ ] **Step 2: Syntax-check it against the DB (empty tables are fine)**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -v ON_ERROR_STOP=1 -f - < scripts/verify-pavlov.sql`
Expected: exits 0; checks B–F all return 0 rows-failed on an empty table.

- [ ] **Step 3: Commit**

```bash
git add scripts/verify-pavlov.sql
git commit -m "test(pavlov): SQL sanity checks for cue generation"
```

---

### Task 8: Frontend — nav link + drill page

**Files:**
- Modify: `frontend/src/lib/components/Nav.svelte` (add link)
- Create: `frontend/src/routes/pavlov/+page.svelte`

**Interfaces:**
- Consumes: `GET /api/pavlov/drill/next`, `POST /api/pavlov/drill/check`, `POST /api/pavlov/drill/grade` (Task 6 shapes), `$lib/api`, `$lib/auth.svelte`.

- [ ] **Step 1: Add the nav link**

In `frontend/src/lib/components/Nav.svelte`, in the links array after `{ href: '/drill', label: 'Drill' },` add:

```javascript
    { href: '/pavlov', label: 'Pavlov' },
```

- [ ] **Step 2: Create the drill page**

Create `frontend/src/routes/pavlov/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let card = $state<{ cueId: number; cuePhrases: string[]; category: string } | null>(null);
  let isNew = $state(false);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);
  let nextDueAt = $state<string | null>(null);
  let dueSoonCount = $state(0);
  let typed = $state('');
  let result = $state<{ correct: boolean; answer: string; examples: any[] } | null>(null);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state('');
  let session = $state({ total: 0, correct: 0 });

  async function fetchNext() {
    loading = true;
    error = '';
    result = null;
    typed = '';
    try {
      const res = await api.get('/api/pavlov/drill/next');
      dueCount = res.dueCount ?? 0;
      newRemaining = res.newRemaining ?? 0;
      if (res.done) {
        done = true;
        card = null;
        nextDueAt = res.nextDueAt ?? null;
        dueSoonCount = res.dueSoonCount ?? 0;
      } else {
        done = false;
        card = res.card;
        isNew = res.isNew;
      }
    } catch (e: any) {
      error = e.message || 'Failed to load';
    } finally {
      loading = false;
    }
  }

  async function check() {
    if (!card || submitting) return;
    submitting = true;
    error = '';
    try {
      result = await api.post('/api/pavlov/drill/check', { cueId: card.cueId, typed });
      session = { total: session.total + 1, correct: session.correct + (result!.correct ? 1 : 0) };
    } catch (e: any) {
      error = e.message || 'Check failed';
    } finally {
      submitting = false;
    }
  }

  async function grade(rating: string) {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post('/api/pavlov/drill/grade', { cueId: card.cueId, rating });
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Grade failed';
    } finally {
      submitting = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && card && !result) check();
  }

  onMount(fetchNext);
</script>

<svelte:head><title>Pavlov Drill</title></svelte:head>

<div class="max-w-2xl mx-auto px-4 py-8">
  <div class="flex items-center justify-between mb-6">
    <h1 class="text-2xl font-bold">Pavlov Drill</h1>
    <div class="text-sm text-gray-400">
      Due: {dueCount} · New left: {newRemaining}
      {#if session.total > 0}
        · Session: {session.correct}/{session.total}
      {/if}
    </div>
  </div>
  <p class="text-sm text-gray-400 mb-6">
    Trigger keywords → answer. Train the reflex, not the clue.
    <a href="/pavlov/list" class="text-jeopardy-gold hover:underline">Browse the list →</a>
  </p>

  {#if error}
    <div class="mb-4 p-3 rounded bg-red-900/40 text-red-300 text-sm">{error}</div>
  {/if}

  {#if loading}
    <p class="text-gray-400">Loading…</p>
  {:else if done}
    <div class="p-6 rounded-lg border border-gray-700 text-center">
      <p class="text-lg font-medium mb-2">Done for now 🎉</p>
      {#if dueSoonCount > 0}
        <p class="text-sm text-gray-400">{dueSoonCount} card{dueSoonCount === 1 ? '' : 's'} due within the hour.</p>
      {:else if nextDueAt}
        <p class="text-sm text-gray-400">Next card due {new Date(nextDueAt).toLocaleString()}.</p>
      {:else}
        <p class="text-sm text-gray-400">No cards due. Generate or unsuspend cues from the list page.</p>
      {/if}
    </div>
  {:else if card}
    <div class="p-6 rounded-lg border border-gray-700">
      <div class="flex items-center gap-2 mb-4">
        <span class="px-2 py-0.5 rounded text-xs font-medium bg-jeopardy-gold/20 text-jeopardy-gold">
          {card.category}
        </span>
        {#if isNew}<span class="px-2 py-0.5 rounded text-xs bg-blue-900/50 text-blue-300">new</span>{/if}
      </div>

      <div class="flex flex-wrap gap-2 mb-6">
        {#each card.cuePhrases as phrase}
          <span class="px-3 py-1.5 rounded-full border border-gray-600 text-lg">{phrase}</span>
        {/each}
      </div>

      {#if !result}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={typed}
          onkeydown={onKeydown}
          autofocus
          placeholder="Who/what is…?"
          class="w-full px-3 py-2 rounded bg-gray-800 border border-gray-600 focus:border-jeopardy-gold outline-none"
        />
        <button
          onclick={check}
          disabled={submitting}
          class="mt-3 px-4 py-2 rounded bg-jeopardy-gold text-black font-medium disabled:opacity-50"
        >
          Check
        </button>
      {:else}
        <div class="mb-4 p-3 rounded {result.correct ? 'bg-green-900/40 text-green-300' : 'bg-red-900/40 text-red-300'}">
          {result.correct ? 'Correct:' : 'Answer:'} <span class="font-semibold">{result.answer}</span>
        </div>
        {#if result.examples.length > 0}
          <div class="mb-4 text-sm text-gray-400 space-y-2">
            {#each result.examples as ex}
              <p>“{ex.clue}” <span class="text-gray-500">({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</span></p>
            {/each}
          </div>
        {/if}
        <div class="flex gap-2">
          {#if result.correct}
            <button onclick={() => grade('got_it')} disabled={submitting}
              class="px-4 py-2 rounded bg-green-700 font-medium disabled:opacity-50">Got it</button>
            <button onclick={() => grade('too_easy')} disabled={submitting}
              class="px-4 py-2 rounded bg-gray-700 font-medium disabled:opacity-50">Too easy</button>
          {:else}
            <button onclick={() => grade('wrong')} disabled={submitting}
              class="px-4 py-2 rounded bg-red-700 font-medium disabled:opacity-50">Continue</button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>
```

- [ ] **Step 3: Build the frontend**

Run: `cd frontend && npm run build`
Expected: builds with no errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/components/Nav.svelte frontend/src/routes/pavlov/+page.svelte
git commit -m "feat(pavlov): drill page and nav link"
```

---

### Task 9: Frontend — cue list page (+ admin generate)

**Files:**
- Create: `frontend/src/routes/pavlov/list/+page.svelte`

**Interfaces:**
- Consumes: `GET /api/pavlov/cues`, `POST /api/pavlov/cues/{id}/suspend`, `POST /api/admin/pavlov/generate`, `GET /api/admin/pavlov/status`.

- [ ] **Step 1: Create the list page**

Create `frontend/src/routes/pavlov/list/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });
  let isAdmin = $derived(auth.user?.role === 'admin');

  type Cue = {
    id: number; answer: string; category: string;
    cuePhrases: string[]; answerFreq: number; suspended: boolean;
  };
  let cues = $state<Cue[]>([]);
  let search = $state('');
  let loading = $state(true);
  let error = $state('');
  let genStatus = $state<{ running: boolean; pending: number; active: number; dropped: number } | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  // Server returns cues pre-sorted by test-weight order then frequency;
  // grouping just walks that order.
  let filtered = $derived(
    cues.filter((c) => {
      const q = search.trim().toLowerCase();
      if (!q) return true;
      return (
        c.answer.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        c.cuePhrases.some((p) => p.toLowerCase().includes(q))
      );
    })
  );
  let grouped = $derived.by(() => {
    const groups: Array<{ category: string; items: Cue[] }> = [];
    for (const c of filtered) {
      const last = groups[groups.length - 1];
      if (last && last.category === c.category) last.items.push(c);
      else groups.push({ category: c.category, items: [c] });
    }
    return groups;
  });

  async function load() {
    loading = true;
    try {
      const res = await api.get('/api/pavlov/cues');
      cues = res.cues ?? [];
    } catch (e: any) {
      error = e.message || 'Failed to load cues';
    } finally {
      loading = false;
    }
  }

  async function toggleSuspend(cue: Cue) {
    const next = !cue.suspended;
    try {
      await api.post(`/api/pavlov/cues/${cue.id}/suspend`, { suspended: next });
      cue.suspended = next;
    } catch (e: any) {
      error = e.message || 'Suspend failed';
    }
  }

  async function refreshStatus() {
    try {
      genStatus = await api.get('/api/admin/pavlov/status');
      if (genStatus && !genStatus.running && pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
        await load();
      }
    } catch {
      /* non-admin or transient; ignore */
    }
  }

  async function generate() {
    error = '';
    try {
      await api.post('/api/admin/pavlov/generate');
      await refreshStatus();
      if (!pollTimer) pollTimer = setInterval(refreshStatus, 5000);
    } catch (e: any) {
      error = e.message || 'Generate failed';
    }
  }

  onMount(async () => {
    await load();
    if (isAdmin) await refreshStatus();
  });
  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });
</script>

<svelte:head><title>Pavlov Cues</title></svelte:head>

<div class="max-w-4xl mx-auto px-4 py-8">
  <div class="flex items-center justify-between mb-2">
    <h1 class="text-2xl font-bold">Pavlov Cues</h1>
    <a href="/pavlov" class="text-jeopardy-gold hover:underline text-sm">Drill →</a>
  </div>
  <p class="text-sm text-gray-400 mb-4">
    Signature keyword → answer associations mined from the clue corpus, weighted to the
    Anytime Test category mix. Suspend rows you don't want in your drill deck.
  </p>

  {#if isAdmin}
    <div class="mb-4 p-3 rounded border border-gray-700 flex items-center gap-4 text-sm">
      <button
        onclick={generate}
        disabled={genStatus?.running}
        class="px-3 py-1.5 rounded bg-jeopardy-gold text-black font-medium disabled:opacity-50"
      >
        {genStatus?.running ? 'Generating…' : 'Generate / resume'}
      </button>
      {#if genStatus}
        <span class="text-gray-400">
          active {genStatus.active} · pending {genStatus.pending} · dropped {genStatus.dropped}
        </span>
      {/if}
    </div>
  {/if}

  {#if error}
    <div class="mb-4 p-3 rounded bg-red-900/40 text-red-300 text-sm">{error}</div>
  {/if}

  <input
    type="text"
    bind:value={search}
    placeholder="Search answers, phrases, categories…"
    class="w-full mb-6 px-3 py-2 rounded bg-gray-800 border border-gray-600 focus:border-jeopardy-gold outline-none"
  />

  {#if loading}
    <p class="text-gray-400">Loading…</p>
  {:else if cues.length === 0}
    <p class="text-gray-400">No cues yet{isAdmin ? ' — run Generate above.' : '.'}</p>
  {:else}
    {#each grouped as group}
      <h2 class="text-lg font-semibold mt-6 mb-2 text-jeopardy-gold">
        {group.category} <span class="text-gray-500 text-sm font-normal">({group.items.length})</span>
      </h2>
      <div class="divide-y divide-gray-800 border border-gray-800 rounded-lg">
        {#each group.items as cue (cue.id)}
          <div class="p-3 flex items-start gap-3 {cue.suspended ? 'opacity-40' : ''}">
            <div class="flex-1 min-w-0">
              <div class="font-medium">{cue.answer}
                <span class="text-xs text-gray-500 ml-1">×{cue.answerFreq}</span>
              </div>
              <div class="flex flex-wrap gap-1.5 mt-1">
                {#each cue.cuePhrases as phrase}
                  <span class="px-2 py-0.5 rounded-full border border-gray-700 text-xs text-gray-300">{phrase}</span>
                {/each}
              </div>
            </div>
            <button
              onclick={() => toggleSuspend(cue)}
              class="text-xs px-2 py-1 rounded border border-gray-600 hover:border-jeopardy-gold shrink-0"
            >
              {cue.suspended ? 'Unsuspend' : 'Suspend'}
            </button>
          </div>
        {/each}
      </div>
    {/each}
  {/if}
</div>
```

- [ ] **Step 2: Build the frontend**

Run: `cd frontend && npm run build`
Expected: builds with no errors. If `auth.user?.role` doesn't exist on the auth store, check `frontend/src/lib/auth.svelte.ts` for the actual field (the Nav admin link uses the same check) and match it.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/routes/pavlov/list/+page.svelte
git commit -m "feat(pavlov): cue browser page with search, suspend, admin generate"
```

---

### Task 10: End-to-end verification

**Files:** none created — verification only.

- [ ] **Step 1: Backend suite**

Run: `cd backend && cargo test`
Expected: all tests pass.

- [ ] **Step 2: Run generation against the dev DB**

Using the dev setup (mock API won't do — generation needs the real backend + `OPENAI_API_KEY` in `backend/.env`): start the backend, log in as an admin user, then:

```bash
curl -s -X POST http://localhost:8080/api/admin/pavlov/generate -H "Cookie: $AUTH_COOKIE"
watch -n 10 'curl -s http://localhost:8080/api/admin/pavlov/status -H "Cookie: $AUTH_COOKIE"'
```

(Adjust port to the backend's actual port; get `$AUTH_COOKIE` from the browser dev tools after login.)
Expected: `pending` drains to 0; `active` lands in the low-to-mid 1000s (some drops are normal); `running` returns to `false`.

- [ ] **Step 3: SQL sanity checks**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -v ON_ERROR_STOP=1 -f - < scripts/verify-pavlov.sql`
Expected: checks B–F report 0 fail_rows; check A's per-category totals track the blend weights (Literature ≈ 300 down to Sports ≈ 30).

- [ ] **Step 4: Manual QA in the browser**

- `/pavlov/list`: cues grouped by category in weight order, search narrows, suspend toggles and dims the row.
- `/pavlov`: card shows cue phrases (not a full clue), typing the right answer marks correct, reveal shows real example clues, Got it/Too easy/Continue advance; counter honors new-cards-per-day.
- Confirm no `question_attempts` rows were written by drilling: `SELECT count(*) FROM question_attempts WHERE answered_at > now() - interval '10 minutes'` before/after a few drill reps (should not change from drilling alone).

- [ ] **Step 5: Commit any fixes, then finish**

Use the superpowers:finishing-a-development-branch flow (this repo works on `main` with manual deploys to Tower — coordinate the deploy step with the user).
