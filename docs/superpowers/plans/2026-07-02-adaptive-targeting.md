# Adaptive Weakness Targeting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tilt Practice's new-clue selection toward the user's measured weak categories (60% weakness-weighted / 40% random), with a Settings toggle and a dashboard "Focus areas" panel.

**Architecture:** A pure, unit-tested weight module (`backend/src/adaptive.rs`) turns per-category `(attempts, correct)` rows into a normalized weakness distribution and samples from it. `practice.rs` gains a small stats query (180-day window, all-time fallback) and a 60/40 branch in the new-clue picker; `status` exposes the same weights for the dashboard; `preferences` gains the toggle. No new dependencies.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8 runtime-checked), PostgreSQL, SvelteKit (Svelte 5 runes), Tailwind.

## Global Constraints

- **Never run migrations/tests against the shared production DB.** Migration `0003` is file-only here, applied manually on Tower.
- Tilt is exactly **60/40**: draw `r ∈ [0,1)`; `r < 0.4` → today's unconstrained pick; else weakness-weighted category first, unconstrained fallback if that category yields no clue.
- Weight math (spec §3, verbatim): `smoothed_acc = (correct + 5 × global_acc) / (attempts + 5)`, `raw = 1 − smoothed_acc`, normalized. Empty input → empty; all-raw-zero → uniform.
- Window: attempts from the last **180 days**; if that window holds **< 200** attempts, use all-time.
- Adaptive applies ONLY in Practice's new-clue picker, and is bypassed when a manual category filter is set or `users.adaptive_targeting` is false.
- The pure module is deterministic (randomness is passed in as `r: f64`); plain `#[test]` units, no DB.
- Wire names: preference key `adaptiveTargeting` (camelCase JSON) ↔ column `adaptive_targeting`; status key `adaptiveWeights: [{category, attempts, accuracy, weight}]` sorted by weight descending, `[]` when the toggle is off.
- Frontend: Svelte 5 runes + Tailwind, matching existing pages. `npm run check` 0 errors, `npm run build` succeeds. Backend: `cargo test` green, clippy clean except the 2 baseline warnings (`field email is never read`, `struct QuestionAttempt is never constructed`).

---

### Task 1: Migration 0003 + prisma parity

**Files:**
- Create: `backend/migrations/0003_adaptive_targeting.sql`
- Modify: `prisma/schema.prisma` (users model)

**Interfaces:**
- Produces: column `users.adaptive_targeting BOOLEAN NOT NULL DEFAULT true`.

- [ ] **Step 1: Write the migration**

Create `backend/migrations/0003_adaptive_targeting.sql`:

```sql
-- Toggle for adaptive weakness targeting in Practice's new-clue picker.
ALTER TABLE users ADD COLUMN IF NOT EXISTS adaptive_targeting BOOLEAN NOT NULL DEFAULT true;
```

- [ ] **Step 2: Mirror in prisma (documentation parity)**

In `prisma/schema.prisma`, inside `model users { ... }`, directly after the `timezone` field, add:

```prisma
  adaptive_targeting    Boolean                  @default(true)
```

Do NOT run `psql` or connect to any database (no scratch DB; production is off-limits — the migration is applied manually on Tower at deploy time).

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/0003_adaptive_targeting.sql prisma/schema.prisma
git commit -m "feat(adaptive): add users.adaptive_targeting migration"
```

---

### Task 2: Pure weight module `adaptive.rs` (TDD)

**Files:**
- Create: `backend/src/adaptive.rs`
- Modify: `backend/src/main.rs` (add `mod adaptive;` after `mod srs;`)
- Test: inline `#[cfg(test)]` in `backend/src/adaptive.rs`

**Interfaces:**
- Produces:
  - `pub struct CategoryStat { pub category: String, pub attempts: i64, pub correct: i64 }`
  - `pub struct CategoryWeight { pub category: String, pub attempts: i64, pub accuracy: f64, pub weight: f64 }`
  - `pub fn compute_weights(stats: &[CategoryStat]) -> Vec<CategoryWeight>` (sorted weight desc)
  - `pub fn sample_category(weights: &[CategoryWeight], r: f64) -> Option<&str>`

- [ ] **Step 1: Write the failing tests**

Create `backend/src/adaptive.rs` containing only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn s(cat: &str, attempts: i64, correct: i64) -> CategoryStat {
        CategoryStat { category: cat.to_string(), attempts, correct }
    }

    #[test]
    fn weights_normalize_and_favor_weak_categories() {
        let stats = vec![s("Music", 100, 55), s("Math", 100, 82), s("Science", 100, 74)];
        let w = compute_weights(&stats);
        let sum: f64 = w.iter().map(|x| x.weight).sum();
        assert!((sum - 1.0).abs() < 1e-9);
        // Sorted descending by weight: weakest (Music) first, strongest (Math) last.
        assert_eq!(w[0].category, "Music");
        assert_eq!(w[2].category, "Math");
        assert!(w[0].weight > w[2].weight);
    }

    #[test]
    fn smoothing_keeps_tiny_samples_from_dominating() {
        // One miss on a single attempt must not outrank a genuinely weak,
        // well-sampled category (50% over 100 attempts vs 0% over 1).
        let stats = vec![s("Tiny", 1, 0), s("BigWeak", 100, 50), s("Strong", 100, 90)];
        let w = compute_weights(&stats);
        let get = |c: &str| w.iter().find(|x| x.category == c).unwrap().weight;
        assert!(get("BigWeak") > get("Tiny"));
        assert!(get("Tiny") > get("Strong"));
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert!(compute_weights(&[]).is_empty());
    }

    #[test]
    fn all_perfect_falls_back_to_uniform() {
        // global_acc = 1.0 → every raw weight is 0 → uniform distribution.
        let stats = vec![s("A", 10, 10), s("B", 10, 10)];
        let w = compute_weights(&stats);
        assert!((w[0].weight - 0.5).abs() < 1e-9);
        assert!((w[1].weight - 0.5).abs() < 1e-9);
    }

    #[test]
    fn accuracy_is_observed_percent() {
        let stats = vec![s("A", 100, 55), s("Zero", 0, 0)];
        let w = compute_weights(&stats);
        let get = |c: &str| w.iter().find(|x| x.category == c).unwrap();
        assert!((get("A").accuracy - 55.0).abs() < 1e-9);
        assert!((get("Zero").accuracy - 0.0).abs() < 1e-9);
    }

    #[test]
    fn sampling_walks_the_cumulative_distribution() {
        let stats = vec![s("Weak", 100, 20), s("Strong", 100, 90)];
        let w = compute_weights(&stats);
        // w[0] = Weak with the larger weight.
        assert_eq!(sample_category(&w, 0.0), Some("Weak"));
        assert_eq!(sample_category(&w, 0.999_999), Some("Strong"));
        assert_eq!(sample_category(&w, w[0].weight + 1e-9), Some("Strong"));
        assert_eq!(sample_category(&[], 0.5), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cd backend && cargo test adaptive::`
Expected: FAIL — `cannot find type CategoryStat`, etc. (Also add `mod adaptive;` to `backend/src/main.rs` after `mod srs;` now, or the module won't compile at all.)

- [ ] **Step 3: Implement**

Prepend to `backend/src/adaptive.rs` (above the test module):

```rust
//! Adaptive weakness targeting: turn per-category attempt history into a
//! normalized "weakness" distribution over classifier categories.
//!
//! Pure and deterministic — no DB, no clock, and randomness is passed IN
//! (`sample_category` takes `r`), so everything here is unit-testable.

const PRIOR_PSEUDO_COUNT: f64 = 5.0;

#[derive(Debug, Clone)]
pub struct CategoryStat {
    pub category: String,
    pub attempts: i64,
    pub correct: i64,
}

#[derive(Debug, Clone)]
pub struct CategoryWeight {
    pub category: String,
    pub attempts: i64,
    pub accuracy: f64, // observed percent, 0 when unattempted
    pub weight: f64,   // normalized selection probability
}

/// Smoothed miss-rate weights, sorted by weight descending.
/// smoothed_acc = (correct + 5·global_acc) / (attempts + 5); raw = 1 − smoothed.
/// Empty input → empty. All-raw-zero (perfect everywhere) → uniform.
pub fn compute_weights(stats: &[CategoryStat]) -> Vec<CategoryWeight> {
    if stats.is_empty() {
        return vec![];
    }
    let total_attempts: i64 = stats.iter().map(|s| s.attempts).sum();
    let total_correct: i64 = stats.iter().map(|s| s.correct).sum();
    let global_acc = if total_attempts > 0 {
        total_correct as f64 / total_attempts as f64
    } else {
        0.5
    };

    let raw: Vec<f64> = stats
        .iter()
        .map(|s| {
            let smoothed = (s.correct as f64 + PRIOR_PSEUDO_COUNT * global_acc)
                / (s.attempts as f64 + PRIOR_PSEUDO_COUNT);
            (1.0 - smoothed).max(0.0)
        })
        .collect();

    let sum: f64 = raw.iter().sum();
    let n = stats.len() as f64;

    let mut out: Vec<CategoryWeight> = stats
        .iter()
        .zip(raw.iter())
        .map(|(s, &r)| CategoryWeight {
            category: s.category.clone(),
            attempts: s.attempts,
            accuracy: if s.attempts > 0 {
                s.correct as f64 / s.attempts as f64 * 100.0
            } else {
                0.0
            },
            weight: if sum > 0.0 { r / sum } else { 1.0 / n },
        })
        .collect();

    out.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Walk the cumulative distribution with r ∈ [0,1). Returns the last entry for
/// float dust at r ≈ 1.0; None only for an empty slice.
pub fn sample_category(weights: &[CategoryWeight], r: f64) -> Option<&str> {
    let mut acc = 0.0;
    for w in weights {
        acc += w.weight;
        if r < acc {
            return Some(&w.category);
        }
    }
    weights.last().map(|w| w.category.as_str())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && cargo test adaptive::`
Expected: PASS — 6 tests.

- [ ] **Step 5: Commit**

```bash
git add backend/src/adaptive.rs backend/src/main.rs
git commit -m "feat(adaptive): pure smoothed-miss-rate weight module with unit tests"
```

---

### Task 3: Backend wiring — picker 60/40, status weights, preference

**Files:**
- Modify: `backend/src/routes/practice.rs` (next-prefs query, `pick_new_clue`, `status`)
- Modify: `backend/src/routes/preferences.rs` (GET/PUT `adaptiveTargeting`)

**Interfaces:**
- Consumes: `crate::adaptive::{compute_weights, sample_category, CategoryStat}` (Task 2), `users.adaptive_targeting` (Task 1).
- Produces: `GET /api/practice/status` → adds `adaptiveWeights: [{category, attempts, accuracy, weight}]` ( `[]` when off); `GET/PUT /api/preferences` ↔ `adaptiveTargeting: bool`.

- [ ] **Step 1: Add imports and the stats query to `practice.rs`**

At the top of `backend/src/routes/practice.rs`, after `use crate::srs::{...};`, add:

```rust
use crate::adaptive::{compute_weights, sample_category, CategoryStat};
```

Then add this helper near `pick_new_clue` (window with all-time fallback):

```rust
/// Per-category (attempts, correct) for the adaptive window: last 180 days,
/// falling back to all-time when the window holds < 200 attempts.
async fn adaptive_category_stats(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<Vec<CategoryStat>, AppError> {
    const WINDOWED_SQL: &str = "SELECT jq.classifier_category, COUNT(*)::bigint, \
             SUM((qa.correct)::int)::bigint \
         FROM question_attempts qa \
         JOIN jeopardy_questions jq ON jq.id = qa.question_id \
         WHERE qa.user_id = $1 AND jq.classifier_category IS NOT NULL \
           AND qa.answered_at >= now() - interval '180 days' \
         GROUP BY jq.classifier_category";
    const ALL_TIME_SQL: &str = "SELECT jq.classifier_category, COUNT(*)::bigint, \
             SUM((qa.correct)::int)::bigint \
         FROM question_attempts qa \
         JOIN jeopardy_questions jq ON jq.id = qa.question_id \
         WHERE qa.user_id = $1 AND jq.classifier_category IS NOT NULL \
         GROUP BY jq.classifier_category";

    let windowed: Vec<(String, i64, i64)> = sqlx::query_as(WINDOWED_SQL)
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;
    let windowed_total: i64 = windowed.iter().map(|r| r.1).sum();

    let rows = if windowed_total < 200 {
        sqlx::query_as(ALL_TIME_SQL)
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?
    } else {
        windowed
    };

    Ok(rows
        .into_iter()
        .map(|(category, attempts, correct)| CategoryStat { category, attempts, correct })
        .collect())
}
```

- [ ] **Step 2: Split the picker and add the 60/40 branch**

Rename the existing `pick_new_clue` to `pick_with_filters` and change how it gets the category: replace its first line

```rust
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
```

with a new parameter. The new signature and header:

```rust
async fn pick_with_filters(
    state: &Arc<AppState>,
    user_id: i32,
    category: &str, // "all" or a classifier category
    params: &HashMap<String, String>,
) -> Result<Option<ClueRow>, AppError> {
    let game_types_str = params.get("gameTypes").map(|s| s.as_str()).unwrap_or("");
```

(everything below — game_types parsing, conditions, count, offset, select — stays byte-for-byte, since it already works off the `category` variable and `use_category = category != "all"`).

Then add the new strategy wrapper with the old name, so `next()` needs only a one-line change:

```rust
/// Strategy wrapper: 60% of pulls (when no manual filter and the user's toggle
/// is on) sample a category by weakness weight first; 40% — and all filtered or
/// toggled-off pulls — behave exactly as before. A weighted pick that finds no
/// eligible clue falls back to unconstrained.
async fn pick_new_clue(
    state: &Arc<AppState>,
    user_id: i32,
    adaptive: bool,
    params: &HashMap<String, String>,
) -> Result<Option<ClueRow>, AppError> {
    let manual_category = params.get("category").map(|s| s.as_str()).unwrap_or("all");

    if manual_category == "all" && adaptive {
        use rand::Rng;
        let roll: f64 = rand::rng().random();
        if roll >= 0.4 {
            let stats = adaptive_category_stats(state, user_id).await?;
            let weights = compute_weights(&stats);
            let r: f64 = rand::rng().random();
            if let Some(cat) = sample_category(&weights, r) {
                let cat = cat.to_string();
                if let Some(row) = pick_with_filters(state, user_id, &cat, params).await? {
                    return Ok(Some(row));
                }
                // Weighted category exhausted — fall through to unconstrained.
            }
        }
    }

    pick_with_filters(state, user_id, manual_category, params).await
}
```

- [ ] **Step 3: Thread the toggle through `next()`**

In `next()`, change the prefs query (currently `SELECT new_cards_per_day, timezone FROM users WHERE id = $1` into `(i32, Option<String>)`) to:

```rust
    let (new_per_day, tz, adaptive): (i32, Option<String>, bool) =
        sqlx::query_as("SELECT new_cards_per_day, timezone, adaptive_targeting FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;
```

and change the call site `pick_new_clue(&state, user_id, &params)` to `pick_new_clue(&state, user_id, adaptive, &params)`.

- [ ] **Step 4: Expose weights in `status`**

In `status()`, extend its prefs query the same way (`(i32, Option<String>, bool)` with `adaptive_targeting`), then before the final `Ok(Json(...)))` add:

```rust
    let adaptive_weights: Vec<Value> = if adaptive {
        let stats = adaptive_category_stats(&state, user_id).await?;
        compute_weights(&stats)
            .into_iter()
            .map(|w| {
                json!({
                    "category": w.category,
                    "attempts": w.attempts,
                    "accuracy": w.accuracy,
                    "weight": w.weight,
                })
            })
            .collect()
    } else {
        vec![]
    };
```

and add `"adaptiveWeights": adaptive_weights,` to the returned JSON object.

- [ ] **Step 5: Preference GET/PUT**

In `backend/src/routes/preferences.rs`:

GET — change the query/tuple to include the flag and return it:

```rust
    let row: (Option<String>, i32, Option<String>, bool) = sqlx::query_as(
        "SELECT game_type_filters, new_cards_per_day, timezone, adaptive_targeting FROM users WHERE id = $1",
    )
```

and add `"adaptiveTargeting": row.3,` to the returned JSON.

PUT — add to `UpdatePreferencesBody`:

```rust
    pub adaptive_targeting: Option<bool>,
```

and after the timezone write:

```rust
    if let Some(adaptive) = body.adaptive_targeting {
        sqlx::query("UPDATE users SET adaptive_targeting = $1 WHERE id = $2")
            .bind(adaptive)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }
```

- [ ] **Step 6: Build, test, lint**

Run: `cd backend && cargo test 2>&1 | grep "test result" && cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | tail -3`
Expected: 25 tests pass (19 existing + 6 adaptive); clean build; only the 2 baseline warnings.

Note: live curl verification (weighted distribution visible over many pulls, `adaptiveWeights` shape) is deferred — no scratch DB; production is off-limits. Record in the report.

- [ ] **Step 7: Commit**

```bash
git add backend/src/routes/practice.rs backend/src/routes/preferences.rs
git commit -m "feat(adaptive): 60/40 weighted new-clue picker, status weights, preference"
```

---

### Task 4: Frontend — Settings toggle + dashboard Focus areas

**Files:**
- Modify: `frontend/src/routes/settings/+page.svelte` (Practice card)
- Modify: `frontend/src/routes/dashboard/+page.svelte` (SRS summary card)

**Interfaces:**
- Consumes: `GET/PUT /api/preferences` `adaptiveTargeting` and `GET /api/practice/status` `adaptiveWeights` (Task 3).

- [ ] **Step 1: Settings toggle**

In `frontend/src/routes/settings/+page.svelte`:

Add state next to the other SRS prefs:

```ts
  let adaptiveTargeting = $state(true);
```

In `onMount`, after `timezone = prefs?.timezone ?? '';` add:

```ts
      adaptiveTargeting = prefs?.adaptiveTargeting ?? true;
```

In `saveSrsPrefs()`'s PUT body, add `adaptiveTargeting,` after `timezone,`.

In the Practice card markup, after the Timezone `</label>` and before the `{#if srsSaved}` block, add:

```svelte
        <label class="flex items-center gap-2 text-sm text-gray-700 cursor-pointer">
          <input
            type="checkbox"
            bind:checked={adaptiveTargeting}
            onchange={saveSrsPrefs}
            class="w-4 h-4 rounded border-gray-300 text-jeopardy-blue focus:ring-jeopardy-blue"
          />
          <span>
            <span class="font-semibold">Adaptive clue selection</span>
            <span class="text-gray-500">— favor your weaker categories</span>
          </span>
        </label>
```

- [ ] **Step 2: Dashboard Focus areas panel**

In `frontend/src/routes/dashboard/+page.svelte`:

Extend the `srs` state type with the new field:

```ts
  let srs = $state<{
    dueCount: number;
    newRemaining: number;
    reviewedToday: number;
    forecast: Array<{ date: string; count: number }>;
    adaptiveWeights?: Array<{ category: string; attempts: number; accuracy: number; weight: number }>;
  } | null>(null);
```

Inside the SRS summary card, after the forecast chart block (`{#if forecastChartData} ... {/if}`) and before the card's closing `</div>`, add:

```svelte
        {#if srs.adaptiveWeights && srs.adaptiveWeights.length > 0}
          <div class="mt-5 pt-4 border-t border-gray-100">
            <h2 class="text-sm font-semibold text-gray-600 mb-1">Focus areas</h2>
            <p class="text-xs text-gray-400 mb-3">New clues favor your weaker categories.</p>
            <div class="flex flex-col gap-1.5">
              {#each srs.adaptiveWeights as w, i (w.category)}
                <div class="flex items-center gap-3 text-sm">
                  <span class="w-52 shrink-0 truncate text-gray-700">
                    {w.category}
                    {#if i < 3}
                      <span class="ml-1 px-1.5 py-0.5 rounded-full bg-jeopardy-gold/20 text-jeopardy-blue text-[10px] font-bold uppercase tracking-wide">Targeted</span>
                    {/if}
                  </span>
                  <div class="flex-1 h-2 bg-gray-100 rounded-full overflow-hidden">
                    <div class="h-full bg-jeopardy-blue rounded-full" style="width: {Math.round(w.weight * 100)}%"></div>
                  </div>
                  <span class="w-28 shrink-0 text-right text-xs text-gray-500">
                    {Math.round(w.accuracy)}% · {w.attempts} tries
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
```

(The weight bars are a single-hue magnitude encoding on the existing chart blue — identity is carried by the row label, not color, so no legend is needed.)

- [ ] **Step 3: Type-check, build**

Run: `cd frontend && npm run check 2>&1 | tail -2 && npm run build 2>&1 | tail -2`
Expected: 0 errors; build succeeds. Live smoke test deferred (no backend/DB here).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/routes/settings/+page.svelte frontend/src/routes/dashboard/+page.svelte frontend/build
git commit -m "feat(adaptive): settings toggle + dashboard Focus areas panel"
```

---

## Notes for the implementer

- **Deploy order:** apply `backend/migrations/0003_adaptive_targeting.sql` on Tower BEFORE the new container ships (the prefs/status queries reference the column). Single instant ALTER; no table rewrite.
- `Math.round` inside the Svelte template requires no import (global). The `bg-jeopardy-gold/20` opacity variant works with the existing Tailwind config since `jeopardy-gold` is a defined color.
- The two `rand::rng().random()` draws in `pick_new_clue` intentionally stay OUTSIDE the pure module — `compute_weights`/`sample_category` receive everything they need as arguments, which is what keeps them unit-testable.
