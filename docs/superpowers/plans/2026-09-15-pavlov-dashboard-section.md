# Pavlov Dashboard Section Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the dashboard into a Pavlov section (first) and a Standard Practice section, giving Pavlov cold/review accuracy, a 30-day trend, category breakdown, deck composition with deltas, a due forecast, and a deck-progress card against a year-end target.

**Architecture:** A new `pavlov_reviews` log table is written on every Pavlov grade (the only way to get accuracy over time — card rows hold only current SRS state). A new `GET /api/pavlov/stats` endpoint returns the same JSON shape as `/api/stats` plus `deck` and `progress` blocks; progress math is a pure, unit-tested function. The dashboard page is rebuilt around a shared `StatsSection` Svelte component rendered twice, with section-specific cards passed in as snippets.

**Tech Stack:** Rust (Axum 0.8, sqlx 0.8 runtime queries, chrono + chrono-tz), PostgreSQL 15 on Tower, SvelteKit + Svelte 5 (runes, snippets), Tailwind 4, Chart.js via `StatsChart`.

**Spec:** `docs/superpowers/specs/2026-09-15-pavlov-dashboard-section-design.md`

## Global Constraints

- Schema source of truth is `backend/migrations/*.sql`. Migration file is `0013_pavlov_reviews.sql`. The container does NOT run migrations; apply manually with `scripts/apply-migration.sh` before pushing code that needs it (Task 8).
- The root Next.js `app/`, `prisma/`, and root `package.json` are dead leftovers. Never touch them. Backend is `backend/`, frontend is `frontend/`.
- Ratings on the wire are exactly `wrong | got_it | too_easy`. "Correct" = rating is not `wrong`.
- Deck buckets are mutually exclusive: banished (`suspended`), struggling (not suspended, `lapses >= 4`), learning (not suspended, lapses < 4, `state <> 'review'`), maturing (review, `interval_days < 21`), mastered (review, `interval_days >= 21`). Banished is NOT struggling.
- `daysLeft` = target − today + 1 (local calendar dates), min 0. `pastTarget` only when target < today and cards remain.
- `pavlov_target_date` defaults to `2026-12-31`.
- There are no DB-backed Rust tests in this repo (only `#[cfg(test)]` pure-logic tests and read-only SQL sanity scripts under `scripts/verify-*.sql`). Follow that: pure logic gets unit tests; SQL gets a verify script run against the live DB after the migration is applied.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Run Rust commands from `backend/` (`cargo build`, `cargo test`); frontend from `frontend/` (`npm run check`, `npm run build`).

---

## File map

| File | Responsibility |
|---|---|
| `backend/migrations/0013_pavlov_reviews.sql` | Create: review log, deck snapshots, target date column |
| `backend/src/routes/pavlov.rs` | Modify `drill_grade`: transaction + review-log insert; add `is_first_grade` |
| `backend/src/routes/preferences.rs` | Modify: read/write `pavlov_target_date` |
| `backend/src/pavlov_stats.rs` | Create: pure `compute_progress` + tests |
| `backend/src/routes/pavlov_stats.rs` | Create: `GET /api/pavlov/stats` handler |
| `backend/src/main.rs` | Modify: `mod pavlov_stats;`, route |
| `backend/src/routes/mod.rs` | Modify: `pub mod pavlov_stats;` |
| `scripts/verify-pavlov-stats.sql` | Create: read-only sanity checks for the new tables/queries |
| `scripts/dev-mock-api.mjs` | Create: mock API for local visual verification |
| `frontend/src/lib/stats.ts` | Create: shared TypeScript types for stats payloads |
| `frontend/src/lib/components/StatsSection.svelte` | Create: shared section (summary strip, forecast, deck bar, tiles, charts, table) |
| `frontend/src/lib/components/PavlovProgressCard.svelte` | Create: deck-progress tile |
| `frontend/src/routes/dashboard/+page.svelte` | Rewrite: two sections built from `StatsSection` |
| `frontend/src/routes/settings/+page.svelte` | Modify: target-date input |

---

### Task 1: Migration 0013 and review-log insert on grade

**Files:**
- Create: `backend/migrations/0013_pavlov_reviews.sql`
- Modify: `backend/src/routes/pavlov.rs` (struct `PavlovCardRow` ~line 438, fn `drill_grade` ~lines 449–516)

**Interfaces:**
- Produces: table `pavlov_reviews(id, user_id, answer_id, rating, first_grade, reviewed_at)`; table `pavlov_deck_snapshots(user_id, snap_date, learning, maturing, mastered, struggling)`; column `users.pavlov_target_date DATE`. Pure fn `pub fn is_first_grade(existing: Option<&PavlovCardRow>) -> bool` in `routes/pavlov.rs`.

- [ ] **Step 1: Write the migration**

Create `backend/migrations/0013_pavlov_reviews.sql`:

```sql
-- 0013: Pavlov per-grade review log (accuracy over time / cold-vs-review),
-- daily deck-composition snapshots (week-over-week deltas), and the user's
-- deck-completion target date for the dashboard progress card.
-- Additive only; safe to apply while the previous image is running.

CREATE TABLE IF NOT EXISTS pavlov_reviews (
  id          BIGSERIAL PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  answer_id   INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
  rating      TEXT NOT NULL CHECK (rating IN ('wrong', 'got_it', 'too_easy')),
  first_grade BOOLEAN NOT NULL,
  reviewed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS pavlov_reviews_user_time   ON pavlov_reviews (user_id, reviewed_at);
CREATE INDEX IF NOT EXISTS pavlov_reviews_user_answer ON pavlov_reviews (user_id, answer_id);

CREATE TABLE IF NOT EXISTS pavlov_deck_snapshots (
  user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  snap_date  DATE NOT NULL,
  learning   INTEGER NOT NULL,
  maturing   INTEGER NOT NULL,
  mastered   INTEGER NOT NULL,
  struggling INTEGER NOT NULL,
  PRIMARY KEY (user_id, snap_date)
);

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS pavlov_target_date DATE NOT NULL DEFAULT '2026-12-31';
```

- [ ] **Step 2: Write the failing test for `is_first_grade`**

In `backend/src/routes/pavlov.rs`, there is no `#[cfg(test)]` module yet. Append at the very end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::{is_first_grade, PavlovCardRow};
    use chrono::Utc;

    fn row(last_review: Option<chrono::DateTime<Utc>>) -> PavlovCardRow {
        PavlovCardRow {
            state: "learning".into(),
            interval_days: 0.0,
            ease: 2.5,
            reps: 0,
            lapses: 0,
            step_index: 0,
            last_review,
        }
    }

    #[test]
    fn no_card_row_is_first_grade() {
        assert!(is_first_grade(None));
    }

    #[test]
    fn banish_created_card_never_graded_is_first_grade() {
        // suspend() inserts a card row with last_review NULL; un-banishing then
        // grading must still count as the card's first grade.
        assert!(is_first_grade(Some(&row(None))));
    }

    #[test]
    fn previously_graded_card_is_not_first_grade() {
        assert!(!is_first_grade(Some(&row(Some(Utc::now())))));
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd backend && cargo test routes::pavlov::tests 2>&1 | tail -20`
Expected: compile error — `is_first_grade` not found, `last_review` not a field of `PavlovCardRow`.

- [ ] **Step 4: Add `last_review` to `PavlovCardRow`, add `is_first_grade`, log the review inside a transaction**

Replace the `PavlovCardRow` struct with:

```rust
#[derive(sqlx::FromRow)]
pub(crate) struct PavlovCardRow {
    pub(crate) state: String,
    pub(crate) interval_days: f64,
    pub(crate) ease: f64,
    pub(crate) reps: i32,
    pub(crate) lapses: i32,
    pub(crate) step_index: i16,
    pub(crate) last_review: Option<DateTime<Utc>>,
}

/// A grade is the card's first when no card row exists yet, or when the row
/// was created by a suspend/banish and has never been graded (last_review is
/// NULL). Mirrors the `last_review IS NOT NULL` rule used for "new today".
pub(crate) fn is_first_grade(existing: Option<&PavlovCardRow>) -> bool {
    existing.map_or(true, |r| r.last_review.is_none())
}
```

Then in `drill_grade`, replace everything from `let existing: Option<PavlovCardRow> = ...` through the `.execute(&state.pool).await?;` after the card upsert with:

```rust
    let mut tx = state.pool.begin().await?;

    let existing: Option<PavlovCardRow> = sqlx::query_as(
        "SELECT state, interval_days, ease, reps, lapses, step_index, last_review
         FROM pavlov_cards WHERE user_id = $1 AND answer_id = $2",
    )
    .bind(user_id)
    .bind(body.answer_id)
    .fetch_optional(&mut *tx)
    .await?;
    let first_grade = is_first_grade(existing.as_ref());
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
           (user_id, answer_id, state, interval_days, ease, due, last_review, reps, lapses, step_index, suspended)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (user_id, answer_id) DO UPDATE SET
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
    .bind(body.answer_id)
    .bind(out.state.as_str())
    .bind(out.interval_days)
    .bind(out.ease)
    .bind(due)
    .bind(now)
    .bind(out.reps)
    .bind(out.lapses)
    .bind(out.step_index)
    .bind(suspended)
    .execute(&mut *tx)
    .await?;

    // Per-grade log — the dashboard's only source for accuracy over time.
    // `body.rating` was validated by Rating::from_wire above.
    sqlx::query(
        "INSERT INTO pavlov_reviews (user_id, answer_id, rating, first_grade, reviewed_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(body.answer_id)
    .bind(&body.rating)
    .bind(first_grade)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
```

Leave the trailing `Ok(Json(json!({ "state": ..., "due": due, ... })))` as it was.

- [ ] **Step 5: Run tests and build**

Run: `cd backend && cargo test routes::pavlov::tests 2>&1 | tail -8 && cargo build 2>&1 | tail -3`
Expected: `test result: ok. 3 passed`; build finishes with no errors (warnings about unused `pub(crate)` are fine).

- [ ] **Step 6: Commit**

```bash
git add backend/migrations/0013_pavlov_reviews.sql backend/src/routes/pavlov.rs
git commit -m "feat(pavlov): migration 0013 — review log, deck snapshots, target date; grade logs a review row

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Target date in preferences and Settings

**Files:**
- Modify: `backend/src/routes/preferences.rs` (whole file is ~95 lines)
- Modify: `frontend/src/routes/settings/+page.svelte` (script lines 14–50, markup ~lines 108–118)

**Interfaces:**
- Produces: `GET /api/preferences` includes `pavlovTargetDate: "YYYY-MM-DD"`; `PUT /api/preferences` accepts optional `pavlovTargetDate: "YYYY-MM-DD"`.

- [ ] **Step 1: Backend — read the column**

In `preferences::get`, change the tuple type and query to:

```rust
    let row: (Option<String>, i32, Option<String>, bool, i32, chrono::NaiveDate) = sqlx::query_as(
        "SELECT game_type_filters, new_cards_per_day, timezone, adaptive_targeting, pavlov_new_per_day, pavlov_target_date FROM users WHERE id = $1",
    )
```

and add to the JSON object:

```rust
        "pavlovTargetDate": row.5,
```

(`chrono::NaiveDate` serializes as `"2026-12-31"` with the `serde` feature already enabled in Cargo.toml.)

- [ ] **Step 2: Backend — write the column**

Add to `UpdatePreferencesBody`:

```rust
    pub pavlov_target_date: Option<chrono::NaiveDate>,
```

Add after the `pavlov_new_per_day` block in `update`:

```rust
    if let Some(d) = body.pavlov_target_date {
        sqlx::query("UPDATE users SET pavlov_target_date = $1 WHERE id = $2")
            .bind(d)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }
```

- [ ] **Step 3: Build**

Run: `cd backend && cargo build 2>&1 | tail -3`
Expected: no errors.

- [ ] **Step 4: Settings page — state, load, save**

In `frontend/src/routes/settings/+page.svelte` script, after `let pavlovNewPerDay = $state(20);` add:

```ts
  let pavlovTargetDate = $state('');
```

In `onMount`, after `pavlovNewPerDay = prefs?.pavlovNewPerDay ?? 20;` add:

```ts
      pavlovTargetDate = prefs?.pavlovTargetDate ?? '';
```

In `saveSrsPrefs`, change the `api.put` body to:

```ts
      await api.put('/api/preferences', {
        gameTypeFilters,
        newCardsPerDay,
        pavlovNewPerDay,
        // Empty string would fail NaiveDate parsing server-side; omit instead.
        pavlovTargetDate: pavlovTargetDate || undefined,
        timezone,
        adaptiveTargeting,
      });
```

- [ ] **Step 5: Settings page — input**

Directly after the closing `</label>` of the "Pavlov new cards/day" label, add:

```svelte
        <label class="block">
          <span class="text-sm font-semibold text-gray-700">Pavlov target date</span>
          <input
            type="date"
            bind:value={pavlovTargetDate}
            onchange={saveSrsPrefs}
            class="mt-1 w-48 rounded-lg border border-gray-300 px-3 py-2"
          />
          <span class="block text-xs text-gray-400 mt-1">The dashboard's deck-progress pace is measured against this date.</span>
        </label>
```

- [ ] **Step 6: Type-check**

Run: `cd frontend && npm run check 2>&1 | tail -5`
Expected: `svelte-check found 0 errors` (pre-existing warnings are acceptable if the count did not grow).

- [ ] **Step 7: Commit**

```bash
git add backend/src/routes/preferences.rs frontend/src/routes/settings/+page.svelte
git commit -m "feat(pavlov): pavlov_target_date preference + Settings date input

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Pure progress math

**Files:**
- Create: `backend/src/pavlov_stats.rs`
- Modify: `backend/src/main.rs` (add `mod pavlov_stats;` next to `mod pavlov;` at line 11)

**Interfaces:**
- Produces:
  ```rust
  pub struct Progress { deck_total: i64, touched: i64, touched_pct: f64, trailing_per_day: f64,
                        target_date: NaiveDate, days_left: i64, required_per_day: i64, past_target: bool,
                        projected_finish: Option<NaiveDate>, days_ahead: Option<i64> }  // Serialize, camelCase
  pub fn compute_progress(deck_total: i64, touched: i64, created_last_14d: i64,
                          today: NaiveDate, target: NaiveDate) -> Progress
  ```

- [ ] **Step 1: Write the failing tests**

Create `backend/src/pavlov_stats.rs` with only the tests for now:

```rust
//! Pure math for the Pavlov deck-progress card (spec §2, "Progress math").

use chrono::NaiveDate;
use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn typical_mid_september_snapshot() {
        // 4,765 deck, 1,097 touched, 154 created in last 14 days (11/day).
        let p = compute_progress(4765, 1097, 154, d(2026, 9, 15), d(2026, 12, 31));
        assert_eq!(p.days_left, 108); // Sept 15 .. Dec 31 inclusive
        assert_eq!(p.required_per_day, 34); // ceil(3668 / 108)
        assert!(!p.past_target);
        assert!((p.trailing_per_day - 11.0).abs() < 1e-9);
        assert!((p.touched_pct - 23.02).abs() < 0.01);
        // ceil(3668 / 11) = 334 days → 2027-08-15
        assert_eq!(p.projected_finish, Some(d(2027, 8, 15)));
        assert_eq!(p.days_ahead, Some(108 - 334));
    }

    #[test]
    fn zero_trailing_pace_has_no_projection() {
        let p = compute_progress(100, 10, 0, d(2026, 9, 15), d(2026, 12, 31));
        assert_eq!(p.trailing_per_day, 0.0);
        assert_eq!(p.projected_finish, None);
        assert_eq!(p.days_ahead, None);
        assert_eq!(p.required_per_day, 1); // ceil(90 / 108)
    }

    #[test]
    fn target_in_the_past_reports_remaining_and_flags() {
        let p = compute_progress(100, 40, 14, d(2026, 9, 15), d(2026, 9, 1));
        assert_eq!(p.days_left, 0);
        assert!(p.past_target);
        assert_eq!(p.required_per_day, 60);
    }

    #[test]
    fn target_today_counts_today_as_a_day() {
        let p = compute_progress(100, 40, 14, d(2026, 9, 15), d(2026, 9, 15));
        assert_eq!(p.days_left, 1);
        assert!(!p.past_target);
        assert_eq!(p.required_per_day, 60);
    }

    #[test]
    fn deck_complete_is_finished_today() {
        let p = compute_progress(100, 100, 0, d(2026, 9, 15), d(2026, 12, 31));
        assert_eq!(p.required_per_day, 0);
        assert!(!p.past_target);
        assert_eq!(p.projected_finish, Some(d(2026, 9, 15)));
        assert_eq!(p.days_ahead, Some(108));
        assert!((p.touched_pct - 100.0).abs() < 1e-9);
    }

    #[test]
    fn required_per_day_rounds_up() {
        // 10 remaining over 3 days → 4/day, not 3.
        let p = compute_progress(20, 10, 0, d(2026, 9, 15), d(2026, 9, 17));
        assert_eq!(p.days_left, 3);
        assert_eq!(p.required_per_day, 4);
    }

    #[test]
    fn touched_above_deck_total_clamps_remaining_to_zero() {
        // Deck regenerated smaller than what was already touched.
        let p = compute_progress(50, 60, 0, d(2026, 9, 15), d(2026, 12, 31));
        assert_eq!(p.required_per_day, 0);
        assert!((p.touched_pct - 100.0).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail**

In `backend/src/main.rs` add `mod pavlov_stats;` directly after `mod pavlov;`.

Run: `cd backend && cargo test pavlov_stats 2>&1 | tail -5`
Expected: compile error, `compute_progress` not found.

- [ ] **Step 3: Implement**

Insert between the `use` lines and the `#[cfg(test)]` block:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub deck_total: i64,
    pub touched: i64,
    pub touched_pct: f64,
    pub trailing_per_day: f64,
    pub target_date: NaiveDate,
    /// Calendar days from today through the target, inclusive; 0 when passed.
    pub days_left: i64,
    /// Cards/day needed from today to finish by the target. When the target
    /// has passed this is simply the remaining count (see `past_target`).
    pub required_per_day: i64,
    pub past_target: bool,
    /// Date the deck finishes at the trailing pace; None when pace is zero.
    pub projected_finish: Option<NaiveDate>,
    /// days_left − days needed at trailing pace; positive = ahead. None when
    /// pace is zero.
    pub days_ahead: Option<i64>,
}

const TRAILING_WINDOW_DAYS: f64 = 14.0;

pub fn compute_progress(
    deck_total: i64,
    touched: i64,
    created_last_14d: i64,
    today: NaiveDate,
    target: NaiveDate,
) -> Progress {
    let remaining = (deck_total - touched).max(0);
    let touched_pct = if deck_total > 0 {
        ((touched.min(deck_total)) as f64 / deck_total as f64) * 100.0
    } else {
        0.0
    };
    let trailing_per_day = created_last_14d as f64 / TRAILING_WINDOW_DAYS;

    let days_left = ((target - today).num_days() + 1).max(0);
    let past_target = target < today && remaining > 0;
    let required_per_day = if remaining == 0 {
        0
    } else if days_left == 0 {
        remaining
    } else {
        (remaining + days_left - 1) / days_left // ceil
    };

    let days_needed: Option<i64> = if remaining == 0 {
        Some(0)
    } else if trailing_per_day > 0.0 {
        Some((remaining as f64 / trailing_per_day).ceil() as i64)
    } else {
        None
    };
    let projected_finish = days_needed.map(|n| today + chrono::Duration::days(n));
    let days_ahead = days_needed.map(|n| days_left - n);

    Progress {
        deck_total,
        touched,
        touched_pct,
        trailing_per_day,
        target_date: target,
        days_left,
        required_per_day,
        past_target,
        projected_finish,
        days_ahead,
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd backend && cargo test pavlov_stats 2>&1 | tail -5`
Expected: `test result: ok. 7 passed`.

- [ ] **Step 5: Commit**

```bash
git add backend/src/pavlov_stats.rs backend/src/main.rs
git commit -m "feat(pavlov): pure deck-progress math (required pace, projection, days ahead)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `GET /api/pavlov/stats` endpoint and SQL verify script

**Files:**
- Create: `backend/src/routes/pavlov_stats.rs`
- Create: `scripts/verify-pavlov-stats.sql`
- Modify: `backend/src/routes/mod.rs` (add `pub mod pavlov_stats;`)
- Modify: `backend/src/main.rs` (add route after line 122 `/api/pavlov/drill/grade`)

**Interfaces:**
- Consumes: `crate::pavlov_stats::compute_progress` (Task 3); `crate::routes::practice::day_start_utc(now, tz: Option<&str>)`.
- Produces: JSON per spec §2 — `overall, cold, review, cold30d, dailyAccuracy[], categoryBreakdown[], reviewedToday, dueCount, newRemaining, forecast[], deck{learning,maturing,mastered,struggling,banished,total,delta}, progress{...}, historySince`.

- [ ] **Step 1: Write the handler**

Create `backend/src/routes/pavlov_stats.rs`:

```rust
//! GET /api/pavlov/stats — Pavlov's counterpart to /api/stats + /api/practice/status,
//! shaped identically where the concept exists so the dashboard shares types.

use axum::{extract::State, Json};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::pavlov_stats::compute_progress;
use crate::routes::practice::day_start_utc;
use crate::AppState;

#[derive(FromRow)]
struct SplitRow {
    total: i64,
    correct: i64,
    cold_total: i64,
    cold_correct: i64,
    review_total: i64,
    review_correct: i64,
}

#[derive(FromRow)]
struct CategoryRow {
    category: String,
    total: i64,
    correct: i64,
    cold_total: i64,
    cold_correct: i64,
    review_total: i64,
    review_correct: i64,
}

#[derive(FromRow)]
struct DailyRow {
    date: NaiveDate,
    total: i64,
    correct: i64,
    cold_total: i64,
    cold_correct: i64,
    review_total: i64,
    review_correct: i64,
}

fn pct(correct: i64, total: i64) -> f64 {
    if total > 0 {
        correct as f64 / total as f64 * 100.0
    } else {
        0.0
    }
}

fn pack(total: i64, correct: i64) -> Value {
    json!({ "total": total, "correct": correct, "accuracy": pct(correct, total) })
}

/// The six aggregate columns every split query shares. `$1` is user_id.
const SPLIT_COLS: &str = "COUNT(*)::bigint AS total,
    COUNT(*) FILTER (WHERE rating <> 'wrong')::bigint AS correct,
    COUNT(*) FILTER (WHERE first_grade)::bigint AS cold_total,
    COUNT(*) FILTER (WHERE first_grade AND rating <> 'wrong')::bigint AS cold_correct,
    COUNT(*) FILTER (WHERE NOT first_grade)::bigint AS review_total,
    COUNT(*) FILTER (WHERE NOT first_grade AND rating <> 'wrong')::bigint AS review_correct";

pub async fn stats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let (new_per_day, tz, target_date): (i32, Option<String>, NaiveDate) = sqlx::query_as(
        "SELECT pavlov_new_per_day, timezone, pavlov_target_date FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let zone: chrono_tz::Tz = tz
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let now = Utc::now();
    let day_start = day_start_utc(now, tz.as_deref());
    let today = now.with_timezone(&zone).date_naive();

    // --- accuracy from the review log ---------------------------------------
    let all: SplitRow = sqlx::query_as(&format!(
        "SELECT {SPLIT_COLS} FROM pavlov_reviews WHERE user_id = $1"
    ))
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let (c30_t, c30_c): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(*) FILTER (WHERE rating <> 'wrong')::bigint
         FROM pavlov_reviews
         WHERE user_id = $1 AND first_grade AND reviewed_at >= now() - interval '30 days'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let history_since: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT MIN(reviewed_at) FROM pavlov_reviews WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    let reviewed_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_reviews WHERE user_id = $1 AND reviewed_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;

    let daily_rows: Vec<DailyRow> = sqlx::query_as(&format!(
        "SELECT (reviewed_at AT TIME ZONE $2)::date AS date, {SPLIT_COLS}
         FROM pavlov_reviews
         WHERE user_id = $1 AND reviewed_at >= now() - interval '30 days'
         GROUP BY 1 ORDER BY 1"
    ))
    .bind(user_id)
    .bind(zone.name())
    .fetch_all(&state.pool)
    .await?;
    let daily_accuracy: Vec<Value> = daily_rows
        .into_iter()
        .map(|d| {
            json!({
                "date": d.date,
                "total": d.total, "correct": d.correct, "accuracy": pct(d.correct, d.total),
                "coldTotal": d.cold_total, "coldCorrect": d.cold_correct,
                "coldAccuracy": pct(d.cold_correct, d.cold_total),
                "reviewTotal": d.review_total, "reviewCorrect": d.review_correct,
                "reviewAccuracy": pct(d.review_correct, d.review_total),
            })
        })
        .collect();

    let category_rows: Vec<CategoryRow> = sqlx::query_as(&format!(
        "SELECT pa.meta_category AS category, {SPLIT_COLS}
         FROM pavlov_reviews pr
         JOIN pavlov_answers pa ON pa.id = pr.answer_id
         WHERE pr.user_id = $1
         GROUP BY 1 ORDER BY 1"
    ))
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let category_breakdown: Vec<Value> = category_rows
        .into_iter()
        .map(|c| {
            json!({
                "category": c.category,
                "total": c.total, "correct": c.correct, "accuracy": pct(c.correct, c.total),
                "coldTotal": c.cold_total, "coldCorrect": c.cold_correct,
                "coldAccuracy": pct(c.cold_correct, c.cold_total),
                "reviewTotal": c.review_total, "reviewCorrect": c.review_correct,
                "reviewAccuracy": pct(c.review_correct, c.review_total),
            })
        })
        .collect();

    // --- queue state from card rows -----------------------------------------
    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards
         WHERE user_id = $1 AND suspended = false AND due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards
         WHERE user_id = $1 AND created_at >= $2 AND last_review IS NOT NULL",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    // 14-day due forecast, user-local days, overdue folded into today
    // (same shape as /api/practice/status).
    let forecast: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "SELECT GREATEST((due AT TIME ZONE $2)::date, (now() AT TIME ZONE $2)::date) AS d, COUNT(*)
         FROM pavlov_cards
         WHERE user_id = $1 AND suspended = false
           AND (due AT TIME ZONE $2)::date <= (now() AT TIME ZONE $2)::date + 13
         GROUP BY d ORDER BY d",
    )
    .bind(user_id)
    .bind(zone.name())
    .fetch_all(&state.pool)
    .await?;
    let forecast_json: Vec<Value> = forecast
        .into_iter()
        .map(|(d, c)| json!({ "date": d, "count": c }))
        .collect();

    // --- deck composition (spec §2 bucket table) ----------------------------
    let (learning, maturing, mastered, struggling, banished): (i64, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT
               COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state <> 'review'),
               COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days < 21),
               COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days >= 21),
               COUNT(*) FILTER (WHERE NOT suspended AND lapses >= 4),
               COUNT(*) FILTER (WHERE suspended)
             FROM pavlov_cards WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;
    let touched = learning + maturing + mastered + struggling + banished;

    // Snapshot today (user-local date) and diff against a baseline: newest
    // snapshot at least a week old, else the oldest one before today.
    sqlx::query(
        "INSERT INTO pavlov_deck_snapshots (user_id, snap_date, learning, maturing, mastered, struggling)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id, snap_date) DO UPDATE SET
           learning = EXCLUDED.learning, maturing = EXCLUDED.maturing,
           mastered = EXCLUDED.mastered, struggling = EXCLUDED.struggling",
    )
    .bind(user_id)
    .bind(today)
    .bind(learning as i32)
    .bind(maturing as i32)
    .bind(mastered as i32)
    .bind(struggling as i32)
    .execute(&state.pool)
    .await?;

    let baseline: Option<(NaiveDate, i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT snap_date, learning, maturing, mastered, struggling FROM pavlov_deck_snapshots
         WHERE user_id = $1 AND snap_date <= $2::date - 7
         ORDER BY snap_date DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(today)
    .fetch_optional(&state.pool)
    .await?;
    let baseline = match baseline {
        Some(b) => Some(b),
        None => {
            sqlx::query_as(
                "SELECT snap_date, learning, maturing, mastered, struggling FROM pavlov_deck_snapshots
                 WHERE user_id = $1 AND snap_date < $2
                 ORDER BY snap_date ASC LIMIT 1",
            )
            .bind(user_id)
            .bind(today)
            .fetch_optional(&state.pool)
            .await?
        }
    };
    let delta = baseline.map(|(since, l, y, m, s)| {
        json!({
            "since": since,
            "learning": learning - l as i64,
            "maturing": maturing - y as i64,
            "mastered": mastered - m as i64,
            "struggling": struggling - s as i64,
        })
    });

    // --- progress toward the target date ------------------------------------
    let deck_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pavlov_answers")
        .fetch_one(&state.pool)
        .await?;
    let window_start = day_start - Duration::days(13); // today + 13 prior local days
    let created_14d: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards WHERE user_id = $1 AND created_at >= $2",
    )
    .bind(user_id)
    .bind(window_start)
    .fetch_one(&state.pool)
    .await?;
    let progress = compute_progress(deck_total, touched, created_14d, today, target_date);

    Ok(Json(json!({
        "overall": pack(all.total, all.correct),
        "cold": pack(all.cold_total, all.cold_correct),
        "review": pack(all.review_total, all.review_correct),
        "cold30d": pack(c30_t, c30_c),
        "historySince": history_since,
        "dailyAccuracy": daily_accuracy,
        "categoryBreakdown": category_breakdown,
        "reviewedToday": reviewed_today,
        "dueCount": due_count,
        "newRemaining": new_remaining,
        "forecast": forecast_json,
        "deck": {
            "learning": learning,
            "maturing": maturing,
            "mastered": mastered,
            "struggling": struggling,
            "banished": banished,
            "total": touched,
            "delta": delta,
        },
        "progress": progress,
    })))
}
```

- [ ] **Step 2: Wire the module and route**

In `backend/src/routes/mod.rs` add `pub mod pavlov_stats;` (alphabetically after `pub mod pavlov;`).

In `backend/src/main.rs`, after the line
`.route("/api/pavlov/drill/grade", post(routes::pavlov::drill_grade))`
add:

```rust
        .route("/api/pavlov/stats", get(routes::pavlov_stats::stats))
```

- [ ] **Step 3: Build and run the full test suite**

Run: `cd backend && cargo build 2>&1 | tail -3 && cargo test 2>&1 | grep "test result"`
Expected: build succeeds; every `test result:` line shows `0 failed`.

- [ ] **Step 4: Write the SQL verify script**

sqlx runtime queries are only checked when executed, so this script exercises every query shape from the handler read-only against the live DB. It is run in Task 8 after the migration is applied. Create `scripts/verify-pavlov-stats.sql`:

```sql
-- Sanity checks for /api/pavlov/stats queries (PG15-compatible, read-only).
-- Run after migration 0013: docker run --rm -i postgres:16 psql "$DB_URL" -f - < scripts/verify-pavlov-stats.sql
-- Uses user_id = 1. "expect 0" checks are failures when nonzero.

-- A. Informational: tables exist and column shapes are right.
SELECT 'pavlov_reviews' AS t, count(*) AS rows FROM pavlov_reviews
UNION ALL SELECT 'pavlov_deck_snapshots', count(*) FROM pavlov_deck_snapshots;
SELECT pavlov_target_date FROM users WHERE id = 1;

-- B. Split aggregates (mirror SPLIT_COLS in routes/pavlov_stats.rs).
SELECT COUNT(*)::bigint AS total,
       COUNT(*) FILTER (WHERE rating <> 'wrong')::bigint AS correct,
       COUNT(*) FILTER (WHERE first_grade)::bigint AS cold_total,
       COUNT(*) FILTER (WHERE first_grade AND rating <> 'wrong')::bigint AS cold_correct,
       COUNT(*) FILTER (WHERE NOT first_grade)::bigint AS review_total,
       COUNT(*) FILTER (WHERE NOT first_grade AND rating <> 'wrong')::bigint AS review_correct
FROM pavlov_reviews WHERE user_id = 1;

-- C. Daily buckets in the user's zone.
SELECT (reviewed_at AT TIME ZONE 'America/Los_Angeles')::date AS date, count(*)
FROM pavlov_reviews WHERE user_id = 1 AND reviewed_at >= now() - interval '30 days'
GROUP BY 1 ORDER BY 1;

-- D. Category rollup joins cleanly (expect 0 orphan reviews).
SELECT 'orphan_reviews' AS check, count(*) AS fail_rows
FROM pavlov_reviews pr LEFT JOIN pavlov_answers pa ON pa.id = pr.answer_id
WHERE pr.user_id = 1 AND pa.id IS NULL;

-- E. Deck buckets are exhaustive and exclusive (expect 0).
WITH b AS (
  SELECT COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state <> 'review') AS learning,
         COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days < 21) AS maturing,
         COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days >= 21) AS mastered,
         COUNT(*) FILTER (WHERE NOT suspended AND lapses >= 4) AS struggling,
         COUNT(*) FILTER (WHERE suspended) AS banished,
         COUNT(*) AS total
  FROM pavlov_cards WHERE user_id = 1)
SELECT 'bucket_sum_mismatch' AS check,
       CASE WHEN learning + maturing + mastered + struggling + banished = total THEN 0 ELSE 1 END AS fail_rows,
       learning, maturing, mastered, struggling, banished, total
FROM b;

-- F. Forecast shape (informational).
SELECT GREATEST((due AT TIME ZONE 'America/Los_Angeles')::date, (now() AT TIME ZONE 'America/Los_Angeles')::date) AS d, COUNT(*)
FROM pavlov_cards
WHERE user_id = 1 AND suspended = false
  AND (due AT TIME ZONE 'America/Los_Angeles')::date <= (now() AT TIME ZONE 'America/Los_Angeles')::date + 13
GROUP BY d ORDER BY d;

-- G. After the first post-deploy grade: every review row's card exists and
--    first_grade rows are unique per card (expect 0 for both).
SELECT 'review_without_card' AS check, count(*) AS fail_rows
FROM pavlov_reviews pr LEFT JOIN pavlov_cards pc ON pc.user_id = pr.user_id AND pc.answer_id = pr.answer_id
WHERE pr.user_id = 1 AND pc.user_id IS NULL;
SELECT 'duplicate_first_grade' AS check, count(*) AS fail_rows FROM (
  SELECT answer_id FROM pavlov_reviews WHERE user_id = 1 AND first_grade GROUP BY answer_id HAVING count(*) > 1
) d;
```

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/pavlov_stats.rs backend/src/routes/mod.rs backend/src/main.rs scripts/verify-pavlov-stats.sql
git commit -m "feat(pavlov): GET /api/pavlov/stats — accuracy splits, deck buckets, forecast, progress

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Shared stats types, `StatsSection`, and `PavlovProgressCard`

**Files:**
- Create: `frontend/src/lib/stats.ts`
- Create: `frontend/src/lib/components/StatsSection.svelte`
- Create: `frontend/src/lib/components/PavlovProgressCard.svelte`

**Interfaces:**
- Consumes: `StatsChart` component (`type`, `data`, `options` props) at `frontend/src/lib/components/StatsChart.svelte`.
- Produces: the types below; `StatsSection` props and snippets exactly as declared; `PavlovProgressCard` with a single `progress` prop.

This task only creates files; nothing renders until Task 6 wires them in. `npm run check` still type-checks them.

- [ ] **Step 1: Types**

Create `frontend/src/lib/stats.ts`:

```ts
// Shared shapes for /api/stats, /api/practice/status and /api/pavlov/stats.

export interface KindStat {
  total: number;
  correct: number;
  accuracy: number;
}

export interface SplitStat {
  total: number;
  correct: number;
  accuracy: number;
  coldTotal: number;
  coldCorrect: number;
  coldAccuracy: number;
  reviewTotal: number;
  reviewCorrect: number;
  reviewAccuracy: number;
}

export interface CategoryStat extends SplitStat {
  category: string;
}

export interface DailyStat extends SplitStat {
  date: string;
}

export interface ForecastDay {
  date: string;
  count: number;
}

export interface DeckDelta {
  since: string;
  learning: number;
  maturing: number;
  mastered: number;
  struggling: number;
}

export interface DeckStats {
  learning: number;
  maturing: number;
  mastered: number;
  struggling: number;
  banished?: number;
  total: number;
  delta: DeckDelta | null;
}

export interface SummaryStrip {
  due: number;
  newLeft: number;
  reviewedToday: number;
  href: string;
  label: string;
}

export interface PavlovProgress {
  deckTotal: number;
  touched: number;
  touchedPct: number;
  trailingPerDay: number;
  targetDate: string;
  daysLeft: number;
  requiredPerDay: number;
  pastTarget: boolean;
  projectedFinish: string | null;
  daysAhead: number | null;
}

export interface PavlovStats {
  overall: KindStat;
  cold: KindStat;
  review: KindStat;
  cold30d: KindStat;
  historySince: string | null;
  dailyAccuracy: DailyStat[];
  categoryBreakdown: CategoryStat[];
  reviewedToday: number;
  dueCount: number;
  newRemaining: number;
  forecast: ForecastDay[];
  deck: DeckStats;
  progress: PavlovProgress;
}

/** Local calendar date → 'YYYY-MM-DD' (matches backend user-timezone bucketing). */
export function localDateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** 'YYYY-MM-DD' → short label like "Dec 31" without UTC-shift surprises. */
export function fmtDate(iso: string): string {
  return new Date(iso + 'T00:00:00').toLocaleDateString([], { month: 'short', day: 'numeric' });
}
```

- [ ] **Step 2: `PavlovProgressCard`**

Create `frontend/src/lib/components/PavlovProgressCard.svelte`:

```svelte
<script lang="ts">
  import type { PavlovProgress } from '$lib/stats';
  import { fmtDate } from '$lib/stats';

  let { progress }: { progress: PavlovProgress } = $props();

  // green: trailing pace meets requirement; amber: within 20% below; red otherwise.
  let paceClass = $derived.by(() => {
    if (progress.requiredPerDay === 0) return 'text-green-600';
    const ratio = progress.trailingPerDay / progress.requiredPerDay;
    return ratio >= 1 ? 'text-green-600' : ratio >= 0.8 ? 'text-amber-500' : 'text-red-500';
  });
  let remaining = $derived(Math.max(0, progress.deckTotal - progress.touched));
</script>

<div class="flex-1 min-w-[260px] bg-white rounded-xl shadow p-6 border-2 border-jeopardy-gold">
  <p class="text-sm font-medium text-gray-500 mb-1">Deck progress</p>
  <p class="text-3xl font-bold text-jeopardy-blue">
    {progress.touched.toLocaleString()}<span class="text-gray-400 text-xl"> / {progress.deckTotal.toLocaleString()}</span>
    <span class="text-base font-semibold text-gray-500 ml-2">{progress.touchedPct.toFixed(0)}%</span>
  </p>
  <div class="mt-2 h-2 rounded-full bg-gray-100 overflow-hidden">
    <div class="h-full bg-jeopardy-gold rounded-full" style="width: {Math.min(100, progress.touchedPct)}%"></div>
  </div>
  {#if remaining === 0}
    <p class="text-xs text-green-600 mt-2 font-semibold">Deck complete.</p>
  {:else if progress.pastTarget}
    <p class="text-xs text-red-500 mt-2">
      Target {fmtDate(progress.targetDate)} passed — {remaining.toLocaleString()} cards left.
      <a href="/settings" class="text-jeopardy-blue hover:underline">Change target</a>
    </p>
  {:else}
    <p class="text-xs text-gray-400 mt-2">
      Need <span class="font-semibold {paceClass}">{progress.requiredPerDay}/day</span>
      to finish by <a href="/settings" class="text-jeopardy-blue hover:underline">{fmtDate(progress.targetDate)}</a>
      · trailing <span class="font-semibold {paceClass}">{progress.trailingPerDay.toFixed(0)}/day</span>
      {#if progress.projectedFinish}
        · projected {fmtDate(progress.projectedFinish)}
      {:else}
        · no new cards in 14 days
      {/if}
    </p>
  {/if}
</div>
```

- [ ] **Step 3: `StatsSection`**

Create `frontend/src/lib/components/StatsSection.svelte`. It renders, in order: header; summary card (strip, forecast, deck bar, `extras`); `afterSummary`; tile row (cold 30d, retention, `readiness`); `belowTiles`; accuracy chart; category chart; category table.

```svelte
<script lang="ts">
  import type { Snippet } from 'svelte';
  import StatsChart from '$lib/components/StatsChart.svelte';
  import type {
    CategoryStat,
    DailyStat,
    DeckStats,
    ForecastDay,
    KindStat,
    SummaryStrip,
  } from '$lib/stats';
  import { localDateKey, fmtDate } from '$lib/stats';

  interface Props {
    title: string;
    accent: 'blue' | 'gold';
    summary: SummaryStrip | null;
    forecast: ForecastDay[] | null;
    deck: DeckStats | null;
    /** Link base for deck bucket labels, e.g. '/cards?state=' — omit for plain text. */
    deckLinkBase?: string;
    cold30d: KindStat | null;
    cold: KindStat | null;
    review: KindStat | null;
    /** Caption under the cold tile's number, e.g. 'clues' or 'cue cards'. */
    unitLabel: string;
    reviewLabel: string;
    /** ISO timestamp of the first logged review; used for the empty state. */
    historySince?: string | null;
    daily: DailyStat[];
    categories: CategoryStat[];
    isMobile: boolean;
    extras?: Snippet;
    afterSummary?: Snippet;
    readiness?: Snippet;
    belowTiles?: Snippet;
  }

  let {
    title,
    accent,
    summary,
    forecast,
    deck,
    deckLinkBase,
    cold30d,
    cold,
    review,
    unitLabel,
    reviewLabel,
    historySince = null,
    daily,
    categories,
    isMobile,
    extras,
    afterSummary,
    readiness,
    belowTiles,
  }: Props = $props();

  const DECK_BUCKETS = [
    { key: 'learning', label: 'learning', color: '#f59e0b' },
    { key: 'maturing', label: 'maturing', color: '#60a5fa' },
    { key: 'mastered', label: 'mastered', color: '#22c55e' },
    { key: 'struggling', label: 'struggling', color: '#ef4444' },
    { key: 'banished', label: 'banished', color: '#9ca3af' },
  ] as const;
  type BucketKey = (typeof DECK_BUCKETS)[number]['key'];
  const bucketCount = (d: DeckStats, k: BucketKey) => (k === 'banished' ? (d.banished ?? 0) : d[k]);
  const fmtDelta = (n: number) => (n > 0 ? `+${n}` : `${n}`);

  let accentBtn = $derived(
    accent === 'gold'
      ? 'bg-jeopardy-gold text-jeopardy-blue hover:bg-yellow-400'
      : 'bg-jeopardy-blue text-white hover:bg-blue-800'
  );
  let accentBar = $derived(accent === 'gold' ? '#d4b200' : '#0c47b7');

  let hasHistory = $derived((cold?.total ?? 0) + (review?.total ?? 0) > 0);

  let lineChartData = $derived(
    daily.length
      ? {
          labels: daily.map((d) => d.date),
          datasets: [
            {
              label: 'Cold (first attempt) %',
              data: daily.map((d) => (d.coldTotal > 0 ? d.coldAccuracy : null)),
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
              data: daily.map((d) => (d.reviewTotal > 0 ? d.reviewAccuracy : null)),
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

  const lineChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: true, position: 'bottom' } },
    scales: {
      y: { min: 0, max: 100, title: { display: true, text: 'Accuracy %' } },
      x: { ticks: { maxRotation: 45 } },
    },
  };

  // Due forecast (7 days on phones, 14 otherwise), padded so quiet days render
  // as true zeros; axis built from local calendar days to match the backend.
  let forecastDays = $derived(isMobile ? 7 : 14);
  let forecastChartData = $derived.by(() => {
    if (!forecast) return null;
    const counts = new Map(forecast.map((f) => [f.date, f.count]));
    const start = new Date();
    const labels: string[] = [];
    const data: number[] = [];
    for (let i = 0; i < forecastDays; i++) {
      const d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i);
      labels.push(i === 0 ? 'Today' : d.toLocaleDateString([], { weekday: 'short', day: 'numeric' }));
      data.push(counts.get(localDateKey(d)) ?? 0);
    }
    return {
      labels,
      datasets: [{ label: 'Reviews due', data, backgroundColor: accentBar, borderRadius: 4, maxBarThickness: 28 }],
    };
  });

  const forecastChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: false } },
    scales: { y: { min: 0, ticks: { precision: 0 } }, x: { grid: { display: false } } },
  };

  let barChartData = $derived(
    categories.length
      ? {
          labels: categories.map((c) => c.category),
          datasets: [
            {
              label: 'Cold accuracy %',
              data: categories.map((c) => c.coldAccuracy),
              backgroundColor: categories.map((c) =>
                c.coldAccuracy >= 75 ? '#22c55e' : c.coldAccuracy >= 50 ? '#f59e0b' : '#ef4444'
              ),
              borderWidth: 1,
            },
          ],
        }
      : null
  );

  // Phones flip to horizontal bars so category names read normally.
  let barChartOptions = $derived(
    isMobile
      ? {
          responsive: true,
          maintainAspectRatio: false,
          indexAxis: 'y' as const,
          plugins: { legend: { display: false } },
          scales: { x: { min: 0, max: 100, title: { display: true, text: 'Cold accuracy %' } } },
        }
      : {
          responsive: true,
          maintainAspectRatio: false,
          scales: {
            y: { min: 0, max: 100, title: { display: true, text: 'Accuracy %' } },
            x: { ticks: { maxRotation: 45 } },
          },
        }
  );
  let barChartHeight = $derived(isMobile ? Math.max(220, categories.length * 28 + 60) : 300);

  let sortedCategories = $derived([...categories].sort((a, b) => a.coldAccuracy - b.coldAccuracy));
</script>

<section class="mb-12">
  <h2 class="text-2xl font-bold mb-4 {accent === 'gold' ? 'text-jeopardy-blue' : 'text-gray-800'}">
    {#if accent === 'gold'}<span class="inline-block w-2 h-6 bg-jeopardy-gold rounded align-middle mr-2"></span>{/if}{title}
  </h2>

  <!-- Summary card: strip + forecast + deck + section extras -->
  {#if summary}
    <div class="bg-white rounded-xl shadow-sm p-5 mb-8">
      <div class="flex flex-wrap gap-8">
        <div>
          <p class="text-3xl font-bold text-jeopardy-blue">{summary.due}</p>
          <p class="text-xs uppercase text-gray-500">Due today</p>
        </div>
        <div>
          <p class="text-3xl font-bold text-jeopardy-blue">{summary.newLeft}</p>
          <p class="text-xs uppercase text-gray-500">New left</p>
        </div>
        <div>
          <p class="text-3xl font-bold text-jeopardy-blue">{summary.reviewedToday}</p>
          <p class="text-xs uppercase text-gray-500">Reviewed today</p>
        </div>
        <a
          href={summary.href}
          class="w-full text-center sm:w-auto sm:ml-auto self-center px-4 py-2 rounded-lg text-sm font-semibold transition-colors {accentBtn}"
        >
          {summary.label} &rarr;
        </a>
      </div>

      {#if forecastChartData}
        <div class="mt-5 pt-4 border-t border-gray-100">
          <h3 class="text-sm font-semibold text-gray-600 mb-2">Reviews due — next {forecastDays} days</h3>
          <div class="h-36">
            <StatsChart type="bar" data={forecastChartData} options={forecastChartOptions} />
          </div>
        </div>
      {/if}

      {#if extras}{@render extras()}{/if}

      {#if deck && deck.total > 0}
        <div class="mt-5 pt-4 border-t border-gray-100">
          <h3 class="text-sm font-semibold text-gray-600 mb-2">Deck · {deck.total.toLocaleString()} cards</h3>
          <div class="flex h-3 rounded-full overflow-hidden bg-gray-100">
            {#each DECK_BUCKETS as b (b.key)}
              {@const n = bucketCount(deck, b.key)}
              {#if n > 0}
                <div style="width: {(n / deck.total) * 100}%; background: {b.color}" title="{n} {b.label}"></div>
              {/if}
            {/each}
          </div>
          <div class="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-sm">
            {#each DECK_BUCKETS as b (b.key)}
              {@const n = bucketCount(deck, b.key)}
              {#if b.key !== 'banished' || n > 0}
                {#if deckLinkBase && b.key !== 'banished'}
                  <a href="{deckLinkBase}{b.key}" class="text-gray-700 hover:underline">
                    <span class="inline-block w-2 h-2 rounded-full align-middle mr-1" style="background: {b.color}"></span>
                    <span class="font-bold">{n}</span> {b.label}
                  </a>
                {:else}
                  <span class="text-gray-700">
                    <span class="inline-block w-2 h-2 rounded-full align-middle mr-1" style="background: {b.color}"></span>
                    <span class="font-bold">{n}</span> {b.label}
                  </span>
                {/if}
              {/if}
            {/each}
          </div>
          {#if deck.delta}
            <p class="mt-1.5 text-xs text-gray-400">
              Since {fmtDate(deck.delta.since)}:
              mastered {fmtDelta(deck.delta.mastered)} · struggling {fmtDelta(deck.delta.struggling)}
            </p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if afterSummary}{@render afterSummary()}{/if}

  <!-- Accuracy tiles + readiness slot -->
  {#if cold30d && cold && review}
    <div class="flex flex-wrap gap-4 mb-8">
      <div class="flex-[2] min-w-[240px] bg-white rounded-xl shadow p-6 border-2 {accent === 'gold' ? 'border-jeopardy-gold' : 'border-jeopardy-blue'}">
        <p class="text-sm font-medium text-gray-500 mb-1">Cold Accuracy — last 30 days</p>
        {#if hasHistory}
          <p class="text-4xl font-extrabold {cold30d.accuracy >= 70 ? 'text-green-600' : cold30d.accuracy >= 55 ? 'text-amber-500' : 'text-red-500'}">
            {cold30d.total > 0 ? `${cold30d.accuracy.toFixed(1)}%` : '—'}
          </p>
          <p class="text-xs text-gray-400 mt-1">
            First-attempt {unitLabel} only ({cold30d.total}) — the number the Anytime Test measures. All-time: {cold.accuracy.toFixed(1)}%.
          </p>
        {:else}
          <p class="text-4xl font-extrabold text-gray-300">—</p>
          <p class="text-xs text-gray-400 mt-1">
            {historySince ? `Tracking since ${fmtDate(historySince.slice(0, 10))}.` : 'Tracking starts with your next drill.'}
          </p>
        {/if}
      </div>
      <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
        <p class="text-sm font-medium text-gray-500 mb-1">Retention (review accuracy)</p>
        {#if hasHistory && review.total > 0}
          <p class="text-3xl font-bold text-jeopardy-blue">{review.accuracy.toFixed(1)}%</p>
          <p class="text-xs text-gray-400 mt-1">{review.total.toLocaleString()} {reviewLabel}</p>
        {:else}
          <p class="text-3xl font-bold text-gray-300">—</p>
          <p class="text-xs text-gray-400 mt-1">No reviews logged yet.</p>
        {/if}
      </div>
      {#if readiness}{@render readiness()}{/if}
    </div>
  {/if}

  {#if belowTiles}{@render belowTiles()}{/if}

  <!-- Accuracy chart -->
  {#if lineChartData}
    <div class="bg-white rounded-xl shadow p-6 mb-8">
      <h3 class="text-lg font-semibold text-gray-800 mb-4">Accuracy — last 30 days</h3>
      <div style="height: 300px;">
        <StatsChart type="line" data={lineChartData} options={lineChartOptions} />
      </div>
    </div>
  {:else}
    <div class="bg-white rounded-xl shadow p-6 mb-8 text-center text-gray-400">No daily performance data yet.</div>
  {/if}

  <!-- Category chart -->
  {#if barChartData}
    <div class="bg-white rounded-xl shadow p-6 mb-8">
      <h3 class="text-lg font-semibold text-gray-800 mb-4">Category Performance</h3>
      <div style="height: {barChartHeight}px;">
        <StatsChart type="bar" data={barChartData} options={barChartOptions} />
      </div>
    </div>
  {:else}
    <div class="bg-white rounded-xl shadow p-6 mb-8 text-center text-gray-400">No category data yet.</div>
  {/if}

  <!-- Category table -->
  {#if sortedCategories.length > 0}
    <div class="bg-white rounded-xl shadow p-6">
      <h3 class="text-lg font-semibold text-gray-800 mb-4">Category Breakdown</h3>
      <div class="overflow-x-auto">
        <table class="min-w-full text-sm">
          <thead>
            <tr class="border-b border-gray-200">
              <th class="text-left py-3 px-2 sm:px-4 font-semibold text-gray-600">Category</th>
              <th class="hidden sm:table-cell text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Total</th>
              <th class="hidden sm:table-cell text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Correct</th>
              <th class="text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Cold</th>
              <th class="text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Review</th>
            </tr>
          </thead>
          <tbody>
            {#each sortedCategories as cat (cat.category)}
              <tr class="border-b border-gray-100 hover:bg-gray-50 transition-colors">
                <td class="py-3 px-2 sm:px-4 text-gray-800">{cat.category}</td>
                <td class="hidden sm:table-cell py-3 px-2 sm:px-4 text-right text-gray-600">{cat.total}</td>
                <td class="hidden sm:table-cell py-3 px-2 sm:px-4 text-right text-gray-600">{cat.correct}</td>
                <td class="py-3 px-2 sm:px-4 text-right font-medium {cat.coldAccuracy >= 70 ? 'text-green-600' : cat.coldAccuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
                  {cat.coldTotal > 0 ? `${cat.coldAccuracy.toFixed(1)}% (${cat.coldTotal})` : '—'}
                </td>
                <td class="py-3 px-2 sm:px-4 text-right text-gray-600">
                  {cat.reviewTotal > 0 ? `${cat.reviewAccuracy.toFixed(1)}% (${cat.reviewTotal})` : '—'}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}
</section>
```

- [ ] **Step 4: Type-check**

Run: `cd frontend && npm run check 2>&1 | tail -5`
Expected: 0 errors. (Unused-component warnings are fine; they get used in Task 6.)

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/stats.ts frontend/src/lib/components/StatsSection.svelte frontend/src/lib/components/PavlovProgressCard.svelte
git commit -m "feat(dashboard): shared StatsSection component, stats types, Pavlov progress card

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Rebuild the dashboard as two sections

**Files:**
- Rewrite: `frontend/src/routes/dashboard/+page.svelte` (currently 573 lines; replace entire file)

**Interfaces:**
- Consumes: `StatsSection` props/snippets and `PavlovProgressCard` from Task 5; `/api/stats`, `/api/practice/status`, `/api/pavlov/stats` (Task 4), `/api/blindspots`.

- [ ] **Step 1: Replace the dashboard file**

Overwrite `frontend/src/routes/dashboard/+page.svelte` with:

```svelte
<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';
  import StatsSection from '$lib/components/StatsSection.svelte';
  import PavlovProgressCard from '$lib/components/PavlovProgressCard.svelte';
  import type { CategoryStat, DailyStat, DeckStats, ForecastDay, KindStat, PavlovStats } from '$lib/stats';

  interface Stats {
    overall: KindStat;
    cold: KindStat;
    review: KindStat;
    cold30d: KindStat;
    mockReadiness: {
      tests: Array<{ id: number; completedAt: string; score: number }>;
      best: number | null;
      latest: number | null;
      passLine: number;
    };
    projectedMock: {
      score: number;
      passLine: number;
      categories: Array<{
        category: string;
        share: number;
        coldAccuracy: number;
        contribution: number;
        headroom: number;
        estimated: boolean;
      }>;
    };
    categoryBreakdown: CategoryStat[];
    dailyAccuracy: DailyStat[];
  }

  interface SrsStatus {
    dueCount: number;
    newRemaining: number;
    reviewedToday: number;
    forecast: ForecastDay[];
    adaptiveWeights?: Array<{ category: string; attempts: number; accuracy: number; weight: number }>;
    adaptiveWindow?: '180d' | 'all' | null;
    deck?: DeckStats;
  }

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let stats = $state<Stats | null>(null);
  let srs = $state<SrsStatus | null>(null);
  let pavlov = $state<PavlovStats | null>(null);
  let loading = $state(true);
  let error = $state('');

  // Phone layout below Tailwind's `sm` breakpoint. SSR sees 0 → desktop
  // config; corrected on hydration (charts are client-only anyway).
  let innerWidth = $state(0);
  let isMobile = $derived(innerWidth > 0 && innerWidth < 640);

  let blindspots = $state<{
    packs: Array<{ id: number; theme: string; diagnosis: string }>;
    insufficientData: boolean;
    configured: boolean;
  } | null>(null);

  onMount(async () => {
    api.get('/api/practice/status').then((s) => (srs = s)).catch(() => (srs = null));
    api.get('/api/pavlov/stats').then((s) => (pavlov = s)).catch(() => (pavlov = null));
    api.get('/api/blindspots').then((b) => (blindspots = b)).catch(() => (blindspots = null));
    try {
      stats = await api.get('/api/stats');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load stats';
    } finally {
      loading = false;
    }
  });

  // Top 3 categories with the most projected-score upside, excluding ones with
  // no cold data yet (estimated at a neutral 0.5 — nothing actionable to show).
  let topHeadroom = $derived(
    stats?.projectedMock ? stats.projectedMock.categories.filter((c) => !c.estimated).slice(0, 3) : []
  );
</script>

<svelte:head>
  <title>Dashboard — Jeopardy! Training</title>
</svelte:head>

<svelte:window bind:innerWidth />

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-6xl mx-auto">
    <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4 mb-8">
      <h1 class="text-3xl font-bold text-jeopardy-blue">Dashboard</h1>
    </div>

    <!-- ============================ Pavlov ============================ -->
    {#if pavlov && pavlov.progress.deckTotal > 0}
      <StatsSection
        title="Pavlov"
        accent="gold"
        summary={{ due: pavlov.dueCount, newLeft: pavlov.newRemaining, reviewedToday: pavlov.reviewedToday, href: '/pavlov', label: 'Pavlov Drill' }}
        forecast={pavlov.forecast}
        deck={pavlov.deck}
        cold30d={pavlov.cold30d}
        cold={pavlov.cold}
        review={pavlov.review}
        unitLabel="cue cards"
        reviewLabel="cue reviews"
        historySince={pavlov.historySince}
        daily={pavlov.dailyAccuracy}
        categories={pavlov.categoryBreakdown}
        {isMobile}
      >
        {#snippet readiness()}
          <PavlovProgressCard progress={pavlov!.progress} />
        {/snippet}
      </StatsSection>
    {/if}

    <!-- ======================= Standard Practice ======================= -->
    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>
    {:else if stats}
      <StatsSection
        title="Standard Practice"
        accent="blue"
        summary={srs ? { due: srs.dueCount, newLeft: srs.newRemaining, reviewedToday: srs.reviewedToday, href: '/practice', label: 'Practice' } : null}
        forecast={srs?.forecast ?? null}
        deck={srs?.deck ?? null}
        deckLinkBase="/cards?state="
        cold30d={stats.cold30d}
        cold={stats.cold}
        review={stats.review}
        unitLabel="questions"
        reviewLabel="SRS reviews"
        daily={stats.dailyAccuracy}
        categories={stats.categoryBreakdown}
        {isMobile}
      >
        {#snippet extras()}
          {#if srs?.adaptiveWeights && srs.adaptiveWeights.length > 0}
            {@const maxWeight = Math.max(...srs.adaptiveWeights.map((w) => w.weight))}
            <div class="mt-5 pt-4 border-t border-gray-100">
              <h3 class="text-sm font-semibold text-gray-600 mb-1">Focus areas</h3>
              <p class="text-xs text-gray-400 mb-3">
                Practice draws new clues where they're worth the most test points — weakness
                weighted by each category's share of the real Anytime Test. The bar and percentage
                show each category's share of your new clues, highest priority first.
              </p>
              <div class="flex flex-col gap-2.5 sm:gap-1.5">
                {#each srs.adaptiveWeights as w (w.category)}
                  <div class="flex flex-wrap sm:flex-nowrap items-center gap-x-3 gap-y-1 text-sm">
                    <span class="order-1 flex-1 sm:flex-none sm:w-52 truncate text-gray-700">{w.category}</span>
                    <span class="order-2 sm:order-3 shrink-0 sm:w-32 text-right text-xs text-gray-400">
                      {w.attempts > 0 ? `${Math.round(w.accuracy)}% right` : 'untried'} · {w.attempts} tries
                    </span>
                    <div class="order-3 sm:order-2 flex items-center gap-2 w-full sm:w-auto sm:flex-1">
                      <div class="flex-1 h-2 bg-gray-100 rounded-full overflow-hidden">
                        <div class="h-full bg-jeopardy-blue rounded-full" style="width: {maxWeight > 0 ? (w.weight / maxWeight) * 100 : 0}%"></div>
                      </div>
                      <span class="w-10 shrink-0 text-right text-xs font-semibold text-gray-600">{Math.round(w.weight * 100)}%</span>
                    </div>
                  </div>
                {/each}
              </div>
              <p class="mt-2 text-[11px] text-gray-400">
                "% right" counts every attempt (first tries and reviews)
                {srs.adaptiveWindow === 'all' ? 'across all time' : 'over the last 180 days'}, so it can
                differ from the cold-accuracy table below. Ranking discounts small samples — a category
                with a few bad tries sits below one that misses often over many.
              </p>
            </div>
          {/if}
        {/snippet}

        {#snippet afterSummary()}
          {#if blindspots && blindspots.configured}
            <a
              href="/blindspots"
              class="bg-white rounded-xl shadow-sm p-5 mb-8 flex items-center justify-between hover:bg-gray-50 transition-colors group block"
            >
              <div>
                <p class="font-semibold text-gray-800">Blind spots</p>
                {#if blindspots.packs.length > 0}
                  <p class="text-sm text-gray-500 mt-0.5">{blindspots.packs.slice(0, 3).map((p) => p.theme).join(' · ')}</p>
                {:else if blindspots.insufficientData}
                  <p class="text-sm text-gray-500 mt-0.5">Keep practicing — analysis unlocks after a few more misses.</p>
                {:else}
                  <p class="text-sm text-gray-500 mt-0.5">Analyze your recent misses for patterns.</p>
                {/if}
              </div>
              <span class="text-gray-400 group-hover:text-gray-600 text-lg">&rarr;</span>
            </a>
          {/if}
          <div class="flex flex-wrap gap-3 mb-8">
            <a href="/practice" class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors">Practice</a>
            <a href="/drill" class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors">Drill</a>
            <a href="/coryat" class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors">Coryat</a>
          </div>
        {/snippet}

        {#snippet readiness()}
          <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
            <p class="text-sm font-medium text-gray-500 mb-1">Mock Test Readiness</p>
            {#if stats!.mockReadiness.tests.length > 0}
              <p class="text-3xl font-bold {(stats!.mockReadiness.latest ?? 0) >= stats!.mockReadiness.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
                {stats!.mockReadiness.latest}/50
              </p>
              <p class="text-xs text-gray-400 mt-1">Best {stats!.mockReadiness.best}/50 · pass line {stats!.mockReadiness.passLine} · <a href="/mock" class="text-jeopardy-blue hover:underline">take another →</a></p>
            {:else}
              <p class="text-sm text-gray-500 mt-1">No mocks yet.</p>
              <a href="/mock" class="text-sm font-semibold text-jeopardy-blue hover:underline">Take your first mock test →</a>
            {/if}
          </div>
        {/snippet}

        {#snippet belowTiles()}
          {#if stats!.projectedMock}
            <div class="bg-white rounded-xl shadow p-6 mb-8">
              <p class="text-sm font-medium text-gray-500 mb-1">Projected Anytime Test Score</p>
              <p class="text-4xl font-extrabold {stats!.projectedMock.score >= 35 ? 'text-green-600' : stats!.projectedMock.score >= 30 ? 'text-amber-500' : 'text-red-500'}">
                {stats!.projectedMock.score.toFixed(1)}/50
              </p>
              <p class="text-xs text-gray-400 mt-1">
                Modeled from each category's cold accuracy weighted by its share of the real test · pass line {stats!.projectedMock.passLine}.
              </p>
              {#if topHeadroom.length > 0}
                <div class="mt-4 pt-3 border-t border-gray-100 flex flex-col gap-1.5">
                  <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Biggest opportunities</p>
                  {#each topHeadroom as c (c.category)}
                    <div class="flex items-center justify-between text-sm">
                      <span class="text-gray-700 truncate">{c.category}</span>
                      <span class="font-semibold text-jeopardy-blue shrink-0 ml-3">+{c.headroom.toFixed(1)} pts available</span>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/snippet}
      </StatsSection>
    {/if}
  </div>
</div>
```

Note: `pavlov!` / `stats!` inside snippets are TypeScript non-null assertions; the enclosing `{#if}` guarantees them but Svelte's narrowing doesn't flow into snippets.

- [ ] **Step 2: Type-check and build**

Run: `cd frontend && npm run check 2>&1 | tail -5 && npm run build 2>&1 | tail -3`
Expected: 0 errors; build completes.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/routes/dashboard/+page.svelte
git commit -m "feat(dashboard): two sections — Pavlov first with full stats parity, Standard Practice below

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Visual verification with a mock API

**Files:**
- Create: `scripts/dev-mock-api.mjs`

**Interfaces:**
- Consumes: `frontend/vite.config.ts` honors `VITE_API_PROXY`.

There are no dev credentials and the only real backend is production, so verification runs the SvelteKit dev server against a small mock. `localhost:3000` is contested on this machine by other projects; bind the mock to `127.0.0.1:3999`.

- [ ] **Step 1: Write the mock server**

Create `scripts/dev-mock-api.mjs`:

```js
#!/usr/bin/env node
// Mock API for verifying dashboard changes locally (no credentials needed).
// Usage:  node scripts/dev-mock-api.mjs            (port 3999)
//         MOCK_EMPTY=1 node scripts/dev-mock-api.mjs   (Pavlov with no review history)
// Then:   cd frontend && VITE_API_PROXY=http://127.0.0.1:3999 npm run dev
import http from 'node:http';

const PORT = Number(process.env.PORT ?? 3999);
const EMPTY = process.env.MOCK_EMPTY === '1';

const CATS = [
  'Literature & Language', 'Geography & Exploration', 'History & Politics', 'Science & Nature',
  'Film, TV & Pop Culture', 'Philosophy, Religion & Society', 'Music & Performing Arts',
  'Miscellaneous', 'Technology & Engineering', 'Mathematics & Logic', 'Business & Economics',
  'Sports & Games', 'Art & Culture',
];
const kind = (total, correct) => ({ total, correct, accuracy: total ? (correct / total) * 100 : 0 });
const split = (c, t, cc, ct, rt, rc) => ({
  category: c, total: t, correct: cc, accuracy: (cc / t) * 100,
  coldTotal: ct, coldCorrect: Math.round(ct * 0.5), coldAccuracy: 50 + (c.length % 30),
  reviewTotal: rt, reviewCorrect: rc, reviewAccuracy: (rc / rt) * 100,
});
const isoDay = (offset) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};
const daily = (n) =>
  Array.from({ length: n }, (_, i) => ({
    date: isoDay(i - n + 1), total: 40, correct: 28, accuracy: 70,
    coldTotal: 15, coldCorrect: 8, coldAccuracy: 45 + ((i * 7) % 25),
    reviewTotal: 25, reviewCorrect: 22, reviewAccuracy: 80 + ((i * 3) % 15),
  }));
const forecast = (base) => Array.from({ length: 14 }, (_, i) => ({ date: isoDay(i), count: base + ((i * 37) % 60) }));

const routes = {
  '/api/auth/me': { user: { id: 1, username: 'ebertx', email: 'ebertx@example.com', role: 'admin' } },
  '/api/blindspots': { configured: true, insufficientData: false, packs: [
    { id: 1, theme: 'Mythology', diagnosis: '' }, { id: 2, theme: 'Opera', diagnosis: '' }, { id: 3, theme: 'Fine Arts', diagnosis: '' } ] },
  '/api/stats': {
    overall: kind(4321, 3300), cold: kind(2100, 1050), review: kind(2221, 1800), cold30d: kind(220, 118),
    mockReadiness: { tests: [{ id: 1, completedAt: '2026-07-20T00:00:00Z', score: 18 }, { id: 2, completedAt: '2026-07-23T00:00:00Z', score: 21 }], best: 21, latest: 21, passLine: 35 },
    projectedMock: { score: 27.4, passLine: 35, categories: CATS.map((c, i) => ({ category: c, share: 0.08, coldAccuracy: 45 + i, contribution: 2, headroom: 3 - i * 0.2, estimated: false })) },
    categoryBreakdown: CATS.map((c) => split(c, 300, 200, 150, 150, 120)),
    dailyAccuracy: daily(30),
  },
  '/api/practice/status': {
    dueCount: 42, newRemaining: 30, reviewedToday: 18, forecast: forecast(20),
    adaptiveWeights: CATS.slice(0, 6).map((c, i) => ({ category: c, attempts: 300 - i * 30, accuracy: 45 + i * 5, weight: 0.3 - i * 0.04 })),
    adaptiveWindow: '180d',
    deck: { learning: 120, maturing: 900, mastered: 1400, struggling: 60, total: 2480, delta: { since: isoDay(-7), learning: 5, maturing: 12, mastered: 18, struggling: -2 } },
  },
  '/api/pavlov/stats': EMPTY
    ? {
        overall: kind(0, 0), cold: kind(0, 0), review: kind(0, 0), cold30d: kind(0, 0), historySince: null,
        dailyAccuracy: [], categoryBreakdown: [], reviewedToday: 0, dueCount: 124, newRemaining: 40, forecast: forecast(30),
        deck: { learning: 24, maturing: 402, mastered: 585, struggling: 0, banished: 86, total: 1097, delta: null },
        progress: { deckTotal: 4765, touched: 1097, touchedPct: 23.02, trailingPerDay: 11, targetDate: '2026-12-31', daysLeft: 108, requiredPerDay: 34, pastTarget: false, projectedFinish: '2027-08-15', daysAhead: -226 },
      }
    : {
        overall: kind(1200, 960), cold: kind(400, 180), review: kind(800, 780), cold30d: kind(400, 180), historySince: '2026-09-16T04:00:00Z',
        dailyAccuracy: daily(12), categoryBreakdown: CATS.map((c) => split(c, 90, 70, 30, 60, 55)),
        reviewedToday: 96, dueCount: 124, newRemaining: 40, forecast: forecast(30),
        deck: { learning: 24, maturing: 402, mastered: 585, struggling: 3, banished: 86, total: 1100, delta: { since: isoDay(-7), learning: -10, maturing: 40, mastered: 55, struggling: 1 } },
        progress: { deckTotal: 4765, touched: 1100, touchedPct: 23.08, trailingPerDay: 36, targetDate: '2026-12-31', daysLeft: 108, requiredPerDay: 34, pastTarget: false, projectedFinish: '2026-12-26', daysAhead: 6 },
      },
};

http
  .createServer((req, res) => {
    const path = req.url.split('?')[0];
    const body = routes[path];
    res.setHeader('Content-Type', 'application/json');
    if (!body) {
      res.statusCode = 404;
      return res.end(JSON.stringify({ error: `no mock for ${path}` }));
    }
    res.end(JSON.stringify(body));
  })
  .listen(PORT, '127.0.0.1', () => console.log(`mock api on http://127.0.0.1:${PORT} (empty=${EMPTY})`));
```

- [ ] **Step 2: Run mock + dev server, screenshot both widths and the empty state**

Terminal 1: `node scripts/dev-mock-api.mjs`
Terminal 2: `cd frontend && VITE_API_PROXY=http://127.0.0.1:3999 npm run dev` (note the port Vite prints, usually 5173).

Screenshot `http://localhost:5173/dashboard` at 1280 wide and at 390×844 using the Playwright MCP browser tools (`browser_navigate`, `browser_resize`, `browser_take_screenshot`), or:

```bash
npx playwright screenshot --viewport-size=1280,900 --full-page http://localhost:5173/dashboard /tmp/dash-desktop.png
npx playwright screenshot --viewport-size=390,844 --full-page http://localhost:5173/dashboard /tmp/dash-phone.png
```

Then restart the mock with `MOCK_EMPTY=1 node scripts/dev-mock-api.mjs` and screenshot desktop again.

- [ ] **Step 3: Check the screenshots against this list**

- Pavlov section renders above Standard Practice with a gold marker in the header.
- Pavlov summary strip shows Due 124 / New left 40 / Reviewed today 96 and a gold "Pavlov Drill →" button.
- Deck bar shows five swatches including gray "banished 86"; delta line under it.
- Deck-progress tile shows "1,100 / 4,765 23%", the bar, and a green "34/day … trailing 36/day … projected Dec 26" line.
- Empty state (MOCK_EMPTY=1): cold tile shows "—" with "Tracking starts with your next drill."; accuracy chart area shows "No daily performance data yet."; deck bar and progress card still populated.
- Standard section is unchanged from before: focus areas, deck bar with links, blind spots, action buttons, cold/retention/mock tiles, projected score, charts, table.
- Phone width: no horizontal page scroll; category chart is horizontal bars; forecast shows 7 days.

Fix anything off in `StatsSection.svelte` / `PavlovProgressCard.svelte` / dashboard and re-screenshot.

- [ ] **Step 4: Commit**

```bash
git add scripts/dev-mock-api.mjs
git commit -m "chore(dev): mock API for local dashboard verification (incl. empty Pavlov history)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

If Step 3 required fixes, commit those too with a `fix(dashboard): …` message.

---

### Task 8: Rollout

**Files:** none new. Uses `scripts/apply-migration.sh`, `scripts/verify-pavlov-stats.sql`.

Deploy is push-to-`main`; GitHub Actions builds the image and Watchtower on Tower restarts the container. The migration MUST be applied before the push. Pushing is outward-facing: confirm with Christian before Step 3 unless he has already said to ship.

- [ ] **Step 1: Apply migration 0013 to the live DB**

```bash
scripts/apply-migration.sh backend/migrations/0013_pavlov_reviews.sql
```

Expected: `CREATE TABLE`, `CREATE INDEX` ×2, `CREATE TABLE`, `ALTER TABLE` with no errors. The running container ignores the new tables until the new image lands.

- [ ] **Step 2: Run the SQL verify script**

```bash
DB_URL=$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')
docker run --rm -i postgres:16 psql "$DB_URL" -P pager=off -f - < scripts/verify-pavlov-stats.sql
```

Expected: section A shows both tables with 0 rows and `pavlov_target_date = 2026-12-31`; every `fail_rows` is 0; section E bucket sum equals total (1,097 as of 2026-09-15, higher after more drilling).

- [ ] **Step 3: Push main (after confirmation)**

```bash
git push origin main
```

Then watch the rollout:

```bash
ssh root@tower 'docker logs --tail 50 watchtower | grep -i jeopardy; docker logs --tail 20 jeopardy-server'
```

Expected: Watchtower pulls the new image within a few minutes; server log shows a clean start.

- [ ] **Step 4: Live smoke test**

1. Open the dashboard in the browser: Pavlov section on top, progress card shows real numbers, accuracy tiles show the empty state.
2. Open `/pavlov`, grade one card, return to the dashboard: "Reviewed today" is 1, cold tile shows a percentage.
3. Re-run the verify script from Step 2: `pavlov_reviews` has ≥1 row and section G checks are 0.
4. Open `/settings`, change the Pavlov target date, save, reload the dashboard: the progress line reflects the new date.

- [ ] **Step 5: Update memory**

Append to `/Users/atropos/.claude/projects/-Users-atropos-ai-jeopardy-jeopardy-training-app/memory/project_jeopardy_test_prep.md` an `UPDATE <date>` line: Pavlov dashboard section deployed (commit hash), migration 0013 applied, review log starts <date>; and refresh the `MEMORY.md` hook for that entry.

---

## Self-review

**Spec coverage:** §1 data → Task 1 (tables, column, grade insert) and Task 2 (preference plumbing). §2 API → Tasks 3 and 4 (every listed field is emitted: overall/cold/review/cold30d, dailyAccuracy, categoryBreakdown, reviewedToday, dueCount, newRemaining, forecast, deck incl. banished + delta, progress incl. pastTarget, historySince). §3 UI → Tasks 5 and 6 (two sections Pavlov-first, gold accent, summary strip, progress card in readiness slot with pace colors and settings link, empty-state captions, five-swatch deck bar with plain labels for Pavlov and links for standard, 7/14-day forecast, charts, table; Settings date input in Task 2). §4 testing → Task 3 unit tests; Task 1 `is_first_grade` tests; DB behaviour via `scripts/verify-pavlov-stats.sql` (Task 4, run in Task 8) since the repo has no DB-backed Rust tests; Task 7 screenshots incl. empty state. §5 rollout → Task 8. Known gap is stated in the empty-state copy.

**Type consistency:** `Progress` serializes camelCase (`deckTotal`, `touchedPct`, `trailingPerDay`, `targetDate`, `daysLeft`, `requiredPerDay`, `pastTarget`, `projectedFinish`, `daysAhead`) and `PavlovProgress` in `stats.ts` uses exactly those names. `StatsSection` props used in Task 6 (`summary`, `forecast`, `deck`, `deckLinkBase`, `cold30d`, `cold`, `review`, `unitLabel`, `reviewLabel`, `historySince`, `daily`, `categories`, `isMobile`) and snippets (`extras`, `afterSummary`, `readiness`, `belowTiles`) match the Task 5 declaration. `is_first_grade` and the `last_review` field are used consistently between Task 1's test and implementation.
