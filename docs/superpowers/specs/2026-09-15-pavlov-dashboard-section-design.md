# Pavlov Dashboard Section — Design

**Date:** 2026-09-15
**Status:** Approved (user chose stacked Pavlov-first layout, deck-progress
card, and the review-log approach)

## Goal

The dashboard today is built around standard practice (`question_attempts`);
Pavlov gets a three-tile strip inside the SRS card. Christian's focus through
the end of 2026 is finishing the Pavlov deck (goal: every one of ~4,765
answers touched by 2026-12-31, possibly booking the Anytime Test in December).
Split the dashboard into two sections, Pavlov first, and give Pavlov the same
stats the standard section has: cold/review accuracy, 30-day accuracy trend,
category breakdown, deck composition with deltas, due forecast — plus a
deck-progress card in place of mock readiness.

## Constraint that shapes the design

`pavlov_cards` stores only current SRS state (reps, lapses, ease, interval).
There is no per-grade log, so accuracy-over-time and cold-vs-review accuracy
cannot be computed from existing data. This spec adds a log; history starts
at deploy. The 3,478 grades made before then remain only as summed counters.

Alternatives rejected: deriving everything from card state (no accuracy
history — not parity); reusing `question_attempts` with a new kind (attempts
are keyed by question id, Pavlov by answer id — every query would need a
nullable join).

## 1. Data (migration 0013)

```sql
CREATE TABLE pavlov_reviews (
  id          BIGSERIAL PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  answer_id   INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
  rating      TEXT NOT NULL CHECK (rating IN ('wrong','got_it','too_easy')),
  first_grade BOOLEAN NOT NULL,
  reviewed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX pavlov_reviews_user_time ON pavlov_reviews (user_id, reviewed_at);
CREATE INDEX pavlov_reviews_user_answer ON pavlov_reviews (user_id, answer_id);

CREATE TABLE pavlov_deck_snapshots (
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

- `drill_grade` inserts one `pavlov_reviews` row per grade, in the same
  transaction as the card upsert. `first_grade` is true when no
  `pavlov_cards` row existed for (user, answer) before this grade — the
  Pavlov analog of `attempt_kind = 'new'`. Suspend/banish inserts nothing.
- "Correct" = rating `got_it` or `too_easy`.
- `pavlov_target_date` is read/written through the existing preferences
  endpoints (`pavlovTargetDate`, ISO date string) and the Settings page.

## 2. API — `GET /api/pavlov/stats`

Lives in `backend/src/routes/pavlov.rs`. Same JSON shape as `/api/stats`
where the concept exists, so the dashboard reuses its TypeScript types.

```
overall / cold / review / cold30d : { total, correct, accuracy }
  cold  = first_grade rows; review = the rest; cold30d = cold in last 30d
dailyAccuracy[] : { date, total, correct, accuracy,
                    coldTotal, coldCorrect, coldAccuracy,
                    reviewTotal, reviewCorrect, reviewAccuracy }   (last 30 days)
categoryBreakdown[] : same columns as /api/stats, grouped by
                      pavlov_answers.meta_category
reviewedToday : count of pavlov_reviews since user-local day start
dueCount, newRemaining : as /api/pavlov/status (kept there too for the drill page)
forecast[] : { date, count } — 14 days, user-timezone bucketed, overdue folds
             into today (same query shape as /api/practice/status)
deck : { learning, maturing, mastered, struggling, banished, total, delta }
progress : { deckTotal, touched, touchedPct, trailingPerDay, targetDate,
             daysLeft, requiredPerDay, pastTarget, projectedFinish, daysAhead,
             historySince }
```

Deck buckets (mutually exclusive):

| bucket | rule |
|---|---|
| banished | `suspended = true` (curation by choice — deliberately NOT struggling) |
| struggling | not suspended, `lapses >= 4` |
| learning | not suspended, lapses < 4, `state <> 'review'` |
| maturing | not suspended, lapses < 4, review, `interval_days < 21` |
| mastered | not suspended, lapses < 4, review, `interval_days >= 21` |

`total` = sum of all five. Snapshot upsert + delta baseline (newest snapshot
≥7 days old, else oldest before today) copied from the SRS pattern; delta
covers the four non-banished buckets.

Progress math (pure function, unit tested):

- `touched` = count of `pavlov_cards` rows for the user (banished included —
  a banished card is done).
- `trailingPerDay` = cards created in the last 14 local days / 14.
- `daysLeft` = targetDate − today (local), min 0.
- `requiredPerDay` = ceil((deckTotal − touched) / daysLeft); if daysLeft = 0
  and remaining > 0, report remaining and flag `pastTarget: true`.
- `projectedFinish` = today + ceil(remaining / trailingPerDay) days;
  null when trailingPerDay = 0.
- `daysAhead` = daysLeft − ceil(remaining / trailingPerDay); null when
  trailingPerDay = 0. Positive means ahead of pace.
- `historySince` = earliest `pavlov_reviews.reviewed_at` for the user, null
  when no rows yet (UI uses it to caption the all-time tiles).

## 3. UI

Dashboard (`frontend/src/routes/dashboard/+page.svelte`) becomes two
sections, each rendered by a shared `StatsSection` component:

**Pavlov** (header, gold accent matching the drill button)

1. Summary strip: Due · New left · Reviewed today · "Pavlov Drill →".
2. Deck progress card (occupies the mock-readiness slot): `1,097 / 4,765 ·
   23%`, thin progress bar, then one line: "Need 34/day to finish by Dec 31 ·
   trailing 11/day · projected Jan 14". Pace color: green when trailing ≥
   required, amber when within 20% below, red otherwise. Target date links
   to `/settings`. Past-target state: "Target passed — N cards left".
3. Accuracy tiles: Cold accuracy last 30 days · Retention (review accuracy).
   With no log rows yet: "—" and caption "tracking since {historySince}" or
   "tracking starts with your next drill".
4. Accuracy chart, last 30 days, cold vs review lines (same colors as
   standard).
5. Deck composition bar: four buckets + gray "banished" swatch; delta line
   under it. Labels are plain text (`/pavlov/list` has no state filter).
6. Due forecast: 7 days on phones, 14 on desktop.
7. Category performance chart + breakdown table, identical to standard.

**Standard Practice** (header): everything there today, unchanged, with the
Pavlov strip removed from the SRS card. Internal order unchanged.

`StatsSection` takes `{ title, accent, stats, srsStatus, links }` and renders
the shared pieces (summary strip, accuracy tiles, accuracy chart, deck bar,
forecast, category chart + table). Section-specific cards (Pavlov progress;
standard mock readiness, projected score, focus areas, blind spots) are
passed as snippets/slots so the component stays generic. Chart configs move
into the component; the dashboard file shrinks.

Settings: date input "Pavlov target date" directly under "Pavlov new cards
per day".

## 4. Testing

Backend (existing Rust test style):
- Progress math: zero trailing pace, target in the past, deck complete,
  remaining not divisible by daysLeft, 14-day window shorter than history.
- Grade handler: first grade → row with `first_grade = true`; second grade
  on same card → `first_grade = false`; suspend → no row.
- Stats endpoint on a seeded pool: cold/review splits, category rollup,
  banished excluded from struggling, daily buckets respect user timezone.

Frontend: render `StatsSection` against the mock API (dev verification
setup), screenshot at 390×844 and 1280 wide, including the empty-log state.

## 5. Rollout

Apply migration 0013 with `scripts/apply-migration.sh` before deploying the
image (container does not run migrations). Additive only, so the running
container is unaffected between steps. No backfill. Update the dev mock API
to serve `/api/pavlov/stats`.

## Known gap

All-time Pavlov accuracy tiles lag reality until the log accumulates; deck
progress, composition, and forecast are exact from day one because they read
card state.
