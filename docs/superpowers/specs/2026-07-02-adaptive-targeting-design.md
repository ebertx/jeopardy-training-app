# Adaptive Weakness Targeting — Design Spec

**Date:** 2026-07-02
**Status:** Approved (design), pending implementation plan
**Builds on:** the SRS Practice engine and topic drilling (both deployed). This is
Phase 2b — the final piece of the question-selection engine from the original
brainstorm.

## 1. Problem & goal

Practice's new-clue picker is date-biased random: it has no idea that the user runs
55% in Music & Performing Arts and 82% in Mathematics & Logic. Goal: tilt new-clue
selection toward measured weak categories so gaps close automatically, and show the
user what the app is targeting and why.

Grounding data (production, 2026-07-02): 3,925 attempts across 13 classifier
categories (38–649 per category) — statistically solid at category granularity,
far too thin at show-category granularity. Weakness is therefore measured at the
**13 classifier categories**; fine-grained topic analysis remains the AI Study
feature's job.

## 2. Decisions (locked)

| Decision | Choice |
| --- | --- |
| Granularity | 13 `classifier_category` buckets |
| Tilt | **60/40**: 60% of new-clue pulls sample a category by weakness weight, 40% keep the existing fully-random behavior |
| Weakness score | Smoothed miss-rate (see §3), normalized across categories |
| Window | Attempts from the **last 180 days**; fall back to all-time if the window holds < 200 attempts |
| Scope | Practice's new-clue picker ONLY. Reviews stay schedule-driven; Drill stays user-aimed; a manual category filter bypasses adaptive entirely |
| Control | One Settings toggle "Adaptive clue selection", default **on** (`users.adaptive_targeting BOOL NOT NULL DEFAULT true`, migration 0003) |
| Visibility | `GET /api/practice/status` gains `adaptiveWeights`; dashboard renders a "Focus areas" panel |

## 3. Weakness score

Per user, per category, over the attempt window:

```
global_acc        = correct_all / attempts_all            (within the window)
smoothed_acc(cat) = (correct_cat + 5 × global_acc) / (attempts_cat + 5)
raw_weight(cat)   = 1 − smoothed_acc(cat)
weight(cat)       = raw_weight(cat) / Σ raw_weight        (normalized)
```

- The +5 pseudo-count prior stops tiny samples from dominating and gives
  unmeasured categories a moderate default weight (natural exploration).
- Categories with zero eligible clues are skipped at sampling time, not in the
  score.
- The math lives in a **pure, unit-tested function** (like the SRS scheduler):
  inputs are `(attempts, correct)` pairs plus the global rate; output is the
  normalized weight vector. No DB, no randomness inside.

## 4. Selection flow

In `practice.rs::pick_new_clue`, when NO manual category filter is set and the
user's `adaptive_targeting` is true:

1. Draw `r ∈ [0,1)`. If `r < 0.4` → existing behavior (unconstrained date-biased
   pick). Else:
2. Load per-category `(attempts, correct)` for the window (one GROUP BY query on
   `question_attempts` joined to `jeopardy_questions`), compute weights, sample one
   category.
3. Run the existing date-biased picker constrained to that category (same SQL,
   category bound as today). If it returns no clue (all seen/archived), fall back
   to the unconstrained pick.

With a manual category filter, or the toggle off, behavior is exactly today's.

## 5. API & frontend

- `GET /api/practice/status` adds
  `adaptiveWeights: [{category, attempts, accuracy, weight}]` sorted by weight
  descending (empty array when the toggle is off). Computed with the same pure
  function — one source of truth.
- `GET/PUT /api/preferences` gains `adaptiveTargeting: bool`.
- **Settings**: an "Adaptive clue selection" checkbox in the Practice card.
- **Dashboard**: a "Focus areas" panel in the SRS summary card — categories by
  weight with accuracy + attempt count, top 3 visually marked as targeted. Copy
  explains the tilt in one line ("New clues favor your weaker categories").

## 6. Migration & testing

- `backend/migrations/0003_adaptive_targeting.sql`:
  `ALTER TABLE users ADD COLUMN IF NOT EXISTS adaptive_targeting BOOLEAN NOT NULL DEFAULT true;`
  Applied manually on Tower before the container ships (instant — single column).
- Tests: pure weight-function units (smoothing, normalization, zero-attempt
  categories, empty input); the 60/40 branch and category-constrained fallback are
  exercised via build/clippy + deferred live verification (no scratch DB), same as
  prior phases.
- Frontend: `npm run check` + `build` clean.

## 7. Out of scope

- Finer-than-category granularity (revisit when per-topic samples are meaningful).
- Tilt-strength knob in the UI (60/40 is the design).
- Applying adaptive weighting to Drill or review scheduling.
