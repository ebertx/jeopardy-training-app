# SRS Practice Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the naive `N-consecutive-correct` mastery model with a spaced-repetition scheduler that drives one unified daily "Practice" session (due reviews + a capped trickle of new clues).

**Architecture:** A pure, dependency-free Rust scheduler module (`srs.rs`) owns all interval math and is unit-tested without a database. Axum handlers in `routes/practice.rs` persist per-user card state in a new `srs_cards` table and call the scheduler. The SvelteKit `/quiz` route becomes `/practice`, swapping its 2-button grade for 3 buttons and its `/api/quiz/*` calls for `/api/practice/*`. The scheduler internals are SM-2-derived but hidden behind an owned interface so they can later be swapped for the `fsrs` crate without touching callers.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8 with runtime-checked `query_as`, chrono, chrono-tz), PostgreSQL, SvelteKit (Svelte 5 runes), TailwindCSS.

## Global Constraints

- Rust edition 2021; axum 0.8; sqlx 0.8 using runtime-checked `sqlx::query`/`query_as::<_, T>` (NOT the compile-time `query!` macros — no live DB needed at build time).
- **Never run tests or migrations against the shared production database.** DB/API verification uses a local throwaway database only (a `SCRATCH_DATABASE_URL` you control).
- Schedulers must be deterministic (no randomness, no wall-clock inside pure logic) so unit tests are stable.
- Frontend uses Svelte 5 runes (`$state`, `$derived`, `$props`, `$effect`) and Tailwind utility classes — match existing files.
- Grade ratings on the wire are the strings `"wrong" | "got_it" | "too_easy"`; the client never sends algorithm integers.
- Derived definitions: **Mastered** = `state = 'review' AND interval_days >= 21`. **Leech** = `lapses >= 8` → `suspended = true`.
- One new backend dependency is allowed: `chrono-tz` (pure, for IANA timezone day boundaries). No other new deps.

---

### Task 1: Database migration — `srs_cards` table + user preference columns

**Files:**
- Create: `backend/migrations/0001_srs_cards.sql`
- Reference (keep in sync, no behavior): `prisma/schema.prisma`

**Interfaces:**
- Produces: table `srs_cards(id, user_id, question_id, state, interval_days, ease, due, last_review, reps, lapses, step_index, suspended, created_at)` with unique `(user_id, question_id)`; columns `users.new_cards_per_day int default 20`, `users.timezone text`.

- [ ] **Step 1: Write the migration SQL**

Create `backend/migrations/0001_srs_cards.sql`:

```sql
-- Spaced-repetition card state, one row per (user, clue), created on first sight.
CREATE TABLE IF NOT EXISTS srs_cards (
    id            SERIAL PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    question_id   INTEGER NOT NULL REFERENCES jeopardy_questions(id),
    state         TEXT NOT NULL DEFAULT 'learning',   -- learning | review | relearning
    interval_days DOUBLE PRECISION NOT NULL DEFAULT 0, -- current review interval (memory strength)
    ease          DOUBLE PRECISION NOT NULL DEFAULT 2.5, -- SM-2 ease factor
    due           TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_review   TIMESTAMPTZ,
    reps          INTEGER NOT NULL DEFAULT 0,
    lapses        INTEGER NOT NULL DEFAULT 0,
    step_index    SMALLINT NOT NULL DEFAULT 0,
    suspended     BOOLEAN NOT NULL DEFAULT false,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, question_id)
);

CREATE INDEX IF NOT EXISTS idx_srs_cards_user_due ON srs_cards (user_id, due);
CREATE INDEX IF NOT EXISTS idx_srs_cards_user_suspended_due ON srs_cards (user_id, suspended, due);
CREATE INDEX IF NOT EXISTS idx_srs_cards_user_created ON srs_cards (user_id, created_at);

ALTER TABLE users ADD COLUMN IF NOT EXISTS new_cards_per_day INTEGER NOT NULL DEFAULT 20;
ALTER TABLE users ADD COLUMN IF NOT EXISTS timezone TEXT;
```

- [ ] **Step 2: Apply to a local scratch database and verify**

Run (against a throwaway DB — NOT production):

```bash
psql "$SCRATCH_DATABASE_URL" -f backend/migrations/0001_srs_cards.sql
psql "$SCRATCH_DATABASE_URL" -c "\d srs_cards"
psql "$SCRATCH_DATABASE_URL" -c "\d users" | grep -E "new_cards_per_day|timezone"
```

Expected: `\d srs_cards` lists all columns and the three indexes; the `users` grep shows both new columns.

- [ ] **Step 3: Mirror the schema in `prisma/schema.prisma` (documentation parity)**

Add a `srs_cards` model and the two `users` fields to `prisma/schema.prisma` so the legacy schema file stays accurate. This is documentation only; Prisma is not the runtime.

Add inside `model users { ... }`:

```prisma
  new_cards_per_day     Int                      @default(20)
  timezone              String?
  srs_cards             srs_cards[]
```

Add a new model:

```prisma
model srs_cards {
  id            Int                @id @default(autoincrement())
  user_id       Int
  question_id   Int
  state         String             @default("learning")
  interval_days Float              @default(0)
  ease          Float              @default(2.5)
  due           DateTime           @default(now()) @db.Timestamptz(6)
  last_review   DateTime?          @db.Timestamptz(6)
  reps          Int                @default(0)
  lapses        Int                @default(0)
  step_index    Int                @default(0) @db.SmallInt
  suspended     Boolean            @default(false)
  created_at    DateTime           @default(now()) @db.Timestamptz(6)
  user          users              @relation(fields: [user_id], references: [id], onDelete: Cascade)
  question      jeopardy_questions @relation(fields: [question_id], references: [id])

  @@unique([user_id, question_id])
  @@index([user_id, due])
  @@index([user_id, suspended, due])
  @@index([user_id, created_at])
}
```

Add to `model jeopardy_questions { ... }`: `srs_cards            srs_cards[]`

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0001_srs_cards.sql prisma/schema.prisma
git commit -m "feat(srs): add srs_cards table and user preference columns"
```

---

### Task 2: Pure scheduler module `srs.rs` (TDD core)

**Files:**
- Create: `backend/src/srs.rs`
- Modify: `backend/src/main.rs` (add `mod srs;` near the other `mod` lines, e.g. after `mod models;`)
- Test: inline `#[cfg(test)]` module in `backend/src/srs.rs`

**Interfaces:**
- Produces:
  - `enum Rating { Wrong, GotIt, TooEasy }` with `Rating::from_wire(&str) -> Option<Rating>`
  - `enum CardKind { Learning, Review, Relearning }` with `as_str()/from_str`
  - `struct Prev { pub state: CardKind, pub interval_days: f64, pub ease: f64, pub reps: i32, pub lapses: i32, pub step_index: i16 }`
  - `struct Outcome { pub state: CardKind, pub interval_days: f64, pub ease: f64, pub reps: i32, pub lapses: i32, pub step_index: i16, pub interval_secs: i64, pub requeue_in_session: bool }`
  - `fn schedule(prev: Option<Prev>, rating: Rating) -> Outcome`

- [ ] **Step 1: Write failing tests**

Create `backend/src/srs.rs` with the test module first (types referenced don't exist yet, so it won't compile — that's the failing state):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn new_card_wrong_stays_learning_step0_requeues() {
        let o = schedule(None, Rating::Wrong);
        assert!(matches!(o.state, CardKind::Learning));
        assert_eq!(o.step_index, 0);
        assert_eq!(o.interval_secs, 60);
        assert!(o.requeue_in_session);
        assert_eq!(o.reps, 0);
    }

    #[test]
    fn new_card_gotit_advances_to_second_learning_step() {
        let o = schedule(None, Rating::GotIt);
        assert!(matches!(o.state, CardKind::Learning));
        assert_eq!(o.step_index, 1);
        assert_eq!(o.interval_secs, 600);
        assert!(o.requeue_in_session);
    }

    #[test]
    fn new_card_tooeasy_graduates_to_review_four_days() {
        let o = schedule(None, Rating::TooEasy);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 4.0);
        assert_eq!(o.interval_secs, 4 * DAY);
        assert!(!o.requeue_in_session);
        assert_eq!(o.reps, 1);
    }

    #[test]
    fn gotit_on_last_learning_step_graduates_one_day() {
        let prev = Prev { state: CardKind::Learning, interval_days: 0.0, ease: 2.5, reps: 0, lapses: 0, step_index: 1 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 1.0);
        assert_eq!(o.interval_secs, DAY);
        assert!(!o.requeue_in_session);
        assert_eq!(o.reps, 1);
    }

    #[test]
    fn review_gotit_multiplies_interval_by_ease() {
        let prev = Prev { state: CardKind::Review, interval_days: 10.0, ease: 2.5, reps: 3, lapses: 0, step_index: 0 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 25.0); // 10 * 2.5
        assert_eq!(o.ease, 2.5);
        assert_eq!(o.reps, 4);
        assert!(!o.requeue_in_session);
    }

    #[test]
    fn review_tooeasy_beats_gotit_and_raises_ease() {
        let prev = Prev { state: CardKind::Review, interval_days: 10.0, ease: 2.5, reps: 3, lapses: 0, step_index: 0 };
        let good = schedule(Some(prev.clone()), Rating::GotIt);
        let easy = schedule(Some(prev), Rating::TooEasy);
        assert!(easy.interval_days > good.interval_days);
        assert!(easy.ease > 2.5);
    }

    #[test]
    fn review_wrong_lapses_into_relearning_and_lowers_ease() {
        let prev = Prev { state: CardKind::Review, interval_days: 20.0, ease: 2.5, reps: 5, lapses: 1, step_index: 0 };
        let o = schedule(Some(prev), Rating::Wrong);
        assert!(matches!(o.state, CardKind::Relearning));
        assert_eq!(o.lapses, 2);
        assert_eq!(o.step_index, 0);
        assert_eq!(o.interval_secs, 600); // relearning step
        assert!(o.requeue_in_session);
        assert!((o.ease - 2.3).abs() < 1e-9); // 2.5 - 0.20
        assert_eq!(o.interval_days, 10.0); // shrunk: 20 * 0.5
    }

    #[test]
    fn relearning_gotit_graduates_back_to_review_at_shrunk_interval() {
        let prev = Prev { state: CardKind::Relearning, interval_days: 10.0, ease: 2.3, reps: 5, lapses: 2, step_index: 0 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 10.0);
        assert_eq!(o.interval_secs, 10 * DAY);
        assert!(!o.requeue_in_session);
    }

    #[test]
    fn ease_never_drops_below_floor() {
        let prev = Prev { state: CardKind::Review, interval_days: 5.0, ease: 1.35, reps: 4, lapses: 3, step_index: 0 };
        let o = schedule(Some(prev), Rating::Wrong);
        assert!(o.ease >= 1.3);
    }

    #[test]
    fn interval_rounds_to_whole_days_min_one() {
        let prev = Prev { state: CardKind::Review, interval_days: 1.0, ease: 1.3, reps: 1, lapses: 0, step_index: 0 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(o.interval_days >= 1.0);
        assert_eq!(o.interval_days, o.interval_days.round());
    }

    #[test]
    fn rating_from_wire_parses_known_values_only() {
        assert!(matches!(Rating::from_wire("wrong"), Some(Rating::Wrong)));
        assert!(matches!(Rating::from_wire("got_it"), Some(Rating::GotIt)));
        assert!(matches!(Rating::from_wire("too_easy"), Some(Rating::TooEasy)));
        assert!(Rating::from_wire("hard").is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (do not compile)**

Run: `cd backend && cargo test srs::`
Expected: FAIL — compile errors, `cannot find type Rating`, etc.

- [ ] **Step 3: Implement the scheduler above the test module**

Prepend to `backend/src/srs.rs` (before the `#[cfg(test)]` block):

```rust
//! Self-contained spaced-repetition scheduler (SM-2 derived, 3 ratings).
//!
//! Pure and deterministic: no DB, no wall clock, no randomness. The DB layer
//! reads `Outcome.interval_secs` to set `due = now() + interval_secs`.
//! Internals are intentionally hidden behind `schedule()` so they can later be
//! swapped for the `fsrs` crate without changing callers.

const LEARNING_STEPS_SECS: [i64; 2] = [60, 600];   // 1 min, 10 min
const RELEARNING_STEPS_SECS: [i64; 1] = [600];     // 10 min
const DAY_SECS: i64 = 86_400;

const STARTING_EASE: f64 = 2.5;
const MIN_EASE: f64 = 1.3;
const EASE_PENALTY: f64 = 0.20; // Wrong on a review card
const EASE_EASY_BONUS: f64 = 0.15; // TooEasy on a review card
const EASY_MULTIPLIER: f64 = 1.3;
const LAPSE_INTERVAL_MULT: f64 = 0.5; // shrink interval on lapse
const GRADUATING_INTERVAL_DAYS: f64 = 1.0;
const EASY_GRADUATING_INTERVAL_DAYS: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Wrong,
    GotIt,
    TooEasy,
}

impl Rating {
    pub fn from_wire(s: &str) -> Option<Rating> {
        match s {
            "wrong" => Some(Rating::Wrong),
            "got_it" => Some(Rating::GotIt),
            "too_easy" => Some(Rating::TooEasy),
            _ => None,
        }
    }
    /// Maps to the `correct` boolean recorded in question_attempts for stats.
    pub fn is_correct(self) -> bool {
        !matches!(self, Rating::Wrong)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Learning,
    Review,
    Relearning,
}

impl CardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CardKind::Learning => "learning",
            CardKind::Review => "review",
            CardKind::Relearning => "relearning",
        }
    }
    pub fn from_str(s: &str) -> CardKind {
        match s {
            "review" => CardKind::Review,
            "relearning" => CardKind::Relearning,
            _ => CardKind::Learning,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prev {
    pub state: CardKind,
    pub interval_days: f64,
    pub ease: f64,
    pub reps: i32,
    pub lapses: i32,
    pub step_index: i16,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: CardKind,
    pub interval_days: f64,
    pub ease: f64,
    pub reps: i32,
    pub lapses: i32,
    pub step_index: i16,
    pub interval_secs: i64,
    pub requeue_in_session: bool,
}

pub fn schedule(prev: Option<Prev>, rating: Rating) -> Outcome {
    let prev = prev.unwrap_or(Prev {
        state: CardKind::Learning,
        interval_days: 0.0,
        ease: STARTING_EASE,
        reps: 0,
        lapses: 0,
        step_index: 0,
    });

    match prev.state {
        CardKind::Learning | CardKind::Relearning => step_through_learning(&prev, rating),
        CardKind::Review => grade_review(&prev, rating),
    }
}

fn steps_for(kind: CardKind) -> &'static [i64] {
    match kind {
        CardKind::Relearning => &RELEARNING_STEPS_SECS,
        _ => &LEARNING_STEPS_SECS,
    }
}

fn step_through_learning(prev: &Prev, rating: Rating) -> Outcome {
    let steps = steps_for(prev.state);
    match rating {
        Rating::Wrong => Outcome {
            state: prev.state,
            interval_days: prev.interval_days,
            ease: prev.ease,
            reps: prev.reps,
            lapses: prev.lapses,
            step_index: 0,
            interval_secs: steps[0],
            requeue_in_session: true,
        },
        Rating::GotIt => {
            let next_step = prev.step_index as usize + 1;
            if next_step >= steps.len() {
                // Graduate. Relearning resumes at its (already shrunk) interval;
                // fresh learning graduates to the standard 1-day interval.
                let interval_days = if prev.state == CardKind::Relearning {
                    prev.interval_days.max(GRADUATING_INTERVAL_DAYS).round()
                } else {
                    GRADUATING_INTERVAL_DAYS
                };
                Outcome {
                    state: CardKind::Review,
                    interval_days,
                    ease: prev.ease,
                    reps: prev.reps + 1,
                    lapses: prev.lapses,
                    step_index: 0,
                    interval_secs: (interval_days as i64) * DAY_SECS,
                    requeue_in_session: false,
                }
            } else {
                Outcome {
                    state: prev.state,
                    interval_days: prev.interval_days,
                    ease: prev.ease,
                    reps: prev.reps,
                    lapses: prev.lapses,
                    step_index: next_step as i16,
                    interval_secs: steps[next_step],
                    requeue_in_session: true,
                }
            }
        }
        Rating::TooEasy => Outcome {
            state: CardKind::Review,
            interval_days: EASY_GRADUATING_INTERVAL_DAYS,
            ease: prev.ease,
            reps: prev.reps + 1,
            lapses: prev.lapses,
            step_index: 0,
            interval_secs: (EASY_GRADUATING_INTERVAL_DAYS as i64) * DAY_SECS,
            requeue_in_session: false,
        },
    }
}

fn grade_review(prev: &Prev, rating: Rating) -> Outcome {
    match rating {
        Rating::Wrong => {
            let ease = (prev.ease - EASE_PENALTY).max(MIN_EASE);
            let interval_days = (prev.interval_days * LAPSE_INTERVAL_MULT).max(1.0).round();
            Outcome {
                state: CardKind::Relearning,
                interval_days,
                ease,
                reps: prev.reps,
                lapses: prev.lapses + 1,
                step_index: 0,
                interval_secs: RELEARNING_STEPS_SECS[0],
                requeue_in_session: true,
            }
        }
        Rating::GotIt => {
            let interval_days = (prev.interval_days * prev.ease).max(1.0).round();
            Outcome {
                state: CardKind::Review,
                interval_days,
                ease: prev.ease,
                reps: prev.reps + 1,
                lapses: prev.lapses,
                step_index: 0,
                interval_secs: (interval_days as i64) * DAY_SECS,
                requeue_in_session: false,
            }
        }
        Rating::TooEasy => {
            let ease = prev.ease + EASE_EASY_BONUS;
            let interval_days = (prev.interval_days * ease * EASY_MULTIPLIER).max(1.0).round();
            Outcome {
                state: CardKind::Review,
                interval_days,
                ease,
                reps: prev.reps + 1,
                lapses: prev.lapses,
                step_index: 0,
                interval_secs: (interval_days as i64) * DAY_SECS,
                requeue_in_session: false,
            }
        }
    }
}
```

Also add `mod srs;` to `backend/src/main.rs` (after `mod models;`).

Note: the `review_tooeasy_beats_gotit` test constructs `prev` twice via `.clone()`, so `Prev` needs `Clone` (already derived above).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && cargo test srs::`
Expected: PASS — all 11 tests green.

- [ ] **Step 5: Lint and commit**

```bash
cd backend && cargo clippy --all-targets 2>&1 | tail -5
git add backend/src/srs.rs backend/src/main.rs
git commit -m "feat(srs): pure SM-2-derived scheduler with unit tests"
```

---

### Task 3: `grade` endpoint + retire mastery writes

**Files:**
- Create: `backend/src/routes/practice.rs`
- Modify: `backend/src/routes/mod.rs` (add `pub mod practice;`)
- Modify: `backend/src/main.rs` (mount route)
- Modify: `backend/src/routes/quiz.rs` (remove `question_mastery` writes from `submit`)

**Interfaces:**
- Consumes: `crate::srs::{schedule, Prev, Rating, CardKind}`, `crate::auth::middleware::AuthUser`, `crate::error::AppError`, `crate::AppState`.
- Produces: `POST /api/practice/grade` accepting `{ questionId: i32, rating: String, sessionId: Option<i32> }`, returning `{ sessionId, state, due, intervalDays, requeueInSession }`.

- [ ] **Step 1: Implement the grade handler**

Create `backend/src/routes/practice.rs`:

```rust
use axum::{extract::State, Json};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::srs::{schedule, CardKind, Prev, Rating};
use crate::AppState;

const LEECH_LAPSES: i32 = 8;

#[derive(sqlx::FromRow)]
struct CardRow {
    state: String,
    interval_days: f64,
    ease: f64,
    reps: i32,
    lapses: i32,
    step_index: i16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeBody {
    pub question_id: i32,
    pub rating: String,
    pub session_id: Option<i32>,
}

pub async fn grade(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<GradeBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let rating = Rating::from_wire(&body.rating)
        .ok_or_else(|| AppError::BadRequest("rating must be wrong|got_it|too_easy".into()))?;

    // Ensure a session row exists (mirrors quiz::submit), for question_attempts stats.
    let session_id = match body.session_id {
        Some(id) => id,
        None => {
            let row: (i32,) = sqlx::query_as(
                "INSERT INTO quiz_sessions (user_id, is_review_session) VALUES ($1, false) RETURNING id",
            )
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;
            row.0
        }
    };

    // Record the attempt for existing stats/analytics.
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct) VALUES ($1, $2, $3, $4)",
    )
    .bind(session_id)
    .bind(body.question_id)
    .bind(user_id)
    .bind(rating.is_correct())
    .execute(&state.pool)
    .await?;

    // Load prior SRS state, if any.
    let existing: Option<CardRow> = sqlx::query_as(
        "SELECT state, interval_days, ease, reps, lapses, step_index
         FROM srs_cards WHERE user_id = $1 AND question_id = $2",
    )
    .bind(user_id)
    .bind(body.question_id)
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
        "INSERT INTO srs_cards
           (user_id, question_id, state, interval_days, ease, due, last_review, reps, lapses, step_index, suspended)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (user_id, question_id) DO UPDATE SET
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
    .bind(body.question_id)
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
        "sessionId": session_id,
        "state": out.state.as_str(),
        "due": due,
        "intervalDays": out.interval_days,
        "requeueInSession": out.requeue_in_session,
    })))
}
```

- [ ] **Step 2: Register the module and route**

In `backend/src/routes/mod.rs` add: `pub mod practice;`

In `backend/src/main.rs`, inside the `api_routes` builder (near the other `.route(...)` lines), add:

```rust
        .route("/api/practice/grade", post(routes::practice::grade))
```

- [ ] **Step 3: Remove mastery writes from `quiz::submit`**

In `backend/src/routes/quiz.rs`, delete the entire block that reads `existing_mastery` and performs the three `INSERT ... question_mastery ... ON CONFLICT` branches (everything from `// Get existing mastery record` through the last mastery `.execute(&state.pool).await?;` before the final `Ok(Json(json!({`). Keep the `question_attempts` insert and the session logic. Also remove the now-unused `use crate::models::mastery::QuestionMastery;` import at the top of the file.

- [ ] **Step 4: Build, lint**

Run: `cd backend && cargo build 2>&1 | tail -5 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: builds clean; no unused-import warnings for `QuestionMastery`.

- [ ] **Step 5: Manual verification against scratch DB**

Start the server pointed at the scratch DB, register/login to get a cookie, then:

```bash
# grade a brand-new card as "got_it"
curl -s -b cookies.txt -X POST localhost:3000/api/practice/grade \
  -H 'Content-Type: application/json' \
  -d '{"questionId": 1, "rating": "got_it"}' | jq
```

Expected JSON contains `"state":"learning"`, `"requeueInSession":true`, a `sessionId`, and a `due` ~10 minutes out. Re-running with `"rating":"too_easy"` and the returned `sessionId` yields `"state":"review"` and a `due` ~4 days out. `psql "$SCRATCH_DATABASE_URL" -c "SELECT state, interval_days, due FROM srs_cards WHERE question_id=1"` shows the row.

- [ ] **Step 6: Commit**

```bash
git add backend/src/routes/practice.rs backend/src/routes/mod.rs backend/src/main.rs backend/src/routes/quiz.rs
git commit -m "feat(srs): practice/grade endpoint; retire question_mastery writes"
```

---

### Task 4: `next` endpoint (due reviews + capped new-clue picker)

**Files:**
- Modify: `backend/src/routes/practice.rs`
- Modify: `backend/src/main.rs` (mount route)
- Modify: `backend/Cargo.toml` (add `chrono-tz`)

**Interfaces:**
- Consumes: everything from Task 3.
- Produces: `GET /api/practice/next?category=&gameTypes=` returning either
  `{ done: false, isNew: bool, card: {id, question, answer, category, classifier_category, clue_value, round, air_date, notes}, dueCount: i64, newRemaining: i64 }`
  or `{ done: true, dueCount: 0, newRemaining: i64 }`.
- Produces helper `fn day_start_utc(tz: Option<&str>) -> DateTime<Utc>` reused by Task 5.

- [ ] **Step 1: Add the `chrono-tz` dependency**

In `backend/Cargo.toml`, under `[dependencies]`, add:

```toml
chrono-tz = "0.10"
```

Run: `cd backend && cargo build 2>&1 | tail -3` (fetches the crate). Expected: builds.

- [ ] **Step 2: Implement the `next` handler and timezone helper**

Append to `backend/src/routes/practice.rs`:

```rust
use axum::extract::Query;
use std::collections::HashMap;

/// Start of "today" in the user's IANA timezone, as a UTC instant.
/// Pure (takes `now`) so it can be unit-tested. Falls back to UTC midnight when
/// tz is missing or unparseable.
pub fn day_start_utc(now: DateTime<Utc>, tz: Option<&str>) -> DateTime<Utc> {
    use chrono::TimeZone;
    let zone: chrono_tz::Tz = tz.and_then(|s| s.parse().ok()).unwrap_or(chrono_tz::UTC);
    let local_now = now.with_timezone(&zone);
    let local_midnight = local_now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight");
    zone.from_local_datetime(&local_midnight)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(now)
}

#[derive(sqlx::FromRow)]
struct ClueRow {
    id: i32,
    question: Option<String>,
    answer: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    clue_value: Option<i32>,
    round: Option<i32>,
    air_date: Option<chrono::NaiveDate>,
    notes: Option<String>,
}

fn clue_json(row: ClueRow) -> Value {
    json!({
        "id": row.id,
        "question": row.question,
        "answer": row.answer,
        "category": row.category,
        "classifier_category": row.classifier_category,
        "clue_value": row.clue_value,
        "round": row.round,
        "air_date": row.air_date,
        "notes": row.notes,
    })
}

pub async fn next(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    // User prefs.
    let (new_per_day, tz): (i32, Option<String>) =
        sqlx::query_as("SELECT new_cards_per_day, timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    // Due review count (unsuspended, due now).
    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards
         WHERE user_id = $1 AND suspended = false AND due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    // New cards introduced since local midnight.
    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards WHERE user_id = $1 AND created_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    // 1) A due review card takes priority.
    let review: Option<ClueRow> = sqlx::query_as(
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
    .await?;

    if let Some(row) = review {
        return Ok(Json(json!({
            "done": false, "isNew": false, "card": clue_json(row),
            "dueCount": due_count, "newRemaining": new_remaining,
        })));
    }

    // 2) A new clue, if the daily allowance remains.
    if new_remaining > 0 {
        if let Some(row) = pick_new_clue(&state, user_id, &params).await? {
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }

    // 3) Nothing to do.
    Ok(Json(json!({ "done": true, "dueCount": due_count, "newRemaining": new_remaining })))
}

async fn pick_new_clue(
    state: &Arc<AppState>,
    user_id: i32,
    params: &HashMap<String, String>,
) -> Result<Option<ClueRow>, AppError> {
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let game_types_str = params.get("gameTypes").map(|s| s.as_str()).unwrap_or("");
    let game_types: Vec<&str> = game_types_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut conditions = vec![
        "question IS NOT NULL".to_string(),
        "answer IS NOT NULL".to_string(),
        "classifier_category IS NOT NULL".to_string(),
        "air_date IS NOT NULL".to_string(),
        "archived = false".to_string(),
        // Exclude clues already in this user's SRS pool.
        "id NOT IN (SELECT question_id FROM srs_cards WHERE user_id = $1)".to_string(),
    ];

    let use_category = category != "all";
    if use_category {
        conditions.push("classifier_category = $2".to_string());
    }
    for gt in &game_types {
        match *gt {
            "kids" | "Kids" => conditions
                .push("NOT (notes ILIKE '%Kids%' OR notes ILIKE '%Kid''s%')".to_string()),
            "teen" | "Teen" => conditions.push("NOT (notes ILIKE '%Teen%')".to_string()),
            "college" | "College" => conditions.push("NOT (notes ILIKE '%College%')".to_string()),
            _ => {}
        }
    }
    let where_clause = conditions.join(" AND ");

    let count_sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {}", where_clause);
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql).bind(user_id);
    if use_category {
        count_q = count_q.bind(category);
    }
    let total: i64 = count_q.fetch_one(&state.pool).await?;
    if total == 0 {
        return Ok(None);
    }

    // Same recency-biased exponential offset used by the legacy quiz picker.
    use rand::Rng;
    let r: f64 = rand::rng().random();
    let lambda = 3.5_f64;
    let normalized = (-(1.0 - r).ln() / lambda).min(1.0);
    let offset = (normalized * total as f64).floor() as i64;

    let sql = format!(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes
         FROM jeopardy_questions WHERE {} ORDER BY air_date DESC LIMIT 1 OFFSET {}",
        where_clause, offset
    );
    let mut q = sqlx::query_as::<_, ClueRow>(&sql).bind(user_id);
    if use_category {
        q = q.bind(category);
    }
    Ok(q.fetch_optional(&state.pool).await?)
}
```

- [ ] **Step 3: Mount the route**

In `backend/src/main.rs` add:

```rust
        .route("/api/practice/next", get(routes::practice::next))
```

- [ ] **Step 4: Add a unit test for the day boundary**

Append to `backend/src/routes/practice.rs` a test module (runs without a DB):

```rust
#[cfg(test)]
mod tests {
    use super::day_start_utc;
    use chrono::{TimeZone, Utc};

    #[test]
    fn chicago_day_start_is_local_midnight_in_utc() {
        // 2026-06-30 12:00 UTC; Chicago is UTC-5 (CDT) in summer → local midnight = 05:00 UTC.
        let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
        let ds = day_start_utc(now, Some("America/Chicago"));
        assert_eq!(ds, Utc.with_ymd_and_hms(2026, 6, 30, 5, 0, 0).unwrap());
    }

    #[test]
    fn unknown_or_missing_tz_falls_back_to_utc_midnight() {
        let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
        assert_eq!(
            day_start_utc(now, Some("Not/AZone")),
            Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap()
        );
        assert_eq!(
            day_start_utc(now, None),
            Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap()
        );
    }
}
```

Run: `cd backend && cargo test practice::`
Expected: PASS — both day-boundary tests green.

- [ ] **Step 5: Build and lint**

Run: `cd backend && cargo build 2>&1 | tail -5 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: clean build.

- [ ] **Step 6: Manual verification**

```bash
curl -s -b cookies.txt "localhost:3000/api/practice/next" | jq '{done, isNew, dueCount, newRemaining, id: .card.id}'
```

Expected: on a fresh user, `done:false`, `isNew:true`, `newRemaining` one less than the daily cap after grading. Grade several new clues past the cap (Task 3 curl) and confirm `newRemaining` reaches 0 and `next` returns `done:true` once no reviews are due. A card graded `wrong` becomes due immediately (relearning 10-min step is `<= now()` only after 10 min; to see a due review quickly, grade one `too_easy` then manually `UPDATE srs_cards SET due = now()` on the scratch DB and confirm `next` returns it with `isNew:false`).

- [ ] **Step 7: Commit**

```bash
git add backend/Cargo.toml backend/Cargo.lock backend/src/routes/practice.rs backend/src/main.rs
git commit -m "feat(srs): practice/next endpoint with due-review + capped new-clue picker"
```

---

### Task 5: `status` endpoint + repoint `review`/`mastered`

**Files:**
- Modify: `backend/src/routes/practice.rs` (add `status`)
- Modify: `backend/src/main.rs` (mount `status`)
- Modify: `backend/src/routes/review.rs` (list upcoming/overdue from `srs_cards`)
- Modify: `backend/src/routes/mastery.rs` (`random_mastered` selects interval-derived mastered)

**Interfaces:**
- Consumes: `day_start_utc` from Task 4.
- Produces: `GET /api/practice/status` → `{ dueCount, newRemaining, reviewedToday, forecast: [{date, count}] }`.

- [ ] **Step 1: Implement `status`**

Append to `backend/src/routes/practice.rs`:

```rust
pub async fn status(
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
        "SELECT COUNT(*) FROM srs_cards WHERE user_id = $1 AND suspended = false AND due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards WHERE user_id = $1 AND created_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    let reviewed_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_attempts WHERE user_id = $1 AND answered_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;

    // 14-day due forecast (calendar day in UTC; good enough for a bar chart).
    let forecast: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        "SELECT (due AT TIME ZONE 'UTC')::date AS d, COUNT(*)
         FROM srs_cards
         WHERE user_id = $1 AND suspended = false
           AND due < now() + interval '14 days'
         GROUP BY d ORDER BY d",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let forecast_json: Vec<Value> = forecast
        .into_iter()
        .map(|(d, c)| json!({ "date": d, "count": c }))
        .collect();

    Ok(Json(json!({
        "dueCount": due_count,
        "newRemaining": new_remaining,
        "reviewedToday": reviewed_today,
        "forecast": forecast_json,
    })))
}
```

Mount in `backend/src/main.rs`:

```rust
        .route("/api/practice/status", get(routes::practice::status))
```

- [ ] **Step 2: Repoint `review::list` to SRS cards, preserving the response envelope**

The frontend review page consumes an array of `{ question: {...}, masteryProgress: { consecutive_correct, required } }` and sorts by `masteryProgress.consecutive_correct`. To avoid touching that page, keep the exact envelope but source rows from `srs_cards` (cards not yet mastered), ordered by soonest due. Map `consecutive_correct` to the card's `reps` and keep `required: 3` so the existing MasteryBadge still renders (now read as "successful reviews so far").

Replace the entire body of `backend/src/routes/review.rs` with:

```rust
use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, FromRow)]
struct ReviewRow {
    id: i32,
    question: Option<String>,
    answer: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    clue_value: Option<i32>,
    round: Option<i32>,
    air_date: Option<chrono::NaiveDate>,
    reps: i32,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";

    // "Review" = SRS cards you're still learning (not yet at the mastered interval),
    // soonest-due first.
    let base = "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
                       jq.clue_value, jq.round, jq.air_date, sc.reps
                FROM srs_cards sc
                JOIN jeopardy_questions jq ON jq.id = sc.question_id
                WHERE sc.user_id = $1 AND sc.suspended = false AND jq.archived = false
                  AND NOT (sc.state = 'review' AND sc.interval_days >= 21)";

    let rows: Vec<ReviewRow> = if use_category {
        let sql = format!("{base} AND jq.classifier_category = $2 ORDER BY sc.due ASC LIMIT 200");
        sqlx::query_as::<_, ReviewRow>(&sql)
            .bind(user_id)
            .bind(category)
            .fetch_all(&state.pool)
            .await?
    } else {
        let sql = format!("{base} ORDER BY sc.due ASC LIMIT 200");
        sqlx::query_as::<_, ReviewRow>(&sql)
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?
    };

    let result: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "question": {
                    "id": row.id,
                    "question": row.question,
                    "answer": row.answer,
                    "category": row.category,
                    "classifier_category": row.classifier_category,
                    "clue_value": row.clue_value,
                    "round": row.round,
                    "air_date": row.air_date,
                },
                "masteryProgress": {
                    "consecutive_correct": row.reps,
                    "required": 3,
                }
            })
        })
        .collect();

    Ok(Json(json!(result)))
}
```

Note: the review page's in-page re-drill still POSTs to `/api/quiz/submit` (which now only records a `question_attempts` row — no mastery write). That is acceptable for this phase; the review page keeps working unchanged.

- [ ] **Step 3: Repoint `mastery::random_mastered` to interval-derived mastered**

In `backend/src/routes/mastery.rs`, change both branches of the `random_mastered` query so "mastered" means an SRS card in review state with a long interval, instead of `question_mastery.mastered = true`. Replace `FROM question_mastery qm JOIN ... WHERE qm.user_id = $1 AND qm.mastered = true` with:

```sql
FROM srs_cards sc
JOIN jeopardy_questions jq ON jq.id = sc.question_id
WHERE sc.user_id = $1 AND sc.state = 'review' AND sc.interval_days >= 21 AND jq.archived = false
```

(keep the optional `AND jq.classifier_category = $2` branch). Replace the selected `qm.mastered_at` column with `sc.last_review AS mastered_at` so the response shape is unchanged. The `reset` handler and `/api/mastery/reset` route may remain; update its UPDATE to target `srs_cards` (set `state='learning', interval_days=0, ease=2.5, reps=0, step_index=0, due=now()` where `user_id=$1 AND question_id=$2`).

- [ ] **Step 4: Build, lint**

Run: `cd backend && cargo build 2>&1 | tail -5 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 5: Manual verification**

```bash
curl -s -b cookies.txt localhost:3000/api/practice/status | jq
curl -s -b cookies.txt localhost:3000/api/review | jq 'length'
```

Expected: `status` returns numeric `dueCount`/`newRemaining`/`reviewedToday` and a `forecast` array. After grading a clue `too_easy` and `UPDATE srs_cards SET interval_days = 30, state='review'`, `/api/mastered` returns it.

- [ ] **Step 6: Commit**

```bash
git add backend/src/routes/practice.rs backend/src/routes/review.rs backend/src/routes/mastery.rs backend/src/main.rs
git commit -m "feat(srs): status endpoint; repoint review + mastered onto srs_cards"
```

---

### Task 6: Extend preferences (`new_cards_per_day`, `timezone`)

**Files:**
- Modify: `backend/src/routes/preferences.rs`

**Interfaces:**
- Produces: `GET /api/preferences` now returns `{ gameTypeFilters, newCardsPerDay, timezone }`; `PUT /api/preferences` accepts those fields (all optional except the existing `gameTypeFilters`).

- [ ] **Step 1: Update the GET handler**

In `backend/src/routes/preferences.rs`, change the `get` query and response to also select and return the new columns:

```rust
    let row: (Option<String>, i32, Option<String>) = sqlx::query_as(
        "SELECT game_type_filters, new_cards_per_day, timezone FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let filters: Vec<Value> = match row.0 {
        Some(s) if !s.is_empty() => serde_json::from_str(&s).unwrap_or_default(),
        _ => vec![],
    };

    Ok(Json(json!({
        "gameTypeFilters": filters,
        "newCardsPerDay": row.1,
        "timezone": row.2,
    })))
```

- [ ] **Step 2: Update the PUT body and handler**

Change `UpdatePreferencesBody` and `update`:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreferencesBody {
    pub game_type_filters: Vec<String>,
    pub new_cards_per_day: Option<i32>,
    pub timezone: Option<String>,
}
```

In `update`, after writing `game_type_filters`, conditionally persist the new fields:

```rust
    if let Some(n) = body.new_cards_per_day {
        let n = n.clamp(0, 500);
        sqlx::query("UPDATE users SET new_cards_per_day = $1 WHERE id = $2")
            .bind(n)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(tz) = body.timezone.as_ref() {
        sqlx::query("UPDATE users SET timezone = $1 WHERE id = $2")
            .bind(tz)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }
```

- [ ] **Step 3: Build, lint, verify**

Run: `cd backend && cargo build 2>&1 | tail -3`. Then:

```bash
curl -s -b cookies.txt -X PUT localhost:3000/api/preferences \
  -H 'Content-Type: application/json' \
  -d '{"gameTypeFilters":[],"newCardsPerDay":30,"timezone":"America/Chicago"}' | jq
curl -s -b cookies.txt localhost:3000/api/preferences | jq
```

Expected: GET returns `newCardsPerDay: 30`, `timezone: "America/Chicago"`.

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/preferences.rs
git commit -m "feat(srs): expose newCardsPerDay and timezone preferences"
```

---

### Task 7: `QuestionCard` — add 3-button grading (backward compatible)

**Files:**
- Modify: `frontend/src/lib/components/QuestionCard.svelte`

**Interfaces:**
- Produces: `QuestionCard` gains OPTIONAL `onWrong`, `onGotIt`, `onTooEasy` callbacks. When `onGotIt` is provided it renders the 3-button grade row; otherwise it falls back to the existing 2-button `onCorrect`/`onIncorrect` row. **The legacy props stay so the review and mastered pages keep working unchanged** — they are the other two consumers of this component.

- [ ] **Step 1: Make legacy callbacks optional and add the new ones**

In the `$props()` type block, change the two legacy callback types to optional and add three optional new ones:

```ts
    onCorrect?: () => void;
    onIncorrect?: () => void;
    onWrong?: () => void;
    onGotIt?: () => void;
    onTooEasy?: () => void;
```

Add `onWrong,`, `onGotIt,`, `onTooEasy,` to the destructured props list (leave `onCorrect,` and `onIncorrect,` in place).

- [ ] **Step 2: Render 3 buttons when new callbacks are present, else the legacy 2**

Replace the `<!-- Correct / Incorrect buttons -->` block (the `<div class="flex gap-3">...</div>` containing the Incorrect/Correct buttons) with:

```svelte
      {#if onGotIt}
        <!-- 3-button SRS grading -->
        <div class="grid grid-cols-3 gap-2">
          <button
            onclick={onWrong}
            disabled={submitting}
            class="py-3 rounded-xl bg-red-500 hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors"
          >
            Wrong
          </button>
          <button
            onclick={onGotIt}
            disabled={submitting}
            class="py-3 rounded-xl bg-green-500 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors"
          >
            Got it
          </button>
          <button
            onclick={onTooEasy}
            disabled={submitting}
            class="py-3 rounded-xl bg-blue-500 hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors"
          >
            Too easy
          </button>
        </div>
      {:else}
        <!-- Legacy correct / incorrect (review + mastered pages) -->
        <div class="flex gap-3">
          <button
            onclick={onIncorrect}
            disabled={submitting}
            class="flex-1 py-3 rounded-xl bg-red-500 hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-lg transition-colors"
          >
            ← Incorrect
          </button>
          <button
            onclick={onCorrect}
            disabled={submitting}
            class="flex-1 py-3 rounded-xl bg-green-500 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-lg transition-colors"
          >
            Correct →
          </button>
        </div>
      {/if}
```

- [ ] **Step 3: Type-check**

Run: `cd frontend && npm run check 2>&1 | tail -15`
Expected: no errors. The review and mastered pages still type-check because the legacy props remain.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/components/QuestionCard.svelte
git commit -m "feat(srs): add optional 3-button grading to QuestionCard (backward compatible)"
```

---

### Task 8: `/practice` page (rename `/quiz`, swap endpoints, 3-button flow)

**Files:**
- Rename: `frontend/src/routes/quiz/` → `frontend/src/routes/practice/`
- Modify: `frontend/src/routes/practice/+page.svelte`

**Interfaces:**
- Consumes: `/api/practice/next`, `/api/practice/grade` (Tasks 3–4); `QuestionCard` callbacks (Task 7).

- [ ] **Step 1: Move the route directory**

```bash
git mv frontend/src/routes/quiz frontend/src/routes/practice
```

- [ ] **Step 2: Replace fetch + grade logic**

In `frontend/src/routes/practice/+page.svelte`, make these edits:

Replace `fetchQuestion` and `prefetchNextQuestion` with a single fetch of the next card, tracking `isNew`, `dueCount`, `newRemaining`:

```ts
  let isNew = $state(false);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);

  function buildQuizParams(): URLSearchParams {
    const params = new URLSearchParams();
    if (selectedCategory !== 'all') params.set('category', selectedCategory);
    if (gameTypeFilters.length > 0) params.set('gameTypes', gameTypeFilters.join(','));
    return params;
  }

  async function fetchQuestion() {
    const gen = filterGen;
    loading = true;
    error = '';
    try {
      const res = await api.get(`/api/practice/next?${buildQuizParams()}`);
      if (gen !== filterGen) return;
      dueCount = res.dueCount ?? 0;
      newRemaining = res.newRemaining ?? 0;
      if (res.done) {
        done = true;
        question = null;
      } else {
        done = false;
        isNew = res.isNew;
        question = res.card;
      }
    } catch (err: any) {
      if (gen !== filterGen) return;
      error = err?.message ?? 'Failed to load question';
    } finally {
      if (gen === filterGen) loading = false;
    }
  }
```

Delete `prefetchedQuestion` state and all references to it. Replace `handleAnswer(correct)` with a rating-based grader:

```ts
  async function handleGrade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (submitting || !question) return;
    submitting = true;
    try {
      const result = await api.post('/api/practice/grade', {
        questionId: question.id,
        rating,
        sessionId,
      });
      sessionId = result.sessionId;
      runningStats.total++;
      if (rating !== 'wrong') runningStats.correct++;
      showAnswer = false;
      await fetchQuestion();
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }
```

In `handleArchive`, replace the prefetched-question fallthrough with `await fetchQuestion();` and `showAnswer = false;`.

- [ ] **Step 3: Update the QuestionCard usage and keyboard shortcuts**

Change the `<QuestionCard ... />` callbacks from `onCorrect`/`onIncorrect` to:

```svelte
          onRevealAnswer={() => { showAnswer = true; }}
          onWrong={() => handleGrade('wrong')}
          onGotIt={() => handleGrade('got_it')}
          onTooEasy={() => handleGrade('too_easy')}
```

Update `handleKeydown`: keep Space to reveal (drop the `prefetchNextQuestion()` call), and map keys `1`/`2`/`3` to grades:

```ts
    if (e.code === 'Space' && !showAnswer) {
      e.preventDefault();
      showAnswer = true;
    } else if (showAnswer && !submitting) {
      if (e.code === 'Digit1') handleGrade('wrong');
      else if (e.code === 'Digit2') handleGrade('got_it');
      else if (e.code === 'Digit3') handleGrade('too_easy');
    }
```

- [ ] **Step 4: Update header + empty state copy**

Change the `<h1>` text from `Quiz` to `Practice`. Add a due/new indicator next to it:

```svelte
      <div class="text-sm font-medium text-gray-600">
        Due <span class="font-bold text-jeopardy-blue">{dueCount}</span>
        · New left <span class="font-bold text-jeopardy-blue">{newRemaining}</span>
      </div>
```

Replace the "No questions available" empty branch so it also covers `done`:

```svelte
    {:else if done}
      <div class="text-center py-16 text-gray-600">
        🎉 All caught up — no reviews due and today's new-clue limit is reached.
      </div>
    {:else}
      <div class="text-center py-16 text-gray-500">No questions available for the selected filters.</div>
    {/if}
```

Update the desktop keyboard hint text under the card to read `1 Wrong · 2 Got it · 3 Too easy` when `showAnswer`.

- [ ] **Step 5: Type-check and manual smoke test**

Run: `cd frontend && npm run check 2>&1 | tail -15`
Expected: no errors.

Then build the frontend and run the server against the scratch DB; load `/practice`, reveal, and grade with the three buttons and with keys 1/2/3. Confirm the Due/New counters update and that after exhausting new + due you see the "All caught up" state.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/routes/practice
git commit -m "feat(srs): /practice page with 3-button flow on the SRS API"
```

---

### Task 9: Nav, Settings, and Dashboard wiring

**Files:**
- Modify: `frontend/src/lib/components/Nav.svelte`
- Modify: `frontend/src/routes/settings/+page.svelte`
- Modify: `frontend/src/routes/dashboard/+page.svelte`

**Interfaces:**
- Consumes: `/api/preferences` (Task 6), `/api/practice/status` (Task 5).

- [ ] **Step 1: Rename the Nav link**

In `frontend/src/lib/components/Nav.svelte`, change the first `links` entry from `{ href: '/quiz', label: 'Quiz' }` to `{ href: '/practice', label: 'Practice' }`.

- [ ] **Step 2: Add SRS preferences to Settings**

In `frontend/src/routes/settings/+page.svelte`, load `newCardsPerDay` and `timezone` from `/api/preferences` on mount and render controls that PUT back. Add (adapting to the file's existing state/markup patterns — read it first):

```ts
  let newCardsPerDay = $state(20);
  let timezone = $state('');

  // in onMount, after fetching preferences:
  //   newCardsPerDay = prefs?.newCardsPerDay ?? 20;
  //   timezone = prefs?.timezone ?? '';

  async function saveSrsPrefs() {
    await api.put('/api/preferences', {
      gameTypeFilters, // reuse whatever the page already tracks
      newCardsPerDay,
      timezone,
    });
  }
```

```svelte
  <label class="block">
    <span class="text-sm font-semibold text-gray-700">New clues per day</span>
    <input type="number" min="0" max="500" bind:value={newCardsPerDay}
      onchange={saveSrsPrefs}
      class="mt-1 w-32 rounded-lg border border-gray-300 px-3 py-2" />
  </label>
  <label class="block mt-4">
    <span class="text-sm font-semibold text-gray-700">Timezone (IANA)</span>
    <input type="text" placeholder="America/Chicago" bind:value={timezone}
      onchange={saveSrsPrefs}
      class="mt-1 w-64 rounded-lg border border-gray-300 px-3 py-2" />
  </label>
```

(If the settings page does not already track `gameTypeFilters`, fetch it in the same `/api/preferences` load and pass the existing value through unchanged so it is not clobbered.)

- [ ] **Step 3: Add a "Due today" widget to the Dashboard**

In `frontend/src/routes/dashboard/+page.svelte`, fetch `/api/practice/status` on mount and render a summary card near the top (adapt to the file's existing layout — read it first):

```ts
  let srs = $state<{ dueCount: number; newRemaining: number; reviewedToday: number; forecast: Array<{date: string; count: number}> } | null>(null);
  // in onMount: srs = await api.get('/api/practice/status').catch(() => null);
```

```svelte
  {#if srs}
    <div class="bg-white rounded-xl shadow-sm p-5 flex gap-8">
      <div><p class="text-3xl font-bold text-jeopardy-blue">{srs.dueCount}</p><p class="text-xs uppercase text-gray-500">Due today</p></div>
      <div><p class="text-3xl font-bold text-jeopardy-blue">{srs.newRemaining}</p><p class="text-xs uppercase text-gray-500">New left</p></div>
      <div><p class="text-3xl font-bold text-jeopardy-blue">{srs.reviewedToday}</p><p class="text-xs uppercase text-gray-500">Reviewed today</p></div>
      <a href="/practice" class="ml-auto self-center px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold">Practice →</a>
    </div>
  {/if}
```

- [ ] **Step 4: Type-check, build, smoke test**

Run: `cd frontend && npm run check 2>&1 | tail -15 && npm run build 2>&1 | tail -5`
Expected: no errors; build succeeds. Load `/dashboard` (widget shows counts) and `/settings` (changing new-per-day persists via `/api/preferences`).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/components/Nav.svelte frontend/src/routes/settings frontend/src/routes/dashboard
git commit -m "feat(srs): nav rename, SRS settings, and dashboard due widget"
```

---

## Notes for the implementer

- **Applying the migration to production** is a manual, deliberate step done outside this plan (the app has no migration runner and the DB is shared). After merging, apply `backend/migrations/0001_srs_cards.sql` to the production database during a maintenance window.
- **Legacy `/quiz` links / PWA start_url:** if any hard-coded `/quiz` remains (e.g., a manifest `start_url` or bookmark), point it at `/practice`. Grep: `grep -rn "/quiz" frontend/src frontend/static` and update non-API occurrences.
- **`question_mastery` table** is intentionally left in place (historical data); nothing writes to it after Task 3.
- **Review and Mastered frontend pages are intentionally NOT modified.** They keep working because (a) `/api/review` preserves its `{ question, masteryProgress }` envelope (Task 5), (b) `/api/mastered` preserves its single-card shape (Task 5), and (c) `QuestionCard` stays backward-compatible via optional legacy props (Task 7). An SRS-native redesign of these two pages (e.g. showing due dates and interval history) is a reasonable follow-on but out of scope here.
- **Deferred from the spec:** `retention_target` preference (no effect in the SM-2-class scheduler) and swapping internals to the `fsrs` crate — both are follow-ons for when true FSRS is wired in behind the same `schedule()` interface.
