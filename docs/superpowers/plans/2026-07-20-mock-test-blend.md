# Mock Test Blend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make mock test sampling match the real Anytime Test: fixed category weights, canon-weighted picks in academic categories, recency-weighted picks in pop-culture categories.

**Architecture:** A new migration adds a precomputed `answer_freq` column (corpus-wide count of each clue's normalized response). A new pure-logic module `backend/src/blend.rs` holds the fixed weight table and per-category sampling-kind rules. `create()` in `backend/src/routes/mock_test.rs` is rewired to apportion seats over the fixed weights and draw each category with a weighted `ORDER BY -ln(random())/weight` sample instead of uniform `ORDER BY random()`.

**Tech Stack:** Rust (axum + sqlx), PostgreSQL 15, migrations applied manually via `scripts/apply-migration.sh`, deploy via push → GitHub Actions (`build.yml`) → ghcr.io → Watchtower on Tower.

**Spec:** `docs/superpowers/specs/2026-07-20-mock-test-blend-design.md`

## Global Constraints

- Category names are exact strings from `jeopardy_questions.classifier_category`, e.g. `Literature & Language`, `Film, TV & Pop Culture` (comma included), `Music & Performing Arts`, `Sports & Games`.
- Column semantics: `jeopardy_questions.question` = the accepted response, `jeopardy_questions.answer` = the clue text. `answer_freq` counts responses (`question` column).
- Recency half-life 6 years: decay constant `ln(2)/6 = 0.11552` per year; one year = `31557600` seconds.
- Unchanged: `TEST_SIZE = 50`, `PASS_LINE = 35`, MIDBAND filter, unseen exclusions, shortfall top-up, shuffle, session anchoring, resume flow.
- Production DB is the only DB; it lives in the `postgresql15` container on Tower. `DATABASE_URL` in repo-root `.env` points at it (used by `scripts/apply-migration.sh`).
- Odd Music seat counts: canon side gets the extra seat.

---

### Task 1: Migration 0008 — `answer_freq` column

**Files:**
- Create: `backend/migrations/0008_answer_freq.sql`

**Interfaces:**
- Produces: `jeopardy_questions.answer_freq INTEGER NOT NULL DEFAULT 1`, backfilled. Task 3's SQL relies on this column existing in prod before deploy.

- [ ] **Step 1: Write the migration**

```sql
-- 0008: answer_freq — corpus-wide count of each clue's normalized response.
-- Canonicity proxy for mock test sampling (spec 2026-07-20-mock-test-blend-design).
ALTER TABLE jeopardy_questions
  ADD COLUMN IF NOT EXISTS answer_freq INTEGER NOT NULL DEFAULT 1;

WITH freq AS (
  SELECT lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) AS norm,
         count(*) AS n
  FROM jeopardy_questions
  WHERE archived = false AND question IS NOT NULL
  GROUP BY 1
)
UPDATE jeopardy_questions jq
SET answer_freq = f.n
FROM freq f
WHERE jq.question IS NOT NULL
  AND lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = f.norm;
```

- [ ] **Step 2: Apply to the database**

Run from repo root: `scripts/apply-migration.sh backend/migrations/0008_answer_freq.sql`
Expected: `ALTER TABLE`, then `UPDATE` with a row count around 530000. (Old running backend code is unaffected — it never references the column, and the default covers new rows.)

- [ ] **Step 3: Verify the backfill**

Run: `tower-ssh "docker exec postgresql15 psql -U ebertx -d jeopardy -Atc \"SELECT max(answer_freq), count(*) FILTER (WHERE answer_freq >= 5) FROM jeopardy_questions\""`
Expected: max in the hundreds (top canon answers appear 100+ times); the `>= 5` count around 380000.

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0008_answer_freq.sql
git commit -m "feat(db): answer_freq canonicity column for mock test sampling"
```

---

### Task 2: `blend` module — weights and sampling kinds (pure logic, TDD)

**Files:**
- Create: `backend/src/blend.rs`
- Modify: `backend/src/main.rs` (add `mod blend;` next to the existing `mod adaptive;` line)

**Interfaces:**
- Consumes: `crate::routes::mock_test::apportion` (existing, `pub fn apportion(dist: &[(String, i64)], seats: i64) -> Vec<(String, i64)>`) — in tests only.
- Produces (used by Task 3):
  - `pub const TARGET_WEIGHTS: [(&str, i64); 13]`
  - `pub enum SamplingKind { Canon, Recency, Split }`
  - `pub fn sampling_kind(category: &str) -> SamplingKind`
  - `pub fn split_seats(total: i64) -> (i64, i64)` — returns `(canon, recency)`, canon gets the odd seat
  - `pub fn target_weights(available: &[String]) -> Vec<(String, i64)>`

- [ ] **Step 1: Write the failing tests**

Create `backend/src/blend.rs` with the tests only (module body empty apart from tests):

```rust
//! Mock test blend: fixed category weights and per-category sampling kinds
//! matching the real Anytime Test composition
//! (docs/superpowers/specs/2026-07-20-mock-test-blend-design.md).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::mock_test::apportion;

    #[test]
    fn weights_sum_to_100() {
        assert_eq!(TARGET_WEIGHTS.iter().map(|(_, w)| w).sum::<i64>(), 100);
    }

    #[test]
    fn full_apportionment_yields_50_seats_with_lit_at_10() {
        let dist: Vec<(String, i64)> = TARGET_WEIGHTS
            .iter()
            .map(|(c, w)| (c.to_string(), *w))
            .collect();
        let q = apportion(&dist, 50);
        assert_eq!(q.iter().map(|(_, s)| s).sum::<i64>(), 50);
        let lit = q.iter().find(|(c, _)| c == "Literature & Language").unwrap().1;
        assert_eq!(lit, 10); // 20% of 50
    }

    #[test]
    fn split_seats_gives_canon_the_odd_seat() {
        assert_eq!(split_seats(0), (0, 0));
        assert_eq!(split_seats(1), (1, 0));
        assert_eq!(split_seats(3), (2, 1));
        assert_eq!(split_seats(4), (2, 2));
    }

    #[test]
    fn sampling_kinds_match_spec() {
        assert_eq!(sampling_kind("Film, TV & Pop Culture"), SamplingKind::Recency);
        assert_eq!(sampling_kind("Sports & Games"), SamplingKind::Recency);
        assert_eq!(sampling_kind("Music & Performing Arts"), SamplingKind::Split);
        assert_eq!(sampling_kind("Literature & Language"), SamplingKind::Canon);
        assert_eq!(sampling_kind("History & Politics"), SamplingKind::Canon);
    }

    #[test]
    fn target_weights_filters_to_available_categories() {
        let available = vec![
            "Literature & Language".to_string(),
            "Sports & Games".to_string(),
        ];
        let w = target_weights(&available);
        assert_eq!(w.len(), 2);
        assert!(w.iter().any(|(c, n)| c == "Literature & Language" && *n == 20));
        assert!(w.iter().any(|(c, n)| c == "Sports & Games" && *n == 2));
    }
}
```

Add `mod blend;` to `backend/src/main.rs` next to the existing `mod adaptive;` line.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test blend`
Expected: compile error — `TARGET_WEIGHTS`, `SamplingKind`, `sampling_kind`, `split_seats`, `target_weights` not found.

- [ ] **Step 3: Write the implementation**

Add above the `#[cfg(test)]` block in `backend/src/blend.rs`:

```rust
/// Real-test composition, from the tally of three archived Anytime Tests
/// (Jan 28–30, 2020). Weights sum to 100.
pub const TARGET_WEIGHTS: [(&str, i64); 13] = [
    ("Literature & Language", 20),
    ("Geography & Exploration", 14),
    ("History & Politics", 13),
    ("Science & Nature", 11),
    ("Film, TV & Pop Culture", 10),
    ("Philosophy, Religion & Society", 6),
    ("Music & Performing Arts", 6),
    ("Miscellaneous", 6),
    ("Technology & Engineering", 4),
    ("Mathematics & Logic", 4),
    ("Art & Culture", 2),
    ("Business & Economics", 2),
    ("Sports & Games", 2),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingKind {
    /// Weight ∝ ln(1 + answer_freq): favors canonical, frequently-recurring answers.
    Canon,
    /// Weight ∝ 6-year-half-life decay on air_date: favors current material.
    Recency,
    /// Music: seats split canon/recency (composer slot + current-artist slot).
    Split,
}

pub fn sampling_kind(category: &str) -> SamplingKind {
    match category {
        "Film, TV & Pop Culture" | "Sports & Games" => SamplingKind::Recency,
        "Music & Performing Arts" => SamplingKind::Split,
        _ => SamplingKind::Canon,
    }
}

/// (canon_seats, recency_seats); canon gets the odd seat.
pub fn split_seats(total: i64) -> (i64, i64) {
    let recency = total / 2;
    (total - recency, recency)
}

/// Fixed weights restricted to categories that have an eligible pool.
pub fn target_weights(available: &[String]) -> Vec<(String, i64)> {
    TARGET_WEIGHTS
        .iter()
        .filter(|(c, _)| available.iter().any(|a| a == c))
        .map(|(c, w)| (c.to_string(), *w))
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && cargo test blend`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add backend/src/blend.rs backend/src/main.rs
git commit -m "feat(mock-test): blend module with fixed weights and sampling kinds"
```

---

### Task 3: Rewire `create()` to fixed weights + weighted draws

**Files:**
- Modify: `backend/src/routes/mock_test.rs` (the `create()` function, currently lines 60–142, plus new consts and a helper)

**Interfaces:**
- Consumes: `crate::blend::{sampling_kind, split_seats, target_weights, SamplingKind}` (Task 2); `jeopardy_questions.answer_freq` (Task 1); existing `apportion`, `MIDBAND`, `TEST_SIZE`.
- Produces: no new public interface; `create()` behavior change only.

- [ ] **Step 1: Add order-expression consts and the draw helper**

In `backend/src/routes/mock_test.rs`, add below the `MIDBAND` const:

```rust
// Weighted sampling via the exponential race: the row minimizing -ln(u)/w is a
// draw with probability proportional to w. answer_freq >= 1 keeps the divisor
// positive; air_date is NOT NULL corpus-wide.
const CANON_ORDER: &str = "-ln(random()) / ln(1 + jq.answer_freq)";
const RECENCY_ORDER: &str =
    "-ln(random()) * exp(0.11552 * EXTRACT(EPOCH FROM (now() - jq.air_date)) / 31557600.0)";

async fn draw_category(
    state: &Arc<AppState>,
    user_id: i32,
    category: &str,
    seats: i64,
    order_expr: &str,
    exclude: &[i32],
) -> Result<Vec<i32>, AppError> {
    if seats <= 0 {
        return Ok(vec![]);
    }
    let sql = format!(
        "SELECT jq.id FROM jeopardy_questions jq
         WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
           AND jq.classifier_category = $2 AND {MIDBAND}
           AND jq.id <> ALL($4)
           AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
           AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
         ORDER BY {order_expr} LIMIT $3"
    );
    let picked: Vec<(i32,)> = sqlx::query_as(&sql)
        .bind(user_id)
        .bind(category)
        .bind(seats)
        .bind(exclude.to_vec())
        .fetch_all(&state.pool)
        .await?;
    Ok(picked.into_iter().map(|(i,)| i).collect())
}
```

- [ ] **Step 2: Replace the apportionment and selection loop in `create()`**

Replace the block from `let quotas = apportion(&dist, TEST_SIZE);` through the end of the per-category `for` loop (currently the `sel_sql` loop) with:

```rust
    let available: Vec<String> = dist.iter().map(|(c, _)| c.clone()).collect();
    let weights = crate::blend::target_weights(&available);
    let quotas = apportion(&weights, TEST_SIZE);
    let mut ids: Vec<i32> = Vec::with_capacity(TEST_SIZE as usize);
    for (cat, seats) in &quotas {
        if *seats == 0 {
            continue;
        }
        match crate::blend::sampling_kind(cat) {
            crate::blend::SamplingKind::Canon => {
                let picked = draw_category(&state, user_id, cat, *seats, CANON_ORDER, &ids).await?;
                ids.extend(picked);
            }
            crate::blend::SamplingKind::Recency => {
                let picked = draw_category(&state, user_id, cat, *seats, RECENCY_ORDER, &ids).await?;
                ids.extend(picked);
            }
            crate::blend::SamplingKind::Split => {
                let (canon_seats, recency_seats) = crate::blend::split_seats(*seats);
                let picked = draw_category(&state, user_id, cat, canon_seats, CANON_ORDER, &ids).await?;
                ids.extend(picked);
                let picked = draw_category(&state, user_id, cat, recency_seats, RECENCY_ORDER, &ids).await?;
                ids.extend(picked);
            }
        }
    }
```

Leave the `dist` query, the `< TEST_SIZE` eligibility check, the shortfall top-up block, the shuffle, and everything after untouched.

- [ ] **Step 3: Build and run all tests**

Run: `cd backend && cargo test`
Expected: all tests pass (including the existing `apportion` tests and Task 2's blend tests). Fix any compile errors before proceeding.

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/mock_test.rs
git commit -m "feat(mock-test): fixed real-test category weights, canon + recency weighted draws"
```

---

### Task 4: SQL verification of the weighted draws

**Files:**
- Create: `scripts/verify-mock-blend.sql`

**Interfaces:**
- Consumes: `answer_freq` (Task 1); the same ORDER BY expressions as Task 3 (copy the SQL text — this script exists to validate that math against real data).

- [ ] **Step 1: Write the verification script**

```sql
-- Sanity checks for mock test weighted sampling (spec 2026-07-20).
-- Read-only. Run: tower-ssh "docker exec -i postgresql15 psql -U ebertx -d jeopardy" < scripts/verify-mock-blend.sql

-- 1. Canon draw: mean answer_freq of 200 weighted Literature picks vs pool mean.
WITH pool AS (
  SELECT jq.answer_freq FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Literature & Language'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
), draw AS (
  SELECT jq.answer_freq FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Literature & Language'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
  ORDER BY -ln(random()) / ln(1 + jq.answer_freq) LIMIT 200
)
SELECT 'canon: pool avg freq' AS metric, round(avg(answer_freq), 1) AS value FROM pool
UNION ALL
SELECT 'canon: draw avg freq', round(avg(answer_freq), 1) FROM draw;

-- 2. Recency draw: median air_date of 200 weighted Film/TV picks vs pool median.
WITH pool AS (
  SELECT jq.air_date FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Film, TV & Pop Culture'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
), draw AS (
  SELECT jq.air_date FROM jeopardy_questions jq
  WHERE jq.archived = false AND jq.classifier_category = 'Film, TV & Pop Culture'
    AND ((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000)
      OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))
  ORDER BY -ln(random()) * exp(0.11552 * EXTRACT(EPOCH FROM (now() - jq.air_date)) / 31557600.0) LIMIT 200
)
SELECT 'recency: pool median air_date' AS metric, percentile_cont(0.5) WITHIN GROUP (ORDER BY air_date)::text AS value FROM pool
UNION ALL
SELECT 'recency: draw median air_date', percentile_cont(0.5) WITHIN GROUP (ORDER BY air_date)::text FROM draw;
```

- [ ] **Step 2: Run it**

Run: `tower-ssh "docker exec -i postgresql15 psql -U ebertx -d jeopardy" < scripts/verify-mock-blend.sql`
Expected: canon draw avg freq at least 3× the pool avg; recency draw median air_date 2018 or later while the pool median is ~2005–2010. If either fails, stop — the ORDER BY expression in Task 3 is wrong (recheck sign and constant).

- [ ] **Step 3: Commit**

```bash
git add scripts/verify-mock-blend.sql
git commit -m "test(mock-test): SQL sanity checks for canon/recency weighted draws"
```

---

### Task 5: Deploy and verify end-to-end

**Files:** none (operational task)

**Interfaces:**
- Consumes: migration already applied in prod (Task 1); image build via `.github/workflows/build.yml`; Watchtower on Tower.

- [ ] **Step 1: Push**

```bash
git push origin main
```

- [ ] **Step 2: Wait for the image build**

Run: `gh run watch --exit-status` (or `gh run list --limit 1` until the `build.yml` run for the pushed commit shows `completed success`).

- [ ] **Step 3: Deploy the new image**

Watchtower pulls automatically on its next cycle. To deploy immediately:
`tower-ssh "docker pull ghcr.io/ebertx/jeopardy-training-app:latest && docker restart jeopardy-server"`
Then confirm health: `tower-ssh "docker ps --filter name=jeopardy-server --format '{{.Status}}'"` → expect `Up ... (healthy)` within ~a minute.

- [ ] **Step 4: End-to-end blend check**

Have the user start a fresh mock test in the app (user id 1), then inspect its composition:

```bash
tower-ssh "docker exec postgresql15 psql -U ebertx -d jeopardy -c \"
SELECT jq.classifier_category, count(*),
       round(avg(jq.answer_freq),1) AS avg_freq,
       max(jq.air_date) FILTER (WHERE jq.classifier_category = 'Film, TV & Pop Culture') AS newest_film
FROM mock_tests mt, unnest(mt.question_ids) qid
JOIN jeopardy_questions jq ON jq.id = qid
WHERE mt.id = (SELECT max(id) FROM mock_tests)
GROUP BY 1 ORDER BY 2 DESC\""
```

Expected: Literature & Language ~10, Geography ~7, History ~6–7, Science ~5–6, Film/TV ~5, Sports/Business/Art 1 each; Film/TV picks mostly recent air dates.

- [ ] **Step 5: Update Tower changelog**

Append to `/boot/config/custom/changelog.md` on Tower per the tower skill convention: date, "jeopardy: mock test blend redesign deployed (migration 0008 + weighted sampling)".
