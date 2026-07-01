# Spaced-Repetition Practice Engine — Design Spec

**Date:** 2026-06-30
**Status:** Approved (design), pending implementation plan
**Scope:** Phase 1 of a larger question-selection engine. This spec covers the SRS
backbone only. Topic/search drilling and adaptive weakness-targeting are documented
as follow-on phases (see §10) but are **out of scope** for this spec.

## 1. Problem & goal

The app is a mature Jeopardy training tool (quiz, Coryat, AI study sheets, stats),
but its retention model is the weakest link: "mastery" is `N consecutive correct →
mastered forever`, which ignores memory decay. A clue answered correctly three months
ago is treated as permanently known.

**Goal:** Replace the naive mastery model with a real spaced-repetition system (SRS)
so the app resurfaces clues at expanding intervals, un-masters what the user starts
forgetting, and provides a daily habit loop. This turns a quiz app into a training
system and lays the substrate for later adaptive/drill features.

## 2. Decisions (locked)

| Decision | Choice |
| --- | --- |
| Grading granularity | **3-button**: Wrong / Got it / Too easy |
| Product surface | **Full replace incl. Quiz** — one unified daily "Practice" session |
| Migration of existing data | **Clean slate** — cards enter the SRS pool as seen going forward |
| Daily sizing | **Capped new clues (default 20), uncapped reviews** |
| Algorithm | **FSRS** via the official Rust `fsrs` crate |
| "Mastered" definition | Derived: current interval ≥ 21 days (adjustable) |
| Leech definition | lapses ≥ 8 → auto-suspend |
| Ordering | Reviews before new clues |

## 3. Concept & daily flow

"Quiz" becomes **Practice** — one unified daily loop:

1. On entry, assemble a queue: **all due review cards** + up to **N new clues**
   (default 20, minus new clues already introduced today in the user's timezone).
2. Reviews are served first, then new clues (protects retention on busy days).
3. Each item: show clue → reveal response → self-rate **Wrong / Got it / Too easy**.
4. **Wrong** re-queues the card within the same session (short learning step) until
   rated Got it / Too easy; the lapse is recorded.
5. Session ends when the queue drains → summary (reviewed count, new learned,
   accuracy, next-due forecast).

A clue enters the SRS pool the first time the user sees it. New clues are pulled by
the **existing date-biased exponential picker** (`quiz::random` logic), still honoring
category + game-type (kids/teen/college) filters, excluding clues already in the
user's `srs_cards`.

## 4. Rating → algorithm

- Engine: **FSRS** (modern Anki scheduler) via the official Rust `fsrs` crate — the
  math and state transitions are not hand-rolled. Fallback only if the crate proves
  unworkable: a compact SM-2 implementation.
- Button → FSRS rating mapping:
  - **Wrong → Again (1)**
  - **Got it → Good (3)**
  - **Too easy → Easy (4)**
  - Hard (2) is never emitted.
- New/lapsed cards use short in-session learning steps. On graduation to the `review`
  state, FSRS owns scheduling: stability/difficulty → next `due`.
- `retention_target` (FSRS desired retention, default 0.90) is a user preference fed
  to FSRS interval computation.

## 5. Data model

New table `srs_cards` (one row per user × clue, created on first sight):

```
srs_cards
  id            serial pk
  user_id       int         references users(id) on delete cascade
  question_id   int         references jeopardy_questions(id)
  state         text        -- learning | review | relearning
  stability     double precision
  difficulty    double precision
  due           timestamptz
  last_review   timestamptz
  reps          int      default 0
  lapses        int      default 0
  step_index    smallint default 0   -- position in learning/relearning steps
  suspended     bool     default false  -- leech guard
  created_at    timestamptz default now()
  unique (user_id, question_id)
  index (user_id, due)
  index (user_id, suspended, due)
```

The existing `question_mastery` table is **retired**: left in place for historical
stats, no longer written to. `question_attempts` continues to be written (it powers
existing stats and the future adaptive phase).

Preference additions:
- `new_cards_per_day` int, default 20
- `timezone` text (IANA, e.g. `America/Chicago`) — day boundary + new-card counter reset
- `retention_target` double, default 0.90

Derived concepts (no stored flag):
- **Mastered** = card in `review` state whose current interval ≥ 21 days (adjustable).
- **Leech** = lapses ≥ 8 → `suspended = true`, surfaced to the user to reset or bury.

## 6. Day boundary

"Today" and the new-card counter reset at local midnight in the user's `timezone`
preference (default falls back to UTC if unset). New-card allowance for the day =
`new_cards_per_day − (new cards first-seen since local midnight)`. Due reviews are
`due <= now()`, never capped.

## 7. API

Single-card fetch, matching the current quiz pattern:

- `GET /api/practice/next` → `{ card, isNew, dueCount, newRemaining }`
  (returns a due review first; else a new clue if allowance remains; else `{ done: true }`).
- `POST /api/practice/grade` `{ question_id, rating }` → updates FSRS state (creating
  the `srs_cards` row on first sight), records a `question_attempts` row, returns next
  `due`/interval.
- `GET /api/practice/status` → `{ dueCount, newRemaining, reviewedToday, forecast[] }`
  for the dashboard badge (`forecast` = due counts for the next ~14 days).
- Repoint `GET /api/review` → overdue/upcoming browsable list.
- Repoint `GET /api/mastered` → interval-derived list (interval ≥ 21 days).
- `/api/quiz/random` stays as the internal new-card source (may be called by
  `practice/next` or kept as a thin internal helper).

`rating` is validated to the set {wrong, got_it, too_easy} and mapped server-side to
FSRS ratings; the client never sends raw FSRS integers.

## 8. Frontend

- `/quiz` route → **`/practice`** (Nav links + PWA `start_url` / manifest updated).
- `QuestionCard`: swap the correct/incorrect toggle for the 3 rating buttons
  (Wrong / Got it / Too easy), shown after the response is revealed.
- **Dashboard**: add widgets — Due today, New remaining, 30-day retention,
  due-forecast bar — alongside existing charts.
- **Review** and **Mastered** pages become browsable derived lists; the active loop
  lives in Practice.
- **Settings**: new-cards-per-day, timezone, retention target.
- `SessionSummary`: report reviewed count, new learned, accuracy, next-due forecast.

## 9. Migration, rollout & testing

- One SQL migration: create `srs_cards` (+ indexes); add the three preference columns.
  No data backfill (clean slate). Migration must apply cleanly and be idempotent-safe
  in the project's migration flow.
- `question_mastery` writes are removed from `quiz::submit`/grading paths; the table
  and its data remain readable for legacy stats.
- Tests:
  - FSRS transition unit tests: each rating → expected interval/state movement
    (Again resets to relearning; Good/Easy grow interval; Easy > Good).
  - Day-boundary + new-allowance logic across timezone rollover.
  - Leech auto-suspend at lapses ≥ 8.
  - `/api/practice/*` integration tests (next → grade → next drains a queue; new-card
    cap respected; due reviews uncapped).
  - Migration applies cleanly on a fresh and an existing DB.

## 10. Out of scope (documented follow-ons)

These share the same selection-engine substrate and are planned as later phases; they
are **not** part of this spec:

- **Topic / search drilling** — full-text (`pg_trgm`/tsvector) index over clue +
  response + original `category`; "drill this show-category / search term" sessions.
  The original `category` column (tens of thousands of micro-topics across ~500k clues)
  is the ready-made subtopic layer.
- **Adaptive weakness targeting** — per-subtopic accuracy from `question_attempts`
  weighting the new-card picker toward gaps; a visual knowledge map.

## 11. Open implementation notes

- Confirm the `fsrs` crate version and its state/param API during planning; pin it.
- Learning-step durations (e.g. Again → re-show later in session; Good → graduate) to
  be finalized in the plan using FSRS defaults where possible.
- Whether Practice fetches one card at a time (`practice/next`) or prefetches a small
  batch for snappier UX — an implementation detail to decide in the plan.
