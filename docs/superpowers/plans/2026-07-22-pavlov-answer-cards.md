# Pavlov v2.1 (Per-Answer Cards + Hint Tier) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Revert the drill card unit to one-per-answer showing 2–3 cue phrases, topping up single-cue answers from a display-only hint tier mined at looser thresholds.

**Architecture:** Migration 0011 adds `tier` to `pavlov_cues`, creates the denormalized `pavlov_answers` card table, and re-keys `pavlov_cards` to `answer_id`. The miner gains a hint pass (only for answers with < 3 standard cues, capped at the deficit) and a post-render assembly stage that rebuilds `pavlov_answers`. Routes/pages switch from cue-keyed to answer-keyed. A user card-review gate sits between local generation and any merge/deploy.

**Tech Stack:** Rust (axum, sqlx), Postgres 15+, SvelteKit (Svelte 5 runes, Tailwind), OpenAI JSON mode (`gpt-4o`).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-22-pavlov-answer-cards-design.md`. v2 evidence model retained; only card unit, hint tier, and cue-facing surfaces change.
- Standard thresholds (unchanged, already in code): bigram `support >= 4 AND prec >= 0.5`; unigram `>= 6 AND >= 0.6`. Hint thresholds: bigram `support >= 3 AND prec >= 0.4`; unigram `>= 5 AND >= 0.5`. Hint mining only for answers with fewer than 3 active standard cues, keeping at most `3 − standard_count` best candidates (score = support × prec).
- Existing rendered cues are NOT re-rendered; the render stage only processes new `pending` rows. Hint cues never form cards alone; `pavlov_answers` rows require ≥ 1 active standard cue.
- Card API shape: `{answerId, phrases: [{text, tier}], category}`. Listing endpoint becomes `GET /api/pavlov/answers` with rows `{id, answer, category, phrases: [{text, tier, support, total, precision}], suspended}`; suspend becomes `POST /api/pavlov/answers/{id}/suspend`. Drill endpoints keep their paths but bodies use `answerId`.
- New-card order: `-ln(random()) / ln(1 + score)` where `score = pavlov_answers.score`.
- Drill handlers still never write `question_attempts`/`quiz_sessions`; honesty mode (`typed` optional), SM-2, allowance rules (incl. `last_review IS NOT NULL` new-count predicate) unchanged.
- USER GATE (Task 6): after local generation, sample cards are shown to the user in-session; NO merge, push, or deploy before approval.
- Migrations: `scripts/apply-migration.sh backend/migrations/0011_pavlov_answer_cards.sql`. Tests: `cd backend && cargo test`. Frontend: `cd frontend && npm run build` then `git checkout -- build` (never commit build artifacts).
- Commit per task with the repo's Claude Code trailer.

---

### Task 1: Migration 0011

**Files:**
- Create: `backend/migrations/0011_pavlov_answer_cards.sql`

**Interfaces:**
- Produces: `pavlov_cues.tier`; `pavlov_answers`; `pavlov_cards` keyed by `answer_id`.

- [ ] **Step 1: Write the migration**

```sql
-- 0011: Pavlov v2.1 — per-answer cards + hint tier
-- (docs/superpowers/specs/2026-07-22-pavlov-answer-cards-design.md).
-- Idempotent. Destructive only to pavlov_cards rows (drill-state reset, approved).

ALTER TABLE pavlov_cues
  ADD COLUMN IF NOT EXISTS tier TEXT NOT NULL DEFAULT 'standard'
    CHECK (tier IN ('standard', 'hint'));

-- The card table: one row per answer, phrases denormalized at generation.
CREATE TABLE IF NOT EXISTS pavlov_answers (
  id               SERIAL PRIMARY KEY,
  answer_norm      TEXT NOT NULL UNIQUE,
  answer           TEXT NOT NULL,
  meta_category    TEXT NOT NULL,
  phrases          TEXT[] NOT NULL DEFAULT '{}',   -- display forms, standard-first
  phrase_tiers     TEXT[] NOT NULL DEFAULT '{}',   -- parallel: 'standard' | 'hint'
  score            REAL NOT NULL DEFAULT 0,        -- max support*prec over standard cues
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_pavlov_answers_category ON pavlov_answers (meta_category);

-- Re-key drill state to answers. Guarded: only fires while the old cue_id
-- shape exists; re-runs are no-ops.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
             WHERE table_name = 'pavlov_cards' AND column_name = 'cue_id') THEN
    DROP TABLE pavlov_cards;
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS pavlov_cards (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  answer_id     INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
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
  UNIQUE (user_id, answer_id)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_due     ON pavlov_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_pavlov_cards_user_created ON pavlov_cards (user_id, created_at);
```

- [ ] **Step 2: Apply**

Run: `scripts/apply-migration.sh backend/migrations/0011_pavlov_answer_cards.sql`
Expected: exits 0 (fast — no corpus scans).

- [ ] **Step 3: Verify**

Run: `docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -c "\d pavlov_answers" -c "SELECT tier, count(*) FROM pavlov_cues GROUP BY 1" -c "SELECT count(*) FROM pavlov_cards"`
Expected: `pavlov_answers` exists with `phrases`/`phrase_tiers`/`score`; all existing cues `standard`; `pavlov_cards` empty with `answer_id`.

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0011_pavlov_answer_cards.sql
git commit -m "feat(pavlov): migration 0011 — answer cards, hint tier, card re-key"
```

---

### Task 2: Pure logic — hint top-up + phrase assembly (TDD)

**Files:**
- Modify: `backend/src/pavlov.rs`

**Interfaces:**
- Consumes: `CueCandidate`, `prune_redundant`, `phrase_leaks_answer` (existing).
- Produces:
  - `pub const HINT_BIGRAM_MIN_SUPPORT: i64 = 3;` `pub const HINT_BIGRAM_MIN_PREC: f64 = 0.4;` `pub const HINT_UNIGRAM_MIN_SUPPORT: i64 = 5;` `pub const HINT_UNIGRAM_MIN_PREC: f64 = 0.5;`
  - `pub fn prune_hints(standard: &[CueCandidate], hints: Vec<CueCandidate>) -> Vec<CueCandidate>` — drops any hint token-related (subset either direction) to a standard cue of the same answer, then prunes hints among themselves via `prune_redundant`; standard cues are never dropped.
  - `pub struct PhraseRow { pub display: String, pub tier: String, pub score: f64 }`
  - `pub fn assemble_phrases(mut rows: Vec<PhraseRow>) -> Vec<PhraseRow>` — orders standard-first then score desc, caps at 3.

- [ ] **Step 1: Write the failing tests**

Append inside the `tests` module of `backend/src/pavlov.rs`:

```rust
    #[test]
    fn prune_hints_drops_hint_related_to_standard_but_never_standard() {
        let standard = vec![cand("dylan thomas", "milk wood", 2, 6, 7)];
        let hints = vec![
            cand("dylan thomas", "wood", 1, 7, 12),      // subset of standard -> dropped
            cand("dylan thomas", "swansea", 1, 5, 9),    // unrelated -> kept
        ];
        let out = prune_hints(&standard, hints);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].gram, "swansea");
    }

    #[test]
    fn prune_hints_prunes_hints_among_themselves() {
        let out = prune_hints(
            &[],
            vec![
                cand("solomon", "temple", 1, 5, 10),          // score 2.5
                cand("solomon", "first temple", 2, 4, 5),     // score 3.2, superset
            ],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].gram, "first temple");
    }

    #[test]
    fn prune_hints_ignores_standard_of_other_answers() {
        let standard = vec![cand("robert frost", "poet laureate", 2, 5, 6)];
        let hints = vec![cand("dylan thomas", "poet", 1, 5, 9)];
        let out = prune_hints(&standard, hints);
        assert_eq!(out.len(), 1);
    }

    fn prow(display: &str, tier: &str, score: f64) -> PhraseRow {
        PhraseRow { display: display.to_string(), tier: tier.to_string(), score }
    }

    #[test]
    fn assemble_orders_standard_first_then_score_and_caps_at_three() {
        let out = assemble_phrases(vec![
            prow("hint high", "hint", 9.0),
            prow("std low", "standard", 1.0),
            prow("std high", "standard", 5.0),
            prow("hint low", "hint", 0.5),
        ]);
        let got: Vec<&str> = out.iter().map(|p| p.display.as_str()).collect();
        assert_eq!(got, vec!["std high", "std low", "hint high"]);
    }

    #[test]
    fn assemble_handles_fewer_than_three() {
        let out = assemble_phrases(vec![prow("only", "standard", 2.0)]);
        assert_eq!(out.len(), 1);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd backend && cargo test pavlov::`
Expected: compile errors (missing items).

- [ ] **Step 3: Implement**

Add above the tests module (near the other pub constants/functions):

```rust
pub const HINT_BIGRAM_MIN_SUPPORT: i64 = 3;
pub const HINT_BIGRAM_MIN_PREC: f64 = 0.4;
pub const HINT_UNIGRAM_MIN_SUPPORT: i64 = 5;
pub const HINT_UNIGRAM_MIN_PREC: f64 = 0.5;

/// Prune hint candidates: any hint token-related (subset either direction) to
/// a standard cue of the same answer is dropped — standard cues are immovable
/// — then survivors are pruned among themselves.
pub fn prune_hints(standard: &[CueCandidate], hints: Vec<CueCandidate>) -> Vec<CueCandidate> {
    use std::collections::HashSet;
    let std_toks: Vec<(&str, HashSet<&str>)> = standard
        .iter()
        .map(|c| (c.answer_norm.as_str(), c.gram.split(' ').collect()))
        .collect();
    let survivors: Vec<CueCandidate> = hints
        .into_iter()
        .filter(|h| {
            let ht: HashSet<&str> = h.gram.split(' ').collect();
            !std_toks.iter().any(|(ans, st)| {
                *ans == h.answer_norm && (ht.is_subset(st) || st.is_subset(&ht))
            })
        })
        .collect();
    prune_redundant(survivors)
}

#[derive(Debug, Clone)]
pub struct PhraseRow {
    pub display: String,
    pub tier: String,
    pub score: f64,
}

/// Standard phrases before hints, score desc within tier, capped at 3.
pub fn assemble_phrases(mut rows: Vec<PhraseRow>) -> Vec<PhraseRow> {
    rows.sort_by(|a, b| {
        let ta = (a.tier != "standard") as u8;
        let tb = (b.tier != "standard") as u8;
        ta.cmp(&tb)
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    rows.truncate(3);
    rows
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && cargo test pavlov::`
Expected: 14 passed (9 existing + 5 new).

- [ ] **Step 5: Commit**

```bash
git add backend/src/pavlov.rs
git commit -m "feat(pavlov): hint pruning and phrase assembly (pure, TDD)"
```

---

### Task 3: DB stages — hint mine pass + card assembly

**Files:**
- Modify: `backend/src/pavlov.rs`

**Interfaces:**
- Consumes: Task 2 items; existing `candidate_rows` (rename-free), `mine_stage`, `render_stage`.
- Produces: `run_generation` = mine → hint_mine → render → assemble. Same public signature.

- [ ] **Step 1: Add the hint mining stage**

Insert after `mine_stage` in `backend/src/pavlov.rs`:

```rust
#[derive(sqlx::FromRow)]
struct StandardCueRow {
    answer_norm: String,
    gram: String,
    n: i16,
    support: i32,
    total: i32,
    prec: f32,
}

/// Stage A2: hint-tier top-up. Only answers with < 3 standard cues; keeps at
/// most (3 - standard_count) best hint candidates per answer after leak
/// filtering and pruning against the answer's standard cues.
async fn hint_mine_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    // Deficit per answer over non-dropped standard cues (pending count too:
    // renders may still be in flight on a resumed run).
    let deficits: Vec<(String, i64)> = sqlx::query_as(
        "SELECT answer_norm, 3 - count(*) AS deficit
         FROM pavlov_cues WHERE tier = 'standard' AND status <> 'dropped'
         GROUP BY 1 HAVING count(*) < 3",
    )
    .fetch_all(&state.pool)
    .await?;
    if deficits.is_empty() {
        return Ok(());
    }
    let deficit_map: std::collections::HashMap<String, i64> = deficits.into_iter().collect();
    let norms: Vec<String> = deficit_map.keys().cloned().collect();

    // Hint-band candidates for exactly those answers (between hint and
    // standard thresholds; standard-qualifying grams are already in the table
    // and excluded by the NOT EXISTS).
    let sql = "WITH sup AS (
           SELECT g.answer_norm, g.gram, g.n, count(DISTINCT g.clue_id) AS support
           FROM pavlov_clue_ngrams g
           WHERE g.answer_norm = ANY($1)
           GROUP BY 1, 2, 3
           HAVING count(DISTINCT g.clue_id) >= $2
         ), tot AS (
           SELECT g.gram, count(DISTINCT g.clue_id) AS total
           FROM pavlov_clue_ngrams g
           WHERE g.gram IN (SELECT DISTINCT gram FROM sup)
           GROUP BY 1
         )
         SELECT s.answer_norm, s.gram, s.n, s.support::int4 AS support,
                t.total::int4 AS total, (s.support::float8 / t.total)::float4 AS prec
         FROM sup s JOIN tot t USING (gram)
         WHERE ((s.n = 2 AND s.support >= $3 AND s.support::float8 / t.total >= $4)
             OR (s.n = 1 AND s.support >= $5 AND s.support::float8 / t.total >= $6))
           AND NOT EXISTS (SELECT 1 FROM pavlov_cues pc
                           WHERE pc.answer_norm = s.answer_norm AND pc.cue_stem = s.gram)";
    let rows: Vec<StandardCueRow> = sqlx::query_as(sql)
        .bind(&norms)
        .bind(HINT_BIGRAM_MIN_SUPPORT.min(HINT_UNIGRAM_MIN_SUPPORT))
        .bind(HINT_BIGRAM_MIN_SUPPORT)
        .bind(HINT_BIGRAM_MIN_PREC)
        .bind(HINT_UNIGRAM_MIN_SUPPORT)
        .bind(HINT_UNIGRAM_MIN_PREC)
        .fetch_all(&state.pool)
        .await?;

    // Existing standard cues of those answers, for pruning.
    let standard_rows: Vec<StandardCueRow> = sqlx::query_as(
        "SELECT answer_norm, cue_stem AS gram, 0::int2 AS n, support, total, prec
         FROM pavlov_cues
         WHERE tier = 'standard' AND status <> 'dropped' AND answer_norm = ANY($1)",
    )
    .bind(&norms)
    .fetch_all(&state.pool)
    .await?;
    let to_cand = |r: StandardCueRow| CueCandidate {
        answer_norm: r.answer_norm,
        gram: r.gram,
        n: r.n,
        support: r.support as i64,
        total: r.total as i64,
        prec: r.prec as f64,
    };
    let standard: Vec<CueCandidate> = standard_rows.into_iter().map(to_cand).collect();
    let hints: Vec<CueCandidate> = rows
        .into_iter()
        .map(to_cand)
        .filter(|c| !phrase_leaks_answer(&c.answer_norm, &c.gram))
        .collect();
    let mut kept = prune_hints(&standard, hints);

    // Per-answer cap at the deficit, best score first.
    kept.sort_by(|a, b| {
        a.answer_norm.cmp(&b.answer_norm).then(
            (b.support as f64 * b.prec)
                .partial_cmp(&(a.support as f64 * a.prec))
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let mut taken: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut inserted = 0usize;
    for c in kept {
        let cap = *deficit_map.get(&c.answer_norm).unwrap_or(&0);
        let t = taken.entry(c.answer_norm.clone()).or_insert(0);
        if *t >= cap {
            continue;
        }
        *t += 1;
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
        sqlx::query(
            "INSERT INTO pavlov_cues
               (answer, answer_norm, meta_category, cue_stem, support, total, prec,
                example_clue_ids, tier)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'hint')
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
        inserted += 1;
    }
    tracing::info!("pavlov hint mine: {} hint cues inserted", inserted);
    Ok(())
}
```

- [ ] **Step 2: Add the assembly stage and wire run_generation**

Insert after `render_stage`:

```rust
/// Stage C: rebuild the denormalized card table from active cues. Derived
/// data — full rebuild is idempotent. Cards require >= 1 active standard cue.
async fn assemble_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    let answers: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT answer_norm FROM pavlov_cues
         WHERE status = 'active' AND tier = 'standard'",
    )
    .fetch_all(&state.pool)
    .await?;

    for (norm,) in &answers {
        let cue_rows: Vec<(String, String, i32, f32, Vec<i32>, String, String)> = sqlx::query_as(
            "SELECT cue_display, tier, support, prec, example_clue_ids, answer, meta_category
             FROM pavlov_cues WHERE status = 'active' AND answer_norm = $1",
        )
        .bind(norm)
        .fetch_all(&state.pool)
        .await?;
        let (answer_display, category) = match cue_rows.first() {
            Some(r) => (r.5.clone(), r.6.clone()),
            None => continue,
        };
        let score = cue_rows
            .iter()
            .filter(|r| r.1 == "standard")
            .map(|r| r.2 as f64 * r.3 as f64)
            .fold(0.0f64, f64::max);
        let chosen = assemble_phrases(
            cue_rows
                .iter()
                .map(|r| PhraseRow {
                    display: r.0.clone(),
                    tier: r.1.clone(),
                    score: r.2 as f64 * r.3 as f64,
                })
                .collect(),
        );
        let phrases: Vec<String> = chosen.iter().map(|p| p.display.clone()).collect();
        let tiers: Vec<String> = chosen.iter().map(|p| p.tier.clone()).collect();
        // Examples: union over the chosen phrases' source cues, newest first.
        let chosen_set: std::collections::HashSet<&str> =
            phrases.iter().map(|s| s.as_str()).collect();
        let mut example_ids: Vec<i32> = cue_rows
            .iter()
            .filter(|r| chosen_set.contains(r.0.as_str()))
            .flat_map(|r| r.4.iter().copied())
            .collect();
        example_ids.dedup();
        let example_ids: Vec<i32> = {
            let ordered: Vec<(i32,)> = sqlx::query_as(
                "SELECT id FROM jeopardy_questions WHERE id = ANY($1)
                 ORDER BY air_date DESC NULLS LAST LIMIT 3",
            )
            .bind(&example_ids)
            .fetch_all(&state.pool)
            .await?;
            ordered.into_iter().map(|(i,)| i).collect()
        };
        sqlx::query(
            "INSERT INTO pavlov_answers
               (answer_norm, answer, meta_category, phrases, phrase_tiers, score, example_clue_ids)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (answer_norm) DO UPDATE SET
               answer = EXCLUDED.answer,
               meta_category = EXCLUDED.meta_category,
               phrases = EXCLUDED.phrases,
               phrase_tiers = EXCLUDED.phrase_tiers,
               score = EXCLUDED.score,
               example_clue_ids = EXCLUDED.example_clue_ids",
        )
        .bind(norm)
        .bind(&answer_display)
        .bind(&category)
        .bind(&phrases)
        .bind(&tiers)
        .bind(score as f32)
        .bind(&example_ids)
        .execute(&state.pool)
        .await?;
    }
    tracing::info!("pavlov assemble: {} answer cards", answers.len());
    Ok(())
}
```

and change `run_generation` to:

```rust
/// Full v2.1 generation: mine standard, top up hints, render new pending
/// cues, then rebuild the answer-card table. All stages idempotent/resumable.
pub async fn run_generation(state: &Arc<AppState>) -> Result<(), AppError> {
    mine_stage(state).await?;
    hint_mine_stage(state).await?;
    render_stage(state).await?;
    assemble_stage(state).await
}
```

- [ ] **Step 3: Compile + full suite**

Run: `cd backend && cargo test`
Expected: all pass (14 pavlov + rest).

- [ ] **Step 4: Commit**

```bash
git add backend/src/pavlov.rs
git commit -m "feat(pavlov): hint mine pass and answer-card assembly stages"
```

---

### Task 4: Routes — answer-keyed cues API

**Files:**
- Modify: `backend/src/routes/pavlov.rs`
- Modify: `backend/src/main.rs` (route registration changes)

**Interfaces:**
- Produces: `GET /api/pavlov/answers` rows `{id, answer, category, phrases: [{text, tier, support, total, precision}], suspended}`; `POST /api/pavlov/answers/{id}/suspend`; drill card `{answerId, phrases: [{text, tier}], category}`; `drill_check`/`drill_grade` bodies use `answerId`. `generate`/`status` untouched.

- [ ] **Step 1: Rewrite the answer-facing handlers**

In `backend/src/routes/pavlov.rs`, replace `CueListRow`, `cues`, `suspend`, `DrillCueRow`, `drill_card_json`, and the drill handlers' cue references with:

```rust
#[derive(sqlx::FromRow)]
struct AnswerListRow {
    id: i32,
    answer: String,
    answer_norm: String,
    meta_category: String,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    score: f32,
    suspended: bool,
}

pub async fn answers(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let mut rows: Vec<AnswerListRow> = sqlx::query_as(
        "SELECT pa.id, pa.answer, pa.answer_norm, pa.meta_category, pa.phrases,
                pa.phrase_tiers, pa.score,
                COALESCE(ca.suspended, false) AS suspended
         FROM pavlov_answers pa
         LEFT JOIN pavlov_cards ca ON ca.answer_id = pa.id AND ca.user_id = $1",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;
    rows.sort_by(|a, b| {
        category_rank(&a.meta_category)
            .cmp(&category_rank(&b.meta_category))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Per-phrase evidence for the listing (one query, mapped client-side).
    let ev: Vec<(String, String, String, i32, i32, f32)> = sqlx::query_as(
        "SELECT answer_norm, cue_display, tier, support, total, prec
         FROM pavlov_cues WHERE status = 'active'",
    )
    .fetch_all(&state.pool)
    .await?;
    use std::collections::HashMap;
    let mut ev_map: HashMap<(String, String), (String, i32, i32, f32)> = HashMap::new();
    for (norm, display, tier, support, total, prec) in ev {
        ev_map.insert((norm, display), (tier, support, total, prec));
    }

    let answers: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let phrases: Vec<Value> = r
                .phrases
                .iter()
                .zip(r.phrase_tiers.iter())
                .map(|(text, tier)| {
                    let key = (r.answer_norm.clone(), text.clone());
                    match ev_map.get(&key) {
                        Some((_, support, total, prec)) => json!({
                            "text": text, "tier": tier,
                            "support": support, "total": total, "precision": prec,
                        }),
                        None => json!({ "text": text, "tier": tier }),
                    }
                })
                .collect();
            json!({
                "id": r.id, "answer": r.answer, "category": r.meta_category,
                "phrases": phrases, "suspended": r.suspended,
            })
        })
        .collect();
    Ok(Json(json!({ "answers": answers })))
}

pub async fn suspend(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(answer_id): Path<i32>,
    Json(body): Json<SuspendBody>,
) -> Result<Json<Value>, AppError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pavlov_answers WHERE id = $1)")
            .bind(answer_id)
            .fetch_one(&state.pool)
            .await?;
    if !exists {
        return Err(AppError::NotFound("No such card".into()));
    }
    sqlx::query(
        "INSERT INTO pavlov_cards (user_id, answer_id, suspended) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, answer_id) DO UPDATE SET suspended = EXCLUDED.suspended",
    )
    .bind(auth.user_id)
    .bind(answer_id)
    .bind(body.suspended)
    .execute(&state.pool)
    .await?;
    Ok(Json(json!({ "suspended": body.suspended })))
}

#[derive(sqlx::FromRow)]
struct DrillAnswerRow {
    id: i32,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    meta_category: String,
}

fn drill_card_json(r: DrillAnswerRow) -> Value {
    let phrases: Vec<Value> = r
        .phrases
        .iter()
        .zip(r.phrase_tiers.iter())
        .map(|(text, tier)| json!({ "text": text, "tier": tier }))
        .collect();
    json!({ "answerId": r.id, "phrases": phrases, "category": r.meta_category })
}
```

In `drill_next`, replace the queries and row type (`DrillCueRow` → `DrillAnswerRow`), and the card-count queries' join targets:

```rust
    let pick_new = "SELECT id, phrases, phrase_tiers, meta_category FROM pavlov_answers
         WHERE id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY -ln(random()) / ln(1 + score) LIMIT 1";
    let fetch_due = "SELECT pa.id, pa.phrases, pa.phrase_tiers, pa.meta_category
         FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()
         ORDER BY ca.due ASC LIMIT 1";
```

and the due-count query:

```rust
    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()",
    )
```

In `drill_check`: body field `answer_id` (wire `answerId`), reveal from `pavlov_answers`:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckBody {
    pub answer_id: i32,
    /// Optional: honesty-mode reveal sends no typed answer.
    pub typed: Option<String>,
}
```

and its query: `"SELECT answer, example_clue_ids FROM pavlov_answers WHERE id = $1"` (404 on miss; rest unchanged).

In `drill_grade`: `DrillGradeBody.answer_id` (wire `answerId`); card select/upsert keyed `answer_id` against `pavlov_cards` (same SM-2/leech/allowance logic — only the key column renames).

- [ ] **Step 2: Update route registration in main.rs**

Replace the two cue routes:

```rust
        .route("/api/pavlov/answers", get(routes::pavlov::answers))
        .route("/api/pavlov/answers/{id}/suspend", post(routes::pavlov::suspend))
```

(drill routes keep their paths).

- [ ] **Step 3: Compile + suite**

Run: `cd backend && cargo test`
Expected: pass.

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/pavlov.rs backend/src/main.rs
git commit -m "feat(pavlov): answer-keyed API — cards, suspend, drill by answerId"
```

---

### Task 5: Frontend — multi-phrase drill card + answer list

**Files:**
- Modify: `frontend/src/routes/pavlov/+page.svelte`
- Modify: `frontend/src/routes/pavlov/list/+page.svelte`

**Interfaces:**
- Consumes: Task 4 API shapes.

- [ ] **Step 1: Drill page**

In `frontend/src/routes/pavlov/+page.svelte`:

```typescript
  let card = $state<{
    answerId: number;
    phrases: Array<{ text: string; tier: string }>;
    category: string;
  } | null>(null);
```

`reveal()` posts `{ answerId: card.answerId }`; `grade()` posts `{ answerId: card.answerId, rating }`. Replace the single-phrase block with chips (hint chips dimmed):

```svelte
        <div class="mb-6 flex flex-wrap gap-2">
          {#each card.phrases as phrase}
            <span class="px-4 py-2 rounded-full border text-xl inline-block
              {phrase.tier === 'hint'
                ? 'border-gray-200 text-gray-500'
                : 'border-gray-300 text-gray-900'}">{phrase.text}</span>
          {/each}
        </div>
```

- [ ] **Step 2: List page**

In `frontend/src/routes/pavlov/list/+page.svelte`:

```typescript
  type Phrase = {
    text: string; tier: string;
    support?: number; total?: number; precision?: number;
  };
  type Card = {
    id: number; answer: string; category: string;
    phrases: Phrase[]; suspended: boolean;
  };
  let cards = $state<Card[]>([]);
```

`load()` reads `/api/pavlov/answers` → `res.answers`; suspend posts to `/api/pavlov/answers/${card.id}/suspend`. Search predicate:

```typescript
      return (
        c.answer.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        c.phrases.some((p) => p.text.toLowerCase().includes(q))
      );
```

Row body:

```svelte
            <div class="flex-1 min-w-0">
              <div class="text-gray-900">
                {#each card.phrases as phrase, i}
                  {#if i > 0}<span class="text-gray-400 mx-1">·</span>{/if}
                  <span class="{phrase.tier === 'hint' ? 'text-gray-500' : 'font-medium'}"
                    >{phrase.text}{#if phrase.support}
                      <span class="text-xs text-gray-400">({phrase.support}/{phrase.total})</span>{/if}</span>
                {/each}
                <span class="text-gray-400 mx-1">→</span>
                <span>{card.answer}</span>
              </div>
            </div>
```

(keep the suspend button, admin panel, grouping walk, and generate/status polling as they are — rename loop variables from `cue` to `card` where they collide).

- [ ] **Step 3: Build both**

Run: `cd backend && cargo test` and `cd frontend && npm run build && git checkout -- build`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/routes/pavlov/+page.svelte frontend/src/routes/pavlov/list/+page.svelte
git commit -m "feat(pavlov): multi-phrase drill card and per-answer list with hint dimming"
```

---

### Task 6: verify-pavlov.sql v2.1 + USER CARD REVIEW GATE (controller)

**Files:**
- Modify: `scripts/verify-pavlov.sql` (append checks H–K)

- [ ] **Step 1: Append card checks**

```sql
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
```

- [ ] **Step 2: Commit**

```bash
git add scripts/verify-pavlov.sql
git commit -m "test(pavlov): v2.1 card checks — shape, standard-cue floor, hint band, leaks"
```

- [ ] **Step 3 (controller): local generation + sanity + CARD REVIEW GATE**

Controller runs: local release backend → temp admin → generate (hint mine + ~4–5k renders + assemble; monitor) → `verify-pavlov.sql` all green → then present to the user IN-SESSION: ~20 random cards plus ~10 cards from previously single-cue answers, formatted `phrases → answer`, hint phrases marked. **STOP. No merge, push, or deploy until the user approves.** If rejected: tune thresholds/logic per feedback, regenerate, re-present.

---

### Task 7: Post-approval — e2e, merge, deploy (controller)

Only after the Task 6 gate passes:

- [ ] Browser QA: drill shows 2–3 chips (hints dimmed), reveal/grade flow, list rows with evidence; no `question_attempts` writes; temp user deleted; servers stopped.
- [ ] Final whole-branch review (most capable model) + fix wave if needed.
- [ ] finishing-a-development-branch: merge to main, push (ebertx account dance), CI monitor, Tower pull/recreate, health + route checks, changelog entry, ledger close-out.
