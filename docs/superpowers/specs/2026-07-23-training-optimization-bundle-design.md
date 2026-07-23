# Training Optimization Bundle — Design

**Date:** 2026-07-23
**Status:** Approved (user: "Let's do all of them")

Six features from the coaching review, all serving one thesis: optimize for
*expected Anytime Test points*, not raw weakness or raw accuracy.

## 1. Test-value-weighted adaptive targeting

`adaptive::compute_weights` currently weights categories by weakness alone.
Multiply each category's weakness weight by its Anytime Test share
(`blend::TARGET_WEIGHTS`, normalized to fractions) so practice attention
follows expected test points. Categories keep a small floor so nothing goes
to zero. Dashboard's adaptive panel needs no UI change (weights just shift);
its explanatory copy gains a clause about test weighting.

## 2. Pavlov throughput + category-weighted introduction

- Migration 0012: `users.pavlov_new_per_day INTEGER NOT NULL DEFAULT 20`.
  `drill_next` and `/api/pavlov/status` use it instead of
  `new_cards_per_day`; Settings page gains the field (same pattern as
  new-cards-per-day).
- New-card introduction becomes two-step, mirroring the mock blend: pick a
  meta-category by `blend::TARGET_WEIGHTS` (restricted to categories that
  still have unseen cards, renormalized), then the existing evidence race
  (`-ln(random())/ln(1+score)`) within that category. Geography stops
  crowding the queue; Literature gets its test share.

## 3. Unobtrusive 8-second countdown (display-only)

Practice and Pavlov cards show a small countdown starting at 8s when a card
appears: subtle text (e.g. top-right, `text-xs text-gray-400`), ticking to 0,
then just sitting at 0 (slightly dimmed/red tint acceptable). **It does
nothing but count down** — no auto-reveal, no auto-grade, no logging, no
sound. Resets per card; stops ticking once the answer is revealed. The user
self-scores honestly against it.

## 4. Projected mock score (dashboard)

`/api/stats` response gains `projectedMock`: per-meta-category cold accuracy
× `TARGET_WEIGHTS` fraction × 50, summed → projected score /50, plus the
per-category contribution and headroom (weight × (1 − coldAccuracy) × 50) so
the UI can show which category moves the needle most. Categories with no
cold attempts contribute at a neutral 0.5 accuracy and are flagged
`estimated`. Dashboard renders a tile: big projected score vs the 35 pass
line + top-3 headroom categories.

## 5. Mock miss classification

- Migration 0012 (same file): `miss_kind TEXT NULL CHECK (miss_kind IN
  ('unknown', 'slow', 'wording'))` on `question_attempts` (mock rows only by
  convention).
- New endpoint `POST /api/mock-test/{id}/miss-kind` body
  `{questionId, missKind}` (idempotent update of that test's attempt row).
- Mock results page: each miss row gets three small tag buttons — "Didn't
  know" / "Knew, too slow" / "Wording/typo" — writing the tag inline; the
  results header shows the tag breakdown once any are set. Tags are
  informational (add-misses-to-SRS unchanged).

## 6. Vocabulary & etymology coverage

- Drill page gains preset chips (client-side links to the existing keyword
  drill): "Word origins" (`q=from the greek OR from the latin OR word
  meaning OR this word means`) and similar Vocab preset(s) — no backend
  change (drill search already supports websearch_to_tsquery).
- Content op post-deploy: generate 3–5 vocab/etymology primers through the
  existing primer machinery (e.g. Greek & Latin roots, foreign words in
  English, -ology/-phobia families).

## Verification

- Unit tests: adaptive weight multiplication (test-share × weakness, floor
  preserved); projected-score math incl. the no-data neutral case; Pavlov
  category-then-evidence pick helper (pure part).
- Existing suites + frontend build; browser QA of timer behavior (counts,
  stops on reveal, resets on next card, does nothing at 0), settings field
  round-trip, miss-tag round-trip, presets, dashboard tiles.
- Drill handlers still never write `question_attempts` (Pavlov) and the
  timer adds no writes anywhere.

## Out of scope

Timer latency logging (explicitly declined — display only); auto-grading on
timeout; changing add-misses-to-SRS behavior; a dedicated vocab corpus
drill beyond keyword presets.
