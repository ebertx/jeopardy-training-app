# Mock Test Mode, Cold/Review Stats, SRS Interleaving — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Anytime-Test simulator (50 unseen clues, 15s timer, typed answers, phonetic grading), break dashboard stats into cold vs. review accuracy, and interleave new SRS cards with due reviews.

**Architecture:** All work targets the live Rust (axum + sqlx) backend in `backend/` and SvelteKit (Svelte 5 runes) frontend in `frontend/`. One hand-written SQL migration adds `question_attempts.attempt_kind` (backfilled) and two mock-test tables. A pure-function answer matcher and quota apportioner get full unit coverage; DB-bound handlers are verified manually against the live schema.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8, rand 0.9, chrono), new crates `strsim`, `rphonetic`, `deunicode`; SvelteKit + Svelte 5 runes, Chart.js via `StatsChart.svelte`, Tailwind.

**Spec:** `docs/superpowers/specs/2026-07-08-mock-test-and-stats-design.md`

## Global Constraints

- **NEVER touch the root Next.js `app/`, `prisma/`, or root `package.json`** — dead pre-rewrite code. Live code is `backend/` + `frontend/` only.
- **Column semantics are inverted (J!Archive convention):** `jeopardy_questions.answer` = the clue text shown to the player; `jeopardy_questions.question` = the accepted response to grade against. Practice UI already renders it this way (`clue_json` passes both; the card shows `answer` as the clue).
- The database is the **live shared Postgres** (DATABASE_URL in `.env`, host 100.92.27.16). Migration 0005 is additive-only and safe under the running binary; do not drop or rewrite existing columns. Never modify credentials.
- Backend handler convention: `async fn(State<Arc<AppState>>, auth: AuthUser, ...) -> Result<Json<Value>, AppError>`, sqlx `$n` binds, `serde_json::json!` responses, camelCase JSON keys.
- rand 0.9 API: `rand::rng().random::<f64>()` (not the 0.8 `thread_rng/gen` names).
- Svelte 5 runes only (`$state`, `$derived`, `$effect`, `$props`); API calls via `$lib/api`'s `api.get/post/put`; auth guard via `getAuth()` + redirect effect (copy the pattern at `frontend/src/routes/practice/+page.svelte:11-15`).
- Mid-band clue filter (used in several tasks, always this exact predicate):
  `((round = 1 AND clue_value BETWEEN 600 AND 1000) OR (round = 2 AND clue_value BETWEEN 800 AND 1200))`
- Mock attempts are `attempt_kind='mock'`: included in adaptive targeting and blindspot analysis (those queries don't filter kind — leave them), **excluded** from all accuracy stats in `stats.rs`.
- Commit after every task with the exact message given; run `cargo test` in `backend/` before every backend commit and `npm run check` in `frontend/` before every frontend commit.

---

### Task 1: Migration 0005 — attempt_kind + mock tables + backfill

**Files:**
- Create: `backend/migrations/0005_mock_test_and_attempt_kind.sql`
- Create: `scripts/apply-migration.sh`

**Interfaces:**
- Produces: `question_attempts.attempt_kind TEXT NOT NULL DEFAULT 'review' CHECK IN ('new','review','mock')`; tables `mock_tests` (with `session_id` linking to `quiz_sessions`) and `mock_test_answers`. Later tasks rely on these exact column names.

- [ ] **Step 1: Write the migration**

```sql
-- 0005: attempt_kind on question_attempts (cold-vs-review tracking) + mock test tables.
-- Idempotent: safe to re-run (also re-run at deploy cutover to reclassify any
-- attempts the old binary inserted with the default between migration and deploy).

ALTER TABLE question_attempts
  ADD COLUMN IF NOT EXISTS attempt_kind TEXT NOT NULL DEFAULT 'review'
  CHECK (attempt_kind IN ('new', 'review', 'mock'));

-- Backfill: earliest non-mock attempt per (user, question) = 'new'; the rest 'review'.
UPDATE question_attempts SET attempt_kind = 'review' WHERE attempt_kind = 'new';
WITH firsts AS (
  SELECT DISTINCT ON (user_id, question_id) id
  FROM question_attempts
  WHERE attempt_kind <> 'mock'
  ORDER BY user_id, question_id, answered_at ASC, id ASC
)
UPDATE question_attempts SET attempt_kind = 'new' WHERE id IN (SELECT id FROM firsts);

CREATE INDEX IF NOT EXISTS idx_qa_user_kind_time
  ON question_attempts (user_id, attempt_kind, answered_at);

CREATE TABLE IF NOT EXISTS mock_tests (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  session_id    INTEGER NOT NULL REFERENCES quiz_sessions(id),
  started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  completed_at  TIMESTAMPTZ,
  question_ids  INTEGER[] NOT NULL,
  current_index INTEGER NOT NULL DEFAULT 0,
  score         INTEGER
);
CREATE INDEX IF NOT EXISTS idx_mock_tests_user ON mock_tests (user_id, completed_at DESC);

CREATE TABLE IF NOT EXISTS mock_test_answers (
  id            SERIAL PRIMARY KEY,
  mock_test_id  INTEGER NOT NULL REFERENCES mock_tests(id) ON DELETE CASCADE,
  question_id   INTEGER NOT NULL REFERENCES jeopardy_questions(id),
  position      INTEGER NOT NULL,
  typed_answer  TEXT NOT NULL DEFAULT '',
  response_ms   INTEGER NOT NULL DEFAULT 0,
  auto_correct  BOOLEAN NOT NULL,
  overridden    BOOLEAN NOT NULL DEFAULT false,
  final_correct BOOLEAN NOT NULL,
  UNIQUE (mock_test_id, position)
);
```

- [ ] **Step 2: Write the apply script** (no local psql; use dockerized psql with the `.env` URL)

```bash
#!/usr/bin/env bash
# Apply a backend SQL migration to the database in .env. Usage: scripts/apply-migration.sh backend/migrations/0005_*.sql
set -euo pipefail
cd "$(dirname "$0")/.."
DB_URL=$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')
docker run --rm -i postgres:16 psql "$DB_URL" -v ON_ERROR_STOP=1 -f - < "$1"
```

- [ ] **Step 3: Apply it**

Run: `chmod +x scripts/apply-migration.sh && scripts/apply-migration.sh backend/migrations/0005_mock_test_and_attempt_kind.sql`
Expected: `ALTER TABLE`, `UPDATE 0`, `UPDATE <n≈1360>`, `CREATE INDEX`, `CREATE TABLE` ×2 — no errors.

- [ ] **Step 4: Verify the backfill against the known baseline**

Run (via the same dockerized psql, heredoc):

```sql
SELECT attempt_kind, count(*) FROM question_attempts WHERE user_id = 1 GROUP BY 1;
SELECT round(100.0*sum(correct::int)/count(*),1) AS cold_pct
FROM question_attempts WHERE user_id = 1 AND attempt_kind = 'new';
```

Expected: `new` count ≈ 1360–1400, `review` the rest, zero `mock`; `cold_pct` ≈ 50.x (matches the 2026-07-08 analysis: all-time first-attempt accuracy ~50%). If cold count equals total attempts, the backfill ordering clause is wrong — stop and fix.

- [ ] **Step 5: Commit**

```bash
git add backend/migrations/0005_mock_test_and_attempt_kind.sql scripts/apply-migration.sh
git commit -m "feat(db): attempt_kind on question_attempts (backfilled) + mock test tables"
```

---

### Task 2: Answer matcher — parsing & normalization

**Files:**
- Modify: `backend/Cargo.toml` (add deps)
- Create: `backend/src/answer_match.rs`
- Modify: `backend/src/main.rs` (add `mod answer_match;` beside the other `mod` lines)

**Interfaces:**
- Produces: `pub fn normalize(s: &str) -> String`, `pub fn accepted_variants(raw: &str) -> Vec<String>` (raw variants, NOT normalized — Task 3 normalizes). Task 3 builds `is_correct` on these.

- [ ] **Step 1: Add dependencies to `backend/Cargo.toml`**

```toml
strsim = "0.11"
rphonetic = "3"
deunicode = "1"
```

Run: `cd backend && cargo build` — expect clean compile (if `rphonetic = "3"` doesn't resolve, use the latest major from `cargo add rphonetic` and adapt Task 3's constructor to its docs).

- [ ] **Step 2: Write failing tests** (bottom of the new `backend/src/answer_match.rs`; stub the two functions above them returning `String::new()` / `vec![]` so it compiles)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_case_punct_diacritics_articles() {
        assert_eq!(normalize("The U.S.S.R."), "u s s r");
        assert_eq!(normalize("Häagen-Dazs"), "haagen dazs");
        assert_eq!(normalize("  a  Möbius strip "), "mobius strip");
        assert_eq!(normalize("\"What A Wonderful World\""), "what a wonderful world");
    }

    #[test]
    fn variants_parenthetical_word_is_optional() {
        let v = accepted_variants("(Thomas) Cromwell");
        assert!(v.contains(&"Thomas Cromwell".to_string()));
        assert!(v.contains(&"Cromwell".to_string()));
    }

    #[test]
    fn variants_or_alternates_split() {
        let v = accepted_variants("the U.S.S.R. (or Soviet Union)");
        assert!(v.contains(&"the U.S.S.R.".to_string()));
        assert!(v.contains(&"Soviet Union".to_string()));
    }

    #[test]
    fn variants_inline_suffix() {
        let v = accepted_variants("rappel(ing)");
        assert!(v.contains(&"rappel".to_string()));
        assert!(v.contains(&"rappeling".to_string()));
    }

    #[test]
    fn variants_strip_escaped_quotes() {
        let v = accepted_variants("\\\"Sweet Dreams\\\"");
        assert!(v.contains(&"Sweet Dreams".to_string()));
    }

    #[test]
    fn variants_plain_answer_passes_through() {
        assert_eq!(accepted_variants("Bellerophon"), vec!["Bellerophon".to_string()]);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd backend && cargo test answer_match`
Expected: FAIL (assertions, since stubs return empty).

- [ ] **Step 4: Implement**

```rust
//! Jeopardy-style answer matching: parse accepted-answer conventions, normalize,
//! and (Task 3) grade typed responses with typo + phonetic forgiveness.

use deunicode::deunicode;

/// Lowercase, ASCII-fold, punctuation→space, collapse whitespace, drop ONE leading article.
pub fn normalize(s: &str) -> String {
    let folded = deunicode(s).to_lowercase();
    let cleaned: String = folded
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() > 1 && matches!(tokens[0], "the" | "a" | "an") {
        tokens.remove(0);
    }
    tokens.join(" ")
}

/// Expand a raw accepted-answer string into acceptable literal variants
/// (J!Archive conventions). Variants are raw text; caller normalizes.
pub fn accepted_variants(raw: &str) -> Vec<String> {
    let cleaned = raw.replace("\\\"", "").replace('"', "");
    let mut bases: Vec<String> = Vec::new();

    // "(or X)" groups are standalone alternates; remaining parens are optional parts.
    let mut remainder = String::new();
    let mut alternates: Vec<String> = Vec::new();
    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('(') {
        let close = match rest[open..].find(')') {
            Some(c) => open + c,
            None => break, // unbalanced — treat rest as literal
        };
        let inner = rest[open + 1..close].trim();
        if let Some(alt) = inner.strip_prefix("or ") {
            alternates.push(alt.trim().to_string());
            remainder.push_str(&rest[..open]);
        } else {
            remainder.push_str(&rest[..close + 1]); // keep for optional expansion
        }
        rest = &rest[close + 1..];
    }
    remainder.push_str(rest);
    bases.push(remainder.trim().to_string());
    bases.extend(alternates);

    // Expand optional parens in each base: with and without. A paren glued to the
    // preceding word ("rappel(ing)") concatenates; a freestanding one is a word.
    let mut out: Vec<String> = Vec::new();
    for base in bases {
        let mut variants = vec![base.clone()];
        // Cap expansion: at most 3 paren groups → 8 variants.
        for _ in 0..3 {
            let mut next: Vec<String> = Vec::new();
            let mut changed = false;
            for v in &variants {
                if let (Some(open), true) = (v.find('('), v.contains(')')) {
                    let close = v[open..].find(')').unwrap() + open;
                    let inner = &v[open + 1..close];
                    let before = &v[..open];
                    let after = &v[close + 1..];
                    let glued = before.ends_with(|c: char| c.is_alphanumeric());
                    // with the parenthetical content
                    let with = if glued {
                        format!("{}{}{}", before, inner, after)
                    } else {
                        format!("{} {} {}", before.trim_end(), inner, after.trim_start())
                    };
                    // without it
                    let without = format!("{} {}", before.trim_end(), after.trim_start());
                    next.push(with.trim().to_string());
                    next.push(without.trim().to_string());
                    changed = true;
                } else {
                    next.push(v.clone());
                }
            }
            variants = next;
            if !changed {
                break;
            }
        }
        out.extend(variants);
    }

    out.retain(|v| !v.trim().is_empty());
    out.dedup();
    out
}
```

Also add `mod answer_match;` in `backend/src/main.rs` next to the existing `mod adaptive;`-style declarations.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd backend && cargo test answer_match`
Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add backend/Cargo.toml backend/Cargo.lock backend/src/answer_match.rs backend/src/main.rs
git commit -m "feat(matcher): accepted-answer variant parsing + normalization"
```

---

### Task 3: Answer matcher — acceptance tiers

**Files:**
- Modify: `backend/src/answer_match.rs`

**Interfaces:**
- Produces: `pub fn is_correct(typed: &str, accepted_raw: &str) -> bool`. Consumed by Task 8's answer endpoint.

- [ ] **Step 1: Write failing tests** (append inside the existing `mod tests`)

```rust
    #[test]
    fn exact_and_case_insensitive_match() {
        assert!(is_correct("bellerophon", "Bellerophon"));
        assert!(is_correct("The Volga", "the Volga"));
    }

    #[test]
    fn optional_parenthetical_and_alternates_accepted() {
        assert!(is_correct("cromwell", "(Thomas) Cromwell"));
        assert!(is_correct("thomas cromwell", "(Thomas) Cromwell"));
        assert!(is_correct("soviet union", "the U.S.S.R. (or Soviet Union)"));
    }

    #[test]
    fn small_typos_accepted_by_edit_distance() {
        assert!(is_correct("bellerophone", "Bellerophon"));   // 1 insert, long word
        assert!(is_correct("volga", "the Volga"));
        assert!(!is_correct("bell", "Bellerophon"));          // not a typo, a different string
    }

    #[test]
    fn phonetic_spellings_accepted() {
        assert!(is_correct("gavara", "(Che) Guevara"));       // phonetically fine
        assert!(is_correct("olduvye gorge", "Olduvai Gorge")); // per-token phonetic
    }

    #[test]
    fn wrong_and_empty_rejected() {
        assert!(!is_correct("", "Bellerophon"));
        assert!(!is_correct("   ", "Bellerophon"));
        assert!(!is_correct("poseidon", "Gaia"));
        assert!(!is_correct("volgaland river", "the Volga")); // added sounds/words
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd backend && cargo test answer_match`
Expected: FAIL — `is_correct` not defined.

- [ ] **Step 3: Implement** (add above the tests)

```rust
use rphonetic::{DoubleMetaphone, Encoder};
use strsim::damerau_levenshtein;

/// Grade a typed response against the raw accepted-answer string.
/// Tiers: exact normalized → edit distance (≤1 short / ≤2 long) → per-token Double Metaphone.
pub fn is_correct(typed: &str, accepted_raw: &str) -> bool {
    let t = normalize(typed);
    if t.is_empty() {
        return false;
    }
    let dm = DoubleMetaphone::default();

    for variant in accepted_variants(accepted_raw) {
        let v = normalize(&variant);
        if v.is_empty() {
            continue;
        }
        if t == v {
            return true;
        }
        let max_dist = if v.len() < 8 { 1 } else { 2 };
        if damerau_levenshtein(&t, &v) <= max_dist {
            return true;
        }
        // Phonetic: same token count, every token phonetically equal
        // ("phonetically correct without adding or dropping sounds").
        let tt: Vec<&str> = t.split(' ').collect();
        let vt: Vec<&str> = v.split(' ').collect();
        if tt.len() == vt.len()
            && tt.iter().zip(&vt).all(|(a, b)| {
                a == b || (!dm.encode(a).is_empty() && dm.encode(a) == dm.encode(b))
            })
        {
            return true;
        }
    }
    false
}
```

Note: if `DoubleMetaphone::default()` doesn't exist in the resolved rphonetic version, use its documented constructor (e.g. `DoubleMetaphone::new(None)`) — the test suite is the arbiter.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && cargo test answer_match`
Expected: 11 passed. If a phonetic case fails, adjust ONLY the encoder invocation, not the test expectations — these encode the product requirement.

- [ ] **Step 5: Commit**

```bash
git add backend/src/answer_match.rs
git commit -m "feat(matcher): typo + phonetic acceptance tiers"
```

---

### Task 4: Persist attempt_kind in practice grading

**Files:**
- Modify: `backend/src/routes/practice.rs:56-65` (the attempt INSERT in `grade()`)

**Interfaces:**
- Consumes: `question_attempts.attempt_kind` (Task 1).
- Produces: every practice/drill grade writes `'new'` or `'review'`, decided server-side.

- [ ] **Step 1: Replace the attempt INSERT in `grade()`**

Replace:

```rust
    // Record the attempt for existing stats/analytics.
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct) VALUES ($1, $2, $3, $4)",
    )
```

with:

```rust
    // Record the attempt. attempt_kind is decided server-side: first-ever attempt
    // at this question = 'new' (cold), anything later = 'review'.
    let prior: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM question_attempts WHERE user_id = $1 AND question_id = $2)",
    )
    .bind(user_id)
    .bind(body.question_id)
    .fetch_one(&state.pool)
    .await?;
    let kind = if prior { "review" } else { "new" };
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct, attempt_kind) VALUES ($1, $2, $3, $4, $5)",
    )
```

and add `.bind(kind)` after the existing `.bind(rating.is_correct())`.

- [ ] **Step 2: Compile and test**

Run: `cd backend && cargo test`
Expected: all existing tests pass, clean build.

- [ ] **Step 3: Manual verification** — with the backend running locally (`cd backend && cargo run`, needs `DATABASE_URL` exported from `.env`), grade one card via the frontend or `curl`, then:

```sql
SELECT attempt_kind, correct FROM question_attempts WHERE user_id = 1 ORDER BY id DESC LIMIT 1;
```

Expected: `review` for a due card you've seen, `new` for a fresh clue.

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/practice.rs
git commit -m "feat(srs): persist attempt_kind (new/review) at grade time"
```

---

### Task 5: SRS interleaving — new cards from the start of the day

**Files:**
- Modify: `backend/src/routes/practice.rs` (`next()` at :191, plus a new pure function + tests)

**Interfaces:**
- Produces: `pub fn serve_new(new_remaining: i64, due_count: i64, roll: f64) -> bool` (pure, unit-tested); `next()` uses it.

- [ ] **Step 1: Write failing tests** (append to the existing `mod tests` in practice.rs)

```rust
    use super::serve_new;

    #[test]
    fn serve_new_boundaries() {
        assert!(!serve_new(0, 5, 0.0));           // no allowance → never new
        assert!(serve_new(3, 0, 0.99));           // no reviews due → always new
        assert!(!serve_new(0, 0, 0.5));           // nothing available → not new
    }

    #[test]
    fn serve_new_is_proportional() {
        // p(new) = 10/(10+30) = 0.25
        assert!(serve_new(10, 30, 0.24));
        assert!(!serve_new(10, 30, 0.26));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd backend && cargo test serve_new`
Expected: FAIL — `serve_new` not defined.

- [ ] **Step 3: Implement the pure function** (near `day_start_utc`)

```rust
/// Interleave decision: with both new allowance and due reviews available, pick a
/// new card with probability new/(new+due) so new cards spread through the day
/// instead of queueing behind every review. Pure (takes `roll`) for testability.
pub fn serve_new(new_remaining: i64, due_count: i64, roll: f64) -> bool {
    if new_remaining <= 0 {
        return false;
    }
    if due_count <= 0 {
        return true;
    }
    roll < new_remaining as f64 / (new_remaining + due_count) as f64
}
```

- [ ] **Step 4: Rewire `next()`** — replace the "1) due review takes priority / 2) new clue" blocks (practice.rs:225-257) with:

```rust
    // Interleave: decide new-vs-review first, then fall back to the other if the
    // chosen source comes up empty. Net behavior when only one source has items
    // is identical to the old strict priority.
    let want_new = {
        use rand::Rng;
        serve_new(new_remaining, due_count, rand::rng().random())
    };

    let fetch_review = |state: Arc<AppState>| async move {
        sqlx::query_as::<_, ClueRow>(
            "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
                    jq.clue_value, jq.round, jq.air_date, jq.notes
             FROM srs_cards sc
             JOIN jeopardy_questions jq ON jq.id = sc.question_id
             WHERE sc.user_id = $1 AND sc.suspended = false AND sc.due <= now()
               AND jq.archived = false
             ORDER BY sc.due ASC
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
    };

    if want_new {
        if let Some(row) = pick_new_clue(&state, user_id, adaptive, &params).await? {
            pregenerate_insight(&state, row.id);
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }

    if let Some(row) = fetch_review(state.clone()).await? {
        pregenerate_insight(&state, row.id);
        return Ok(Json(json!({
            "done": false, "isNew": false, "card": clue_json(row),
            "dueCount": due_count, "newRemaining": new_remaining,
        })));
    }

    // Review pool empty (or roll chose review with none due) — try a new clue.
    if new_remaining > 0 {
        if let Some(row) = pick_new_clue(&state, user_id, adaptive, &params).await? {
            pregenerate_insight(&state, row.id);
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }
```

(The trailing "3) nothing to do" block stays unchanged.)

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test`
Expected: all pass (including the two new ones).

- [ ] **Step 6: Commit**

```bash
git add backend/src/routes/practice.rs
git commit -m "feat(srs): interleave new cards with due reviews proportionally"
```

---

### Task 6: Conflict error + quota apportionment

**Files:**
- Modify: `backend/src/error.rs`
- Create: `backend/src/routes/mock_test.rs` (module skeleton + pure quota fn + tests)
- Modify: `backend/src/routes/mod.rs` (add `pub mod mock_test;`)

**Interfaces:**
- Produces: `AppError::Conflict(String)` → HTTP 409; `pub fn apportion(dist: &[(String, i64)], seats: i64) -> Vec<(String, i64)>` (largest remainder, sums exactly to `seats`). Task 7 consumes both.

- [ ] **Step 1: Add `Conflict` to `AppError`** — new variant in the enum and match arm in `into_response`:

```rust
    Conflict(String),
```
```rust
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg),
```

- [ ] **Step 2: Write failing tests** (bottom of new `backend/src/routes/mock_test.rs`, with a stub `apportion` returning `vec![]`)

```rust
#[cfg(test)]
mod tests {
    use super::apportion;

    fn seats(v: &[(String, i64)], name: &str) -> i64 {
        v.iter().find(|(c, _)| c == name).map(|(_, s)| *s).unwrap_or(0)
    }

    #[test]
    fn apportion_sums_to_seats_and_tracks_proportion() {
        let dist = vec![
            ("History".to_string(), 30000_i64),
            ("Science".to_string(), 24000),
            ("Math".to_string(), 2500),
        ];
        let q = apportion(&dist, 50);
        assert_eq!(q.iter().map(|(_, s)| s).sum::<i64>(), 50);
        assert!(seats(&q, "History") > seats(&q, "Science"));
        assert!(seats(&q, "Math") >= 1); // largest remainder keeps small cats alive
    }

    #[test]
    fn apportion_handles_empty_and_zero() {
        assert!(apportion(&[], 50).is_empty());
        let q = apportion(&[("A".to_string(), 10)], 50);
        assert_eq!(q, vec![("A".to_string(), 50)]);
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cd backend && cargo test apportion`
Expected: FAIL.

- [ ] **Step 4: Implement**

```rust
use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::answer_match::is_correct;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub const TEST_SIZE: i64 = 50;
pub const PASS_LINE: i64 = 35;

/// Largest-remainder apportionment of `seats` across categories weighted by pool size.
pub fn apportion(dist: &[(String, i64)], seats: i64) -> Vec<(String, i64)> {
    let total: i64 = dist.iter().map(|(_, n)| n).sum();
    if total == 0 || dist.is_empty() {
        return vec![];
    }
    let mut rows: Vec<(String, i64, f64)> = dist
        .iter()
        .map(|(c, n)| {
            let exact = seats as f64 * *n as f64 / total as f64;
            (c.clone(), exact.floor() as i64, exact - exact.floor())
        })
        .collect();
    let mut assigned: i64 = rows.iter().map(|(_, f, _)| f).sum();
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut i = 0;
    while assigned < seats {
        rows[i % rows.len()].1 += 1;
        assigned += 1;
        i += 1;
    }
    rows.into_iter().map(|(c, s, _)| (c, s)).collect()
}
```

Add `pub mod mock_test;` to `backend/src/routes/mod.rs`.

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add backend/src/error.rs backend/src/routes/mock_test.rs backend/src/routes/mod.rs
git commit -m "feat(mock): 409 Conflict error + largest-remainder quota apportionment"
```

---

### Task 7: Mock test backend — create & current

**Files:**
- Modify: `backend/src/routes/mock_test.rs`
- Modify: `backend/src/main.rs` (register routes)

**Interfaces:**
- Consumes: `apportion` (Task 6), mid-band predicate (Global Constraints), tables from Task 1.
- Produces: `POST /api/mock-test` → `{testId, resumed, progress}`; `GET /api/mock-test/current` → `{testId, position, total, clue: {id, category, text}}` or 404 when no active test. `clue.text` is `jeopardy_questions.answer` (the clue — see Global Constraints).

- [ ] **Step 1: Implement `create` and `current`** (append to mock_test.rs)

```rust
const MIDBAND: &str = "((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000) \
                       OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))";

async fn active_test(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<Option<(i32, Vec<i32>, i32)>, AppError> {
    let row: Option<(i32, Vec<i32>, i32)> = sqlx::query_as(
        "SELECT id, question_ids, current_index FROM mock_tests
         WHERE user_id = $1 AND completed_at IS NULL
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row)
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    if let Some((id, _, idx)) = active_test(&state, user_id).await? {
        return Ok(Json(json!({ "testId": id, "resumed": true, "position": idx, "total": TEST_SIZE })));
    }

    // Eligible-pool distribution per category (unseen, mid-band).
    let dist_sql = format!(
        "SELECT jq.classifier_category, COUNT(*)::bigint
         FROM jeopardy_questions jq
         WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
           AND jq.classifier_category IS NOT NULL AND {MIDBAND}
           AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
           AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
         GROUP BY jq.classifier_category"
    );
    let dist: Vec<(String, i64)> = sqlx::query_as(&dist_sql).bind(user_id).fetch_all(&state.pool).await?;
    if dist.iter().map(|(_, n)| n).sum::<i64>() < TEST_SIZE {
        return Err(AppError::BadRequest("Not enough unseen clues for a mock test".into()));
    }

    let quotas = apportion(&dist, TEST_SIZE);
    let mut ids: Vec<i32> = Vec::with_capacity(TEST_SIZE as usize);
    for (cat, seats) in &quotas {
        if *seats == 0 { continue; }
        let sel_sql = format!(
            "SELECT jq.id FROM jeopardy_questions jq
             WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
               AND jq.classifier_category = $2 AND {MIDBAND}
               AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
               AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
             ORDER BY random() LIMIT $3"
        );
        let picked: Vec<(i32,)> = sqlx::query_as(&sel_sql)
            .bind(user_id).bind(cat).bind(seats)
            .fetch_all(&state.pool).await?;
        ids.extend(picked.into_iter().map(|(i,)| i));
    }
    // Shortfall (a category quota exceeded its pool): top up from any category.
    if (ids.len() as i64) < TEST_SIZE {
        let need = TEST_SIZE - ids.len() as i64;
        tracing::warn!("mock test shortfall: borrowing {} clues across categories", need);
        let fill_sql = format!(
            "SELECT jq.id FROM jeopardy_questions jq
             WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
               AND jq.classifier_category IS NOT NULL AND {MIDBAND}
               AND jq.id <> ALL($2)
               AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
               AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
             ORDER BY random() LIMIT $3"
        );
        let extra: Vec<(i32,)> = sqlx::query_as(&fill_sql)
            .bind(user_id).bind(&ids).bind(need)
            .fetch_all(&state.pool).await?;
        ids.extend(extra.into_iter().map(|(i,)| i));
    }

    use rand::seq::SliceRandom;
    ids.shuffle(&mut rand::rng());

    // One quiz_sessions row anchors the question_attempts FK for this test.
    let (session_id,): (i32,) = sqlx::query_as(
        "INSERT INTO quiz_sessions (user_id, is_review_session) VALUES ($1, false) RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let (test_id,): (i32,) = sqlx::query_as(
        "INSERT INTO mock_tests (user_id, session_id, question_ids) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(&ids)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({ "testId": test_id, "resumed": false, "position": 0, "total": TEST_SIZE })))
}

pub async fn current(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (test_id, ids, idx) = active_test(&state, auth.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("No active mock test".into()))?;
    let qid = ids[idx as usize];
    // `answer` is the clue text; `question` (the accepted response) is NOT sent mid-test.
    let (category, text): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT category, answer FROM jeopardy_questions WHERE id = $1",
    )
    .bind(qid)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "testId": test_id, "position": idx, "total": TEST_SIZE,
        "clue": { "id": qid, "category": category, "text": text },
    })))
}
```

- [ ] **Step 2: Register routes in `backend/src/main.rs`** (after the coryat routes)

```rust
        .route("/api/mock-test", post(routes::mock_test::create))
        .route("/api/mock-test/current", get(routes::mock_test::current))
```

- [ ] **Step 3: Compile + smoke test**

Run: `cd backend && cargo test && cargo run` (with `DATABASE_URL` exported), then:
`curl -s -X POST localhost:3000/api/mock-test -H 'Cookie: <auth cookie from a logged-in browser>'`
Expected: `{"testId":N,"resumed":false,"position":0,"total":50}`; `GET /api/mock-test/current` returns a clue with category and text, no accepted answer. Verify in SQL: `SELECT array_length(question_ids,1) FROM mock_tests ORDER BY id DESC LIMIT 1;` → 50.

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/mock_test.rs backend/src/main.rs
git commit -m "feat(mock): test creation with show-distribution quotas + current-clue endpoint"
```

---

### Task 8: Mock test backend — answer, complete, results, override, add-misses, history

**Files:**
- Modify: `backend/src/routes/mock_test.rs`
- Modify: `backend/src/main.rs`

**Interfaces:**
- Consumes: `is_correct` (Task 3), `AppError::Conflict` (Task 6).
- Produces:
  - `POST /api/mock-test/answer` `{position, typedAnswer, responseMs}` → `{completed, position, total, score?}` (409 on position mismatch; grades silently; auto-completes after clue 50)
  - `GET /api/mock-test/{id}/results` → `{score, passLine, completedAt, answers: [{position, clue, category, accepted, typed, autoCorrect, overridden, finalCorrect, responseMs}]}`
  - `POST /api/mock-test/{id}/override` `{position, correct}` → `{score}`
  - `POST /api/mock-test/{id}/add-misses-to-srs` → `{added}`
  - `GET /api/mock-test/history` → `{tests: [{id, completedAt, score}], best}`

- [ ] **Step 1: Implement the five handlers** (append)

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerBody {
    pub position: i32,
    pub typed_answer: String,
    pub response_ms: i32,
}

pub async fn answer(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<AnswerBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let (test_id, ids, idx) = active_test(&state, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("No active mock test".into()))?;
    if body.position != idx {
        return Err(AppError::Conflict(format!("Expected position {idx}")));
    }
    let qid = ids[idx as usize];
    let (accepted,): (Option<String>,) =
        sqlx::query_as("SELECT question FROM jeopardy_questions WHERE id = $1")
            .bind(qid)
            .fetch_one(&state.pool)
            .await?;
    let correct = accepted
        .as_deref()
        .map(|a| is_correct(&body.typed_answer, a))
        .unwrap_or(false);

    sqlx::query(
        "INSERT INTO mock_test_answers
           (mock_test_id, question_id, position, typed_answer, response_ms, auto_correct, final_correct)
         VALUES ($1, $2, $3, $4, $5, $6, $6)
         ON CONFLICT (mock_test_id, position) DO NOTHING",
    )
    .bind(test_id).bind(qid).bind(idx)
    .bind(&body.typed_answer).bind(body.response_ms).bind(correct)
    .execute(&state.pool)
    .await?;

    let (session_id,): (i32,) =
        sqlx::query_as("SELECT session_id FROM mock_tests WHERE id = $1")
            .bind(test_id)
            .fetch_one(&state.pool)
            .await?;
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct, attempt_kind)
         VALUES ($1, $2, $3, $4, 'mock')",
    )
    .bind(session_id).bind(qid).bind(user_id).bind(correct)
    .execute(&state.pool)
    .await?;

    let next_idx = idx + 1;
    sqlx::query("UPDATE mock_tests SET current_index = $2 WHERE id = $1")
        .bind(test_id).bind(next_idx)
        .execute(&state.pool)
        .await?;

    if next_idx as i64 >= TEST_SIZE {
        let (score,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE final_correct) FROM mock_test_answers WHERE mock_test_id = $1",
        )
        .bind(test_id)
        .fetch_one(&state.pool)
        .await?;
        sqlx::query("UPDATE mock_tests SET completed_at = now(), score = $2 WHERE id = $1")
            .bind(test_id).bind(score as i32)
            .execute(&state.pool)
            .await?;
        sqlx::query("UPDATE quiz_sessions SET completed_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(&state.pool)
            .await?;
        return Ok(Json(json!({ "completed": true, "position": next_idx, "total": TEST_SIZE, "score": score })));
    }
    Ok(Json(json!({ "completed": false, "position": next_idx, "total": TEST_SIZE })))
}

/// Loads a completed, owned test or errors.
async fn owned_completed_test(
    state: &Arc<AppState>,
    user_id: i32,
    test_id: i32,
) -> Result<(i32, Option<i32>), AppError> {
    let row: Option<(i32, Option<i32>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT session_id, score, completed_at FROM mock_tests WHERE id = $1 AND user_id = $2",
    )
    .bind(test_id).bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    match row {
        None => Err(AppError::NotFound("Mock test not found".into())),
        Some((_, _, None)) => Err(AppError::BadRequest("Mock test not completed".into())),
        Some((session_id, score, Some(_))) => Ok((session_id, score)),
    }
}

pub async fn results(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let _ = owned_completed_test(&state, auth.user_id, test_id).await?;
    #[derive(sqlx::FromRow)]
    struct Row {
        position: i32,
        typed_answer: String,
        response_ms: i32,
        auto_correct: bool,
        overridden: bool,
        final_correct: bool,
        clue: Option<String>,
        accepted: Option<String>,
        category: Option<String>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT mta.position, mta.typed_answer, mta.response_ms, mta.auto_correct,
                mta.overridden, mta.final_correct,
                jq.answer AS clue, jq.question AS accepted, jq.category
         FROM mock_test_answers mta
         JOIN jeopardy_questions jq ON jq.id = mta.question_id
         WHERE mta.mock_test_id = $1
         ORDER BY mta.position",
    )
    .bind(test_id)
    .fetch_all(&state.pool)
    .await?;

    let (score, completed_at): (Option<i32>, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT score, completed_at FROM mock_tests WHERE id = $1")
            .bind(test_id)
            .fetch_one(&state.pool)
            .await?;

    let answers: Vec<Value> = rows.into_iter().map(|r| json!({
        "position": r.position, "clue": r.clue, "category": r.category,
        "accepted": r.accepted, "typed": r.typed_answer, "responseMs": r.response_ms,
        "autoCorrect": r.auto_correct, "overridden": r.overridden, "finalCorrect": r.final_correct,
    })).collect();

    Ok(Json(json!({ "score": score, "passLine": PASS_LINE, "completedAt": completed_at, "answers": answers })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideBody {
    pub position: i32,
    pub correct: bool,
}

pub async fn override_verdict(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
    Json(body): Json<OverrideBody>,
) -> Result<Json<Value>, AppError> {
    let (session_id, _) = owned_completed_test(&state, auth.user_id, test_id).await?;

    let qid: Option<i32> = sqlx::query_scalar(
        "UPDATE mock_test_answers SET overridden = true, final_correct = $3
         WHERE mock_test_id = $1 AND position = $2 RETURNING question_id",
    )
    .bind(test_id).bind(body.position).bind(body.correct)
    .fetch_optional(&state.pool)
    .await?;
    let qid = qid.ok_or_else(|| AppError::NotFound("No answer at that position".into()))?;

    sqlx::query(
        "UPDATE question_attempts SET correct = $4
         WHERE session_id = $1 AND question_id = $2 AND user_id = $3 AND attempt_kind = 'mock'",
    )
    .bind(session_id).bind(qid).bind(auth.user_id).bind(body.correct)
    .execute(&state.pool)
    .await?;

    let (score,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE final_correct) FROM mock_test_answers WHERE mock_test_id = $1",
    )
    .bind(test_id)
    .fetch_one(&state.pool)
    .await?;
    sqlx::query("UPDATE mock_tests SET score = $2 WHERE id = $1")
        .bind(test_id).bind(score as i32)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({ "score": score })))
}

pub async fn add_misses_to_srs(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let _ = owned_completed_test(&state, auth.user_id, test_id).await?;
    let added: i64 = sqlx::query_scalar(
        "WITH ins AS (
           INSERT INTO srs_cards (user_id, question_id)
           SELECT $2, mta.question_id
           FROM mock_test_answers mta
           WHERE mta.mock_test_id = $1 AND mta.final_correct = false
           ON CONFLICT (user_id, question_id) DO NOTHING
           RETURNING 1
         ) SELECT COUNT(*) FROM ins",
    )
    .bind(test_id).bind(auth.user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({ "added": added })))
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rows: Vec<(i32, Option<DateTime<Utc>>, Option<i32>)> = sqlx::query_as(
        "SELECT id, completed_at, score FROM mock_tests
         WHERE user_id = $1 AND completed_at IS NOT NULL
         ORDER BY completed_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;
    let best = rows.iter().filter_map(|(_, _, s)| *s).max();
    let tests: Vec<Value> = rows.into_iter()
        .map(|(id, at, s)| json!({ "id": id, "completedAt": at, "score": s }))
        .collect();
    Ok(Json(json!({ "tests": tests, "best": best, "passLine": PASS_LINE })))
}
```

Note: `srs_cards` defaults (`state='learning'`, `due=now()`, `step_index=0`) make the two-column INSERT in `add_misses_to_srs` sufficient.

- [ ] **Step 2: Register in `main.rs`**

```rust
        .route("/api/mock-test/answer", post(routes::mock_test::answer))
        .route("/api/mock-test/history", get(routes::mock_test::history))
        .route("/api/mock-test/{id}/results", get(routes::mock_test::results))
        .route("/api/mock-test/{id}/override", post(routes::mock_test::override_verdict))
        .route("/api/mock-test/{id}/add-misses-to-srs", post(routes::mock_test::add_misses_to_srs))
```

- [ ] **Step 3: Compile + tests**

Run: `cd backend && cargo test`
Expected: all pass, clean build.

- [ ] **Step 4: Manual lifecycle smoke test** — with backend running and an auth cookie: create a test, answer position 0 twice (second → 409), answer with a deliberately misspelled-but-phonetic response and check `auto_correct=true` in `mock_test_answers`. Full 50-answer run happens in Task 12.

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/mock_test.rs backend/src/main.rs
git commit -m "feat(mock): answer/complete/results/override/add-misses/history endpoints"
```

---

### Task 9: Stats — cold/review split + mock readiness

**Files:**
- Modify: `backend/src/routes/stats.rs`

**Interfaces:**
- Consumes: `attempt_kind` (Task 1), `mock_tests` (Task 1).
- Produces (new/changed keys in `GET /api/stats`; existing keys keep their shape):
  - `cold: {total, correct, accuracy}` and `review: {total, correct, accuracy}` (all-time, mock excluded)
  - `cold30d: {total, correct, accuracy}`
  - `categoryBreakdown[]` entries gain `coldTotal, coldCorrect, coldAccuracy, reviewTotal, reviewCorrect, reviewAccuracy`
  - `dailyAccuracy[]` entries gain `coldTotal, coldCorrect, coldAccuracy, reviewTotal, reviewCorrect, reviewAccuracy`
  - `mockReadiness: {tests: [{id, completedAt, score}], best, latest, passLine}`
  - Every pre-existing aggregate now excludes `attempt_kind='mock'`.

- [ ] **Step 1: Exclude mock from existing aggregates** — in `overall_sql`, `category_sql`, and the `daily_accuracy_rows` query, add `AND qa.attempt_kind <> 'mock'` to the WHERE clause. (`sessions_sql`/`daily_sql` aggregate via sessions; the mock session is one extra row in "recent sessions" — acceptable.)

- [ ] **Step 2: Add the split queries** (after the `overall` block, following its style)

```rust
    // Cold vs review (all-time) and cold last-30d — the test-relevant metrics.
    let kind_split: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT attempt_kind, COUNT(*)::bigint,
                COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0)::bigint
         FROM question_attempts
         WHERE user_id = $1 AND attempt_kind IN ('new', 'review')
         GROUP BY attempt_kind",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let pack = |t: i64, c: i64| {
        json!({ "total": t, "correct": c,
                "accuracy": if t > 0 { c as f64 / t as f64 * 100.0 } else { 0.0 } })
    };
    let find = |k: &str| kind_split.iter().find(|(kind, _, _)| kind == k)
        .map(|(_, t, c)| (*t, *c)).unwrap_or((0, 0));
    let (cold_t, cold_c) = find("new");
    let (rev_t, rev_c) = find("review");

    let (c30_t, c30_c): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0)::bigint
         FROM question_attempts
         WHERE user_id = $1 AND attempt_kind = 'new' AND answered_at >= now() - interval '30 days'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let mock_rows: Vec<(i32, Option<chrono::DateTime<chrono::Utc>>, Option<i32>)> = sqlx::query_as(
        "SELECT id, completed_at, score FROM mock_tests
         WHERE user_id = $1 AND completed_at IS NOT NULL ORDER BY completed_at ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let mock_best = mock_rows.iter().filter_map(|(_, _, s)| *s).max();
    let mock_latest = mock_rows.last().and_then(|(_, _, s)| *s);
    let mock_tests: Vec<Value> = mock_rows.into_iter()
        .map(|(id, at, s)| json!({ "id": id, "completedAt": at, "score": s }))
        .collect();
```

- [ ] **Step 3: Split category breakdown and dailyAccuracy by kind** — replace `category_sql` with:

```rust
    let category_sql = format!(
        "SELECT jq.classifier_category,
          COUNT(*)::bigint as total,
          SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::bigint as correct,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'new')::bigint as cold_total,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'new' AND qa.correct)::bigint as cold_correct,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'review')::bigint as review_total,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'review' AND qa.correct)::bigint as review_correct
        FROM question_attempts qa
        JOIN jeopardy_questions jq ON qa.question_id = jq.id
        JOIN quiz_sessions qs ON qa.session_id = qs.id
        WHERE qa.user_id = $1 AND jq.archived = false AND qa.attempt_kind <> 'mock'{}
        GROUP BY jq.classifier_category
        ORDER BY jq.classifier_category",
        review_filter
    );
```

extend `CategoryStat` with the four new `i64` fields (`cold_total`, `cold_correct`, `review_total`, `review_correct`), and emit the six new JSON keys per entry (accuracy computed like the existing one, 0.0 when the denominator is 0). Apply the same `FILTER` pattern to the `daily_accuracy_rows` query and its JSON.

- [ ] **Step 4: Extend the response JSON**

```rust
        "cold": pack(cold_t, cold_c),
        "review": pack(rev_t, rev_c),
        "cold30d": pack(c30_t, c30_c),
        "mockReadiness": {
            "tests": mock_tests, "best": mock_best, "latest": mock_latest,
            "passLine": crate::routes::mock_test::PASS_LINE,
        },
```

- [ ] **Step 5: Compile, run, verify against baseline**

Run: `cd backend && cargo test && cargo run`, then `curl -s 'localhost:3000/api/stats' -H 'Cookie: …' | python3 -m json.tool | head -40`
Expected: `cold.accuracy` ≈ 50.x and `cold.total` ≈ 1360–1400 for user 1 (matches Task 1's verification numbers); `review.accuracy` ≈ 75–80.

- [ ] **Step 6: Commit**

```bash
git add backend/src/routes/stats.rs
git commit -m "feat(stats): cold/review split + mock readiness in /api/stats"
```

---

### Task 10: Frontend — /mock page

**Files:**
- Create: `frontend/src/routes/mock/+page.svelte`
- Modify: `frontend/src/lib/components/Nav.svelte:9-14` (links array)

**Interfaces:**
- Consumes: all `/api/mock-test*` endpoints (Tasks 7–8, exact shapes as specified there).

- [ ] **Step 1: Add the nav link** — in the `links` array after Drill:

```ts
    { href: '/mock', label: 'Mock Test' },
```

- [ ] **Step 2: Build the page.** One component, three phases (`idle → active → results`). Complete implementation:

```svelte
<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount, onDestroy } from 'svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  const CLUE_MS = 15000;

  type Phase = 'idle' | 'active' | 'results';
  let phase = $state<Phase>('idle');
  let loading = $state(false);
  let error = $state('');
  let hasResumable = $state(false);

  // Active-test state
  let testId = $state<number | null>(null);
  let position = $state(0);
  let total = $state(50);
  let clue = $state<{ id: number; category: string; text: string } | null>(null);
  let typed = $state('');
  let deadline = 0;               // performance.now() when the clock hits zero
  let remainingMs = $state(CLUE_MS);
  let timerHandle: ReturnType<typeof setInterval> | null = null;
  let submitting = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);

  // Results state
  let results = $state<any>(null);

  onMount(async () => {
    // Detect a resumable test without starting a new one.
    try {
      await api.get('/api/mock-test/current');
      hasResumable = true;
    } catch { /* 404 = none */ }
  });

  function stopTimer() {
    if (timerHandle) clearInterval(timerHandle);
    timerHandle = null;
  }

  function startTimer() {
    stopTimer();
    deadline = performance.now() + CLUE_MS;
    remainingMs = CLUE_MS;
    timerHandle = setInterval(() => {
      remainingMs = Math.max(0, deadline - performance.now());
      if (remainingMs <= 0) submit();   // auto-submit whatever is typed
    }, 100);
  }

  onDestroy(stopTimer); // never let the timer keep firing after navigating away mid-test

  async function loadCurrent() {
    const cur = await api.get('/api/mock-test/current');
    testId = cur.testId;
    position = cur.position;
    total = cur.total;
    clue = cur.clue;
    typed = '';
    phase = 'active';
    startTimer();
    queueMicrotask(() => inputEl?.focus());
  }

  async function start() {
    loading = true;
    error = '';
    try {
      await api.post('/api/mock-test');
      await loadCurrent();
    } catch (e: any) {
      error = e?.message ?? 'Could not start test';
    } finally {
      loading = false;
    }
  }

  async function submit() {
    if (submitting || phase !== 'active') return;
    submitting = true;
    stopTimer();
    const responseMs = Math.min(CLUE_MS, Math.max(0, Math.round(CLUE_MS - (deadline - performance.now()))));
    try {
      const res = await api.post('/api/mock-test/answer', {
        position,
        typedAnswer: typed,
        responseMs,
      });
      if (res.completed) {
        await showResults(testId!);
      } else {
        await loadCurrent();
      }
    } catch (e: any) {
      if (e?.status === 409) {
        await loadCurrent();      // position drift — resync
      } else {
        error = e?.message ?? 'Submit failed';
      }
    } finally {
      submitting = false;
    }
  }

  async function showResults(id: number) {
    results = await api.get(`/api/mock-test/${id}/results`);
    phase = 'results';
  }

  let overridingPos = $state<number | null>(null);
  async function toggleOverride(row: any) {
    if (overridingPos !== null) return; // one override in flight at a time
    overridingPos = row.position;
    try {
      const res = await api.post(`/api/mock-test/${testId}/override`, {
        position: row.position,
        correct: !row.finalCorrect,
      });
      row.finalCorrect = !row.finalCorrect;
      row.overridden = true;
      results.score = res.score;
    } finally {
      overridingPos = null;
    }
  }

  let addingMisses = $state(false);
  let missesAdded = $state<number | null>(null);
  async function addMisses() {
    addingMisses = true;
    try {
      const res = await api.post(`/api/mock-test/${testId}/add-misses-to-srs`);
      missesAdded = res.added;
    } finally {
      addingMisses = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && phase === 'active') submit();
  }

  let secondsLeft = $derived(Math.ceil(remainingMs / 1000));
  let barPct = $derived((remainingMs / CLUE_MS) * 100);
</script>

<svelte:head><title>Mock Test — Jeopardy! Training</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto">
    {#if phase === 'idle'}
      <div class="bg-white rounded-xl shadow p-8 text-center">
        <h1 class="text-3xl font-bold text-jeopardy-blue mb-3">Anytime Test Simulator</h1>
        <p class="text-gray-600 mb-2">50 clues you've never seen · 15 seconds each · typed answers.</p>
        <p class="text-gray-500 text-sm mb-6">
          No feedback until the end — just like the real thing. Spelling is graded phonetically.
          The commonly-cited pass line is 35/50.
        </p>
        {#if error}<p class="text-red-600 mb-4">{error}</p>{/if}
        <button
          onclick={start}
          disabled={loading}
          class="px-8 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-800 transition-colors disabled:opacity-50"
        >
          {hasResumable ? 'Resume Test' : 'Start Test'}
        </button>
      </div>
    {:else if phase === 'active' && clue}
      <div class="bg-white rounded-xl shadow p-8">
        <div class="flex items-center justify-between mb-4 text-sm text-gray-500">
          <span>Clue {position + 1} / {total}</span>
          <span class="font-mono text-lg {secondsLeft <= 5 ? 'text-red-600 font-bold' : 'text-gray-700'}">{secondsLeft}s</span>
        </div>
        <div class="h-1.5 bg-gray-100 rounded-full overflow-hidden mb-6">
          <div class="h-full bg-jeopardy-gold transition-none" style="width: {barPct}%"></div>
        </div>
        <p class="text-xs uppercase tracking-wide text-jeopardy-blue font-bold mb-2">{clue.category}</p>
        <p class="text-xl text-gray-900 mb-6">{clue.text}</p>
        <input
          bind:this={inputEl}
          bind:value={typed}
          onkeydown={onKeydown}
          disabled={submitting}
          placeholder="Type your answer…"
          autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false"
          class="w-full px-4 py-3 border-2 border-jeopardy-blue rounded-lg text-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-gold"
        />
        <p class="text-xs text-gray-400 mt-2">Enter submits · auto-submits at 0:00 · don't phrase as a question</p>
      </div>
    {:else if phase === 'results' && results}
      <div class="bg-white rounded-xl shadow p-8 mb-6 text-center">
        <h1 class="text-2xl font-bold text-gray-800 mb-1">Score</h1>
        <p class="text-5xl font-extrabold {results.score >= results.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
          {results.score}/50
        </p>
        <p class="text-gray-500 mt-2">
          {results.score >= results.passLine
            ? `At or above the commonly-cited pass line (${results.passLine}).`
            : `${results.passLine - results.score} short of the commonly-cited pass line (${results.passLine}).`}
        </p>
        <div class="mt-4 flex justify-center gap-3">
          <button
            onclick={addMisses}
            disabled={addingMisses || missesAdded !== null}
            class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 disabled:opacity-50"
          >
            {missesAdded !== null ? `${missesAdded} misses added to deck` : 'Add misses to SRS deck'}
          </button>
          <a href="/dashboard" class="px-4 py-2 rounded-lg border border-gray-300 text-sm font-semibold text-gray-700 hover:bg-gray-50">Dashboard</a>
        </div>
      </div>
      <div class="bg-white rounded-xl shadow divide-y divide-gray-100">
        {#each results.answers as row}
          <div class="p-4 flex gap-4 items-start">
            <span class="mt-1 shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold
              {row.finalCorrect ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}">
              {row.finalCorrect ? '✓' : '✗'}
            </span>
            <div class="flex-1 min-w-0">
              <p class="text-xs uppercase text-gray-400">{row.category}</p>
              <p class="text-sm text-gray-800">{row.clue}</p>
              <p class="text-sm mt-1">
                <span class="text-gray-500">You:</span>
                <span class="{row.finalCorrect ? 'text-green-700' : 'text-red-700'} font-medium">{row.typed || '(no answer)'}</span>
                <span class="text-gray-400 mx-1">·</span>
                <span class="text-gray-500">Accepted:</span> <span class="font-medium">{row.accepted}</span>
                {#if row.overridden}<span class="ml-1 text-[10px] uppercase text-amber-600 font-bold">overridden</span>{/if}
              </p>
            </div>
            <button
              onclick={() => toggleOverride(row)}
              disabled={overridingPos !== null}
              class="shrink-0 text-xs px-2 py-1 rounded border border-gray-300 text-gray-600 hover:bg-gray-50 disabled:opacity-50"
            >
              Mark {row.finalCorrect ? 'wrong' : 'right'}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
```

- [ ] **Step 3: Type-check**

Run: `cd frontend && npm run check`
Expected: 0 errors. (Note: `api.ts`'s `ApiError` carries `.status` — the 409 resync path relies on it.)

- [ ] **Step 4: Manual test** — `cd frontend && npm run dev` (proxies `/api` → localhost:3000; backend running). Start a test: timer counts down, empty auto-submit at 0:00 advances, Enter submits, refresh mid-test resumes, after 50 the results screen shows verdicts, override flips score, add-misses reports a count.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/mock frontend/src/lib/components/Nav.svelte
git commit -m "feat(mock): /mock test page with 15s timer, results review, overrides"
```

---

### Task 11: Frontend — dashboard cold/review breakout + readiness tile

**Files:**
- Modify: `frontend/src/routes/dashboard/+page.svelte`

**Interfaces:**
- Consumes: `cold`, `review`, `cold30d`, `mockReadiness`, extended `categoryBreakdown`/`dailyAccuracy` (Task 9 shapes).

- [ ] **Step 1: Extend the `Stats` interface** (dashboard/+page.svelte:8-15)

```ts
  interface KindStat { total: number; correct: number; accuracy: number }
  interface Stats {
    overall: KindStat;
    cold: KindStat;
    review: KindStat;
    cold30d: KindStat;
    mockReadiness: { tests: Array<{ id: number; completedAt: string; score: number }>; best: number | null; latest: number | null; passLine: number };
    categoryBreakdown: Array<{ category: string; total: number; correct: number; accuracy: number;
      coldTotal: number; coldCorrect: number; coldAccuracy: number;
      reviewTotal: number; reviewCorrect: number; reviewAccuracy: number }>;
    recentSessions: Array<{ id: number; started_at: string; completed_at: string; total: number; correct: number }>;
    dailyStats: Array<{ date: string; avgPercentage: number; sessionCount: number }>;
    dailyAccuracy: Array<{ date: string; total: number; correct: number; accuracy: number;
      coldTotal: number; coldCorrect: number; coldAccuracy: number;
      reviewTotal: number; reviewCorrect: number; reviewAccuracy: number }>;
  }
```

- [ ] **Step 2: Replace the "Overall Stats Cards" block** (`<!-- Overall Stats Cards -->`, lines ~332-348) with cold-headline cards:

```svelte
      <!-- Cold (test-relevant) vs review stats -->
      <div class="flex flex-wrap gap-4 mb-8">
        <div class="flex-[2] min-w-[240px] bg-white rounded-xl shadow p-6 border-2 border-jeopardy-blue">
          <p class="text-sm font-medium text-gray-500 mb-1">Cold Accuracy — last 30 days</p>
          <p class="text-4xl font-extrabold {stats.cold30d.accuracy >= 70 ? 'text-green-600' : stats.cold30d.accuracy >= 55 ? 'text-amber-500' : 'text-red-500'}">
            {stats.cold30d.accuracy.toFixed(1)}%
          </p>
          <p class="text-xs text-gray-400 mt-1">
            First-attempt questions only ({stats.cold30d.total} clues) — the number the Anytime Test measures. All-time: {stats.cold.accuracy.toFixed(1)}%.
          </p>
        </div>
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Retention (review accuracy)</p>
          <p class="text-3xl font-bold text-jeopardy-blue">{stats.review.accuracy.toFixed(1)}%</p>
          <p class="text-xs text-gray-400 mt-1">{stats.review.total.toLocaleString()} SRS reviews</p>
        </div>
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Mock Test Readiness</p>
          {#if stats.mockReadiness.tests.length > 0}
            <p class="text-3xl font-bold {stats.mockReadiness.latest! >= stats.mockReadiness.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
              {stats.mockReadiness.latest}/50
            </p>
            <p class="text-xs text-gray-400 mt-1">Best {stats.mockReadiness.best}/50 · pass line {stats.mockReadiness.passLine} · <a href="/mock" class="text-jeopardy-blue hover:underline">take another →</a></p>
          {:else}
            <p class="text-sm text-gray-500 mt-1">No mocks yet.</p>
            <a href="/mock" class="text-sm font-semibold text-jeopardy-blue hover:underline">Take your first mock test →</a>
          {/if}
        </div>
      </div>
```

- [ ] **Step 3: Two-series trend chart** — replace `lineChartData` (lines ~77-96):

```ts
  let lineChartData = $derived(
    stats?.dailyAccuracy?.length
      ? {
          labels: stats.dailyAccuracy.map((d) => d.date),
          datasets: [
            {
              label: 'Cold (first attempt) %',
              data: stats.dailyAccuracy.map((d) => (d.coldTotal > 0 ? d.coldAccuracy : null)),
              borderColor: '#0c47b7',
              borderWidth: 2.5,
              pointRadius: 3,
              pointBackgroundColor: '#0c47b7',
              fill: false,
              tension: 0.3,
              spanGaps: true,
            },
            {
              label: 'Review %',
              data: stats.dailyAccuracy.map((d) => (d.reviewTotal > 0 ? d.reviewAccuracy : null)),
              borderColor: '#9ca3af',
              borderWidth: 1.5,
              pointRadius: 2,
              pointBackgroundColor: '#9ca3af',
              fill: false,
              tension: 0.3,
              spanGaps: true,
            },
          ],
        }
      : null
  );
```

and in `lineChartOptions` set `plugins: { legend: { display: true, position: 'bottom' } }`.

- [ ] **Step 4: Cold-driven category chart + table** — in `barChartData`, use `c.coldAccuracy` for `data` and color thresholds, and label `'Cold accuracy %'`; in `sortedCategories`, sort by `coldAccuracy`; in the table add Cold/Review columns:

header row:
```svelte
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Cold</th>
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Review</th>
```
body row (replacing the single Accuracy cell):
```svelte
                    <td class="py-3 px-4 text-right font-medium {cat.coldAccuracy >= 70 ? 'text-green-600' : cat.coldAccuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
                      {cat.coldTotal > 0 ? `${cat.coldAccuracy.toFixed(1)}% (${cat.coldTotal})` : '—'}
                    </td>
                    <td class="py-3 px-4 text-right text-gray-600">
                      {cat.reviewTotal > 0 ? `${cat.reviewAccuracy.toFixed(1)}% (${cat.reviewTotal})` : '—'}
                    </td>
```

- [ ] **Step 5: Check + manual verify**

Run: `cd frontend && npm run check` → 0 errors. In the browser: headline shows cold 30d (~50s%), retention ~75-80%, readiness tile links to /mock, trend chart shows two labeled lines, category table has Cold and Review columns.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/routes/dashboard/+page.svelte
git commit -m "feat(dashboard): cold-accuracy headline, review split, mock readiness tile"
```

---

### Task 12: End-to-end verification & deploy checklist

**Files:** none (verification only)

- [ ] **Step 1: Full test suites** — `cd backend && cargo test` (expect: all pass, including answer_match ×11, apportion ×2, serve_new ×2, day_start ×2, srs scheduler suite) and `cd frontend && npm run check` (0 errors), `npm run build` (clean).

- [ ] **Step 2: Full mock test run-through** in the browser (use the `/verify` skill flow): complete all 50 clues, confirming timer auto-submit on at least one clue, Enter-submit on others, a mid-test refresh resume, results with phonetic grading visible (misspell one answer you know), one override, add-misses count, dashboard readiness tile updates after reload.

- [ ] **Step 3: Interleave sanity check** — with due reviews > 0 and new allowance > 0, pull ~10 practice cards; expect a mix of `isNew` true/false rather than reviews-only. SQL check: `SELECT attempt_kind, count(*) FROM question_attempts WHERE user_id=1 AND answered_at > now() - interval '1 hour' GROUP BY 1;`

- [ ] **Step 4: Deploy checklist (user-approved; live service on Tower)**
  1. Migration 0005 is already applied (Task 1) and idempotent.
  2. Build & push the container, redeploy on Tower (existing `tower` skill / DEPLOYMENT.md flow).
  3. **Immediately after cutover, re-run the backfill portion of migration 0005** (idempotent) to reclassify any attempts the old binary wrote with the `'review'` default between migration and deploy.
  4. Smoke-test `/mock` and the dashboard on the deployed app.

- [ ] **Step 5: Final commit if any fixups emerged, then report completion to the user with the verification evidence.**

---

### Task 13: Primer library — backend

**Files:**
- Create: `backend/migrations/0006_primers.sql`
- Create: `backend/src/routes/primers.rs`
- Modify: `backend/src/routes/mod.rs` (add `pub mod primers;`), `backend/src/main.rs` (register routes)

**Interfaces:**
- Consumes: `crate::openai::chat_json(&api_key, MODEL, SYSTEM, user, temp) -> Result<Value, AppError>`; the blindspot generator's model constant (`PACK_MODEL` in `backend/src/blindspots.rs`) — use the same model string value as `PRIMER_MODEL`.
- Produces: `GET /api/primers` → `{primers: [{id, slug, topic, source, createdAt}], canon: [string], configured: bool}`; `GET /api/primers/{slug}` → `{id, slug, topic, source, contentMd, createdAt}`; `POST /api/primers/generate` `{topic, source?}` → the same shape as the get-by-slug response (`cached: true` when it already existed). Task 14 consumes these.

- [ ] **Step 1: Write migration `backend/migrations/0006_primers.sql`**

```sql
-- 0006: shared primer library (LLM-generated long-form study guides).
CREATE TABLE IF NOT EXISTS primers (
  id           SERIAL PRIMARY KEY,
  slug         TEXT NOT NULL UNIQUE,
  topic        TEXT NOT NULL,
  content_md   TEXT NOT NULL,
  model        TEXT NOT NULL,
  source       TEXT NOT NULL DEFAULT 'custom' CHECK (source IN ('canon', 'blindspot', 'custom')),
  requested_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Apply: `scripts/apply-migration.sh backend/migrations/0006_primers.sql` — expect `CREATE TABLE`.

- [ ] **Step 2: Write failing unit tests for `slugify`** (bottom of new `backend/src/routes/primers.rs`, stub returning `String::new()`)

```rust
#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Greek & Roman Mythology"), "greek-roman-mythology");
        assert_eq!(slugify("  New Deal & FDR  "), "new-deal-fdr");
        assert_eq!(slugify("Opera"), "opera");
        assert_eq!(slugify("U.S. Presidents"), "u-s-presidents");
    }
}
```

Run: `cd backend && cargo test slugify` — expect FAIL.

- [ ] **Step 3: Implement the module**

```rust
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

/// Same model as the blindspot generator (see PACK_MODEL in blindspots.rs).
const PRIMER_MODEL: &str = "<copy the PACK_MODEL value from backend/src/blindspots.rs verbatim>";

pub const CANON_TOPICS: &[&str] = &[
    "Opera",
    "Greek & Roman Mythology",
    "Norse Mythology",
    "Art Movements & Artists",
    "Baseball History",
    "New Deal & FDR",
    "Civil Rights Movement",
    "Shakespeare",
    "U.S. Presidents",
    "World Geography — Capitals & Rivers",
    "The Bible",
    "British Royals & History",
];

const PRIMER_SYSTEM_PROMPT: &str = "You write study primers for Jeopardy! preparation. \
Return JSON: {\"title\": string, \"content_md\": string}. content_md is a 1500-2500 word \
GitHub-flavored markdown study guide with these sections: \
## How this topic appears on Jeopardy (clue styles, frequency, typical difficulty); \
## The core canon (the facts that cover most clues, as markdown tables or tight lists — \
e.g. for opera: composer | work | plot one-liner | famous aria); \
## Clue angles & pivot words (the phrasings and giveaway words clues hinge on); \
## Mnemonic hooks (memorable groupings and associations); \
## Practice pairs (10 sample clue -> correct response pairs in Jeopardy style). \
Be specific and factual; prefer canonical, frequently-tested material over trivia depth.";

pub fn slugify(topic: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true; // suppress leading dash
    for c in topic.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn primer_json(row: (i32, String, String, String, String, chrono::DateTime<chrono::Utc>), cached: bool) -> Value {
    let (id, slug, topic, source, content_md, created_at) = row;
    json!({ "id": id, "slug": slug, "topic": topic, "source": source,
            "contentMd": content_md, "createdAt": created_at, "cached": cached })
}

type PrimerRow = (i32, String, String, String, String, chrono::DateTime<chrono::Utc>);
const PRIMER_COLS: &str = "id, slug, topic, source, content_md, created_at";

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rows: Vec<(i32, String, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, slug, topic, source, created_at FROM primers ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    let primers: Vec<Value> = rows.into_iter()
        .map(|(id, slug, topic, source, at)| json!({ "id": id, "slug": slug, "topic": topic, "source": source, "createdAt": at }))
        .collect();
    Ok(Json(json!({
        "primers": primers,
        "canon": CANON_TOPICS,
        "configured": !state.config.openai_api_key.is_empty(),
    })))
}

pub async fn get_primer(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(slug): Path<String>,
) -> Result<Json<Value>, AppError> {
    let sql = format!("SELECT {PRIMER_COLS} FROM primers WHERE slug = $1");
    let row: Option<PrimerRow> = sqlx::query_as(&sql).bind(&slug).fetch_optional(&state.pool).await?;
    row.map(|r| Json(primer_json(r, true)))
        .ok_or_else(|| AppError::NotFound("Primer not found".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateBody {
    pub topic: String,
    pub source: Option<String>,
}

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<GenerateBody>,
) -> Result<Json<Value>, AppError> {
    let topic = body.topic.trim().to_string();
    if topic.is_empty() || topic.len() > 120 {
        return Err(AppError::BadRequest("Topic must be 1-120 characters".into()));
    }
    let source = match body.source.as_deref() {
        Some("canon") | None if CANON_TOPICS.contains(&topic.as_str()) => "canon",
        Some("blindspot") => "blindspot",
        _ => "custom",
    };
    let slug = slugify(&topic);
    if slug.is_empty() {
        return Err(AppError::BadRequest("Topic has no usable characters".into()));
    }

    let sel = format!("SELECT {PRIMER_COLS} FROM primers WHERE slug = $1");
    if let Some(row) = sqlx::query_as::<_, PrimerRow>(&sel).bind(&slug).fetch_optional(&state.pool).await? {
        return Ok(Json(primer_json(row, true)));
    }
    if state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest("Primer generation is not configured (no API key)".into()));
    }

    let user_prompt = format!("Topic: {topic}\nReturn the JSON now.");
    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        PRIMER_MODEL,
        PRIMER_SYSTEM_PROMPT,
        &user_prompt,
        0.7,
    )
    .await?;
    let content_md = v["content_md"]
        .as_str()
        .ok_or_else(|| AppError::Internal("LLM response missing content_md".into()))?
        .to_string();
    if content_md.len() < 500 {
        return Err(AppError::Internal("LLM primer implausibly short".into()));
    }

    // Concurrent-generation guard: first writer wins, everyone re-selects.
    sqlx::query(
        "INSERT INTO primers (slug, topic, content_md, model, source, requested_by)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (slug) DO NOTHING",
    )
    .bind(&slug).bind(&topic).bind(&content_md)
    .bind(PRIMER_MODEL).bind(source).bind(auth.user_id)
    .execute(&state.pool)
    .await?;
    let row: PrimerRow = sqlx::query_as(&sel).bind(&slug).fetch_one(&state.pool).await?;
    Ok(Json(primer_json(row, false)))
}
```

Replace the `PRIMER_MODEL` placeholder with the verbatim `PACK_MODEL` value from `backend/src/blindspots.rs` before compiling. Add `pub mod primers;` to `backend/src/routes/mod.rs` and register in `main.rs`:

```rust
        .route("/api/primers", get(routes::primers::list))
        .route("/api/primers/generate", post(routes::primers::generate))
        .route("/api/primers/{slug}", get(routes::primers::get_primer))
```

(Register `/api/primers/generate` BEFORE `/api/primers/{slug}` is not required — axum matches static segments over captures — but keep this order for readability.)

- [ ] **Step 4: Run tests**

Run: `cd backend && cargo test`
Expected: all pass including `slugify_basics`.

- [ ] **Step 5: Manual smoke test** — with backend running and an auth cookie: `GET /api/primers` returns canon list + `configured`; if `OPENAI_API_KEY` is present locally, `POST /api/primers/generate {"topic":"Opera"}` returns a primer with `contentMd` and a second POST returns `cached: true`. If no local key, verify the `BadRequest` path and note deploy-time verification in the report.

- [ ] **Step 6: Commit**

```bash
git add backend/migrations/0006_primers.sql backend/src/routes/primers.rs backend/src/routes/mod.rs backend/src/main.rs
git commit -m "feat(primers): LLM-generated primer library backend (list/get/generate)"
```

---

### Task 14: Primer library — frontend

**Files:**
- Create: `frontend/src/routes/primers/+page.svelte`
- Create: `frontend/src/routes/primers/[slug]/+page.svelte`
- Modify: `frontend/src/lib/components/Nav.svelte` (links array), `frontend/src/routes/blindspots/+page.svelte` (Study-primer button)
- Modify: `frontend/package.json` (add `marked`, `dompurify`)

**Interfaces:**
- Consumes: Task 13's three endpoints, exact shapes as specified there.

- [ ] **Step 1: Install renderer deps**

Run: `cd frontend && npm install marked dompurify && npm install -D @types/dompurify`

- [ ] **Step 2: Nav link** — in the `links` array after `Mock Test`:

```ts
    { href: '/primers', label: 'Primers' },
```

- [ ] **Step 3: Library page `frontend/src/routes/primers/+page.svelte`**

```svelte
<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let primers = $state<Array<{ id: number; slug: string; topic: string; source: string; createdAt: string }>>([]);
  let canon = $state<string[]>([]);
  let configured = $state(true);
  let loading = $state(true);
  let error = $state('');
  let generating = $state<string | null>(null); // topic currently generating
  let customTopic = $state('');

  onMount(async () => {
    try {
      const res = await api.get('/api/primers');
      primers = res.primers;
      canon = res.canon;
      configured = res.configured;
    } catch (e: any) {
      error = e?.message ?? 'Failed to load primers';
    } finally {
      loading = false;
    }
  });

  let existingTopics = $derived(new Set(primers.map((p) => p.topic)));

  async function generate(topic: string, source?: string) {
    if (generating) return;
    generating = topic;
    error = '';
    try {
      const res = await api.post('/api/primers/generate', { topic, source });
      goto(`/primers/${res.slug}`);
    } catch (e: any) {
      error = e?.message ?? 'Generation failed';
    } finally {
      generating = null;
    }
  }
</script>

<svelte:head><title>Primers — Jeopardy! Training</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-4xl mx-auto">
    <h1 class="text-3xl font-bold text-jeopardy-blue mb-2">Primers</h1>
    <p class="text-gray-500 mb-6">Long-form study guides for the canonical Jeopardy topics. Read the primer, then drill the topic.</p>

    {#if error}<div class="px-4 py-3 mb-4 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>{/if}
    {#if !configured}<div class="px-4 py-3 mb-4 bg-amber-50 border border-amber-200 text-amber-700 rounded-lg">Generation is not configured (no API key) — existing primers are still readable.</div>{/if}

    {#if loading}
      <div class="flex justify-center py-16"><div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div></div>
    {:else}
      {#if primers.length > 0}
        <div class="bg-white rounded-xl shadow divide-y divide-gray-100 mb-8">
          {#each primers as p}
            <a href="/primers/{p.slug}" class="flex items-center justify-between p-4 hover:bg-gray-50 transition-colors group">
              <div>
                <p class="font-semibold text-gray-800">{p.topic}</p>
                <p class="text-xs text-gray-400">{p.source} · {new Date(p.createdAt).toLocaleDateString()}</p>
              </div>
              <span class="text-gray-400 group-hover:text-gray-600">&rarr;</span>
            </a>
          {/each}
        </div>
      {/if}

      <h2 class="text-sm font-semibold text-gray-600 mb-3">Generate a primer</h2>
      <div class="flex flex-wrap gap-2 mb-4">
        {#each canon.filter((t) => !existingTopics.has(t)) as topic}
          <button
            onclick={() => generate(topic, 'canon')}
            disabled={generating !== null || !configured}
            class="px-3 py-1.5 rounded-full border border-jeopardy-blue text-jeopardy-blue text-sm font-medium hover:bg-jeopardy-blue hover:text-white transition-colors disabled:opacity-50"
          >
            {generating === topic ? 'Generating… (~30s)' : `+ ${topic}`}
          </button>
        {/each}
      </div>
      <form
        class="flex gap-2"
        onsubmit={(e) => { e.preventDefault(); if (customTopic.trim()) generate(customTopic.trim()); }}
      >
        <input
          bind:value={customTopic}
          placeholder="Custom topic (e.g. 'French Revolution')"
          disabled={generating !== null || !configured}
          class="flex-1 px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
        />
        <button type="submit" disabled={generating !== null || !configured || !customTopic.trim()}
          class="px-4 py-2 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 disabled:opacity-50">
          {generating === customTopic.trim() ? 'Generating…' : 'Generate'}
        </button>
      </form>
    {/if}
  </div>
</div>
```

- [ ] **Step 4: Primer view `frontend/src/routes/primers/[slug]/+page.svelte`**

```svelte
<script lang="ts">
  import { page } from '$app/state';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let primer = $state<{ topic: string; contentMd: string; createdAt: string } | null>(null);
  let error = $state('');
  let html = $state('');

  $effect(() => {
    const slug = page.params.slug;
    api.get(`/api/primers/${slug}`)
      .then(async (p) => {
        primer = p;
        html = DOMPurify.sanitize(await marked.parse(p.contentMd));
      })
      .catch((e: any) => (error = e?.message ?? 'Not found'));
  });
</script>

<svelte:head><title>{primer?.topic ?? 'Primer'} — Jeopardy! Training</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto">
    <div class="mb-4 flex items-center justify-between">
      <a href="/primers" class="text-sm text-jeopardy-blue hover:underline">&larr; All primers</a>
      {#if primer}
        <a href="/drill?q={encodeURIComponent(primer.topic)}"
          class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800">
          Drill this topic &rarr;
        </a>
      {/if}
    </div>
    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>
    {:else if primer}
      <article class="bg-white rounded-xl shadow p-8 prose prose-slate max-w-none
        prose-headings:text-jeopardy-blue prose-table:text-sm">
        <h1>{primer.topic}</h1>
        {@html html}
      </article>
    {:else}
      <div class="flex justify-center py-16"><div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div></div>
    {/if}
  </div>
</div>
```

Note: if Tailwind's `prose` classes don't exist (no typography plugin), style the article manually with a scoped `<style>` block covering `h2, h3, table, th, td, ul, li` — check `frontend/tailwind.config`/CSS first and match the codebase's approach.

- [ ] **Step 5: Blindspots "Study primer" button** — in `frontend/src/routes/blindspots/+page.svelte`, add next to each pack's existing actions:

```svelte
<button
  onclick={() => studyPrimer(pack.theme)}
  disabled={primerLoading === pack.theme}
  class="px-3 py-1.5 rounded-lg border border-jeopardy-blue text-jeopardy-blue text-sm font-medium hover:bg-jeopardy-blue hover:text-white transition-colors disabled:opacity-50"
>
  {primerLoading === pack.theme ? 'Preparing primer…' : 'Study primer'}
</button>
```

with, in the script block:

```ts
let primerLoading = $state<string | null>(null);
async function studyPrimer(theme: string) {
  primerLoading = theme;
  try {
    const res = await api.post('/api/primers/generate', { topic: theme, source: 'blindspot' });
    goto(`/primers/${res.slug}`);
  } catch (e) {
    primerLoading = null;
  }
}
```

Match the existing page's button styling and layout — read the file first and place the button beside the pack's existing drill/action controls.

- [ ] **Step 6: Check + manual test**

Run: `cd frontend && npm run check` → 0 errors. Manual: /primers lists canon chips; generating (with key) lands on a rendered primer with headings and tables; blindspot button navigates to the same primer on second use (cached).

- [ ] **Step 7: Commit**

```bash
git add frontend/src/routes/primers frontend/src/lib/components/Nav.svelte frontend/src/routes/blindspots/+page.svelte frontend/package.json frontend/package-lock.json
git commit -m "feat(primers): /primers library, primer view, blindspot study-primer links"
```
