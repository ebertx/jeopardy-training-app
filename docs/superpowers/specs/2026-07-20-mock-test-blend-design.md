# Mock Test Blend Redesign — Fixed Weights, Canon & Recency Sampling

**Date:** 2026-07-20
**Status:** Approved
**Supersedes:** sampling section of `2026-07-08-mock-test-and-stats-design.md` (test size, pass line, session anchoring, and resume flow are unchanged)

## Problem

The mock test currently apportions its 50 seats proportionally to the user's *unseen mid-band corpus pool* and draws uniformly at random within each category. Comparison against three real Anytime Tests (Jan 28–30, 2020; 150 questions, answers archived by The Jeopardy Fan) shows the real test differs systematically:

- **Literature & Language ≈ 20%** of the real test (corpus: ~12%), including several straight vocabulary questions per test.
- **Sports, Business, Art ≈ 2% each** (corpus: 4–6%).
- **Pop culture is current** (2019–2020 material on a 2020 test); uniform corpus draws serve 1990s-era clues.
- **Answers are canonical** — famous entities asked directly (Tolstoy, Thames, Macbeth). Uniform mid-band draws skew obscure; user scored 18/50 vs ~48% practice cold accuracy on the same band.

## Design

### 1. Fixed target weights

Replace the pool-proportional `dist` query in `create()` (`backend/src/routes/mock_test.rs`) with a constant table:

| Category | Weight |
|---|---|
| Literature & Language | 20 |
| Geography & Exploration | 14 |
| History & Politics | 13 |
| Science & Nature | 11 |
| Film, TV & Pop Culture | 10 |
| Philosophy, Religion & Society | 6 |
| Music & Performing Arts | 6 |
| Miscellaneous | 6 |
| Technology & Engineering | 4 |
| Mathematics & Logic | 4 |
| Art & Culture | 2 |
| Business & Economics | 2 |
| Sports & Games | 2 |

Weights sum to 100. Existing largest-remainder `apportion()` runs over these weights, restricted to categories with a non-empty eligible pool. Existing shortfall top-up is unchanged.

### 2. Canon weighting (academic categories)

- New migration `0008_answer_freq.sql`: add `answer_freq INTEGER NOT NULL DEFAULT 1` to `jeopardy_questions`, backfilled as the corpus-wide count of the clue's normalized response (`lower(trim())`, leading `the/a/an` stripped). Applied manually via `apply-migration.sh` per project convention.
- Within academic categories, replace `ORDER BY random()` with weighted sampling, weight `ln(1 + answer_freq)`, via `ORDER BY -ln(random()) / ln(1 + answer_freq) LIMIT n` (exponential-race weighted sampling).
- Effect: freq-100+ answers ~5× likelier per clue than one-offs; recurring answers (≥2 occurrences) cover 72% of the corpus, so most picks are canon.

### 3. Recency weighting (pop-culture categories)

- **Film, TV & Pop Culture** and **Sports & Games**: weight = recency decay on `air_date` with a 6-year half-life (`exp(-ln(2) * age_years / 6)`), same `ORDER BY -ln(random())/weight` mechanism. Corpus runs to 2025-07, so most picks land 2019+ with occasional evergreens.
- **Music & Performing Arts**: seats split 50/50 (larger half canon) — half canon-weighted (composer/Broadway slot), half recency-weighted (current-artist slot), mirroring the real test's consistent inclusion of both.
- All other categories: canon-weighted per section 2.

### 4. Unchanged

Mid-band value filter ($600–1000 J / $800–1200 DJ), unseen-question exclusions (no prior attempts, no SRS card), shortfall top-up, shuffle, quiz-session anchoring, resume flow, scoring, TEST_SIZE=50, PASS_LINE=35.

## Testing

- Unit: weight table sums to 100 and apportions to exactly 50; apportionment restricted to available categories still yields 50; Music seat split arithmetic (odd seat counts → canon side gets the extra).
- Integration (dev DB): generate many tests; assert per-category histogram matches targets within tolerance; assert Film/TV picks' median air_date ≥ 2018; assert mean `answer_freq` of academic picks exceeds pool mean.
- Manual: create a mock test via local dev setup (mock API / `VITE_API_PROXY`) and eyeball the 50 clues.

## Risks / later tweaks

- Film/TV is recency-only; if picks feel non-canonical, blend canon×recency there (one-line change).
- `answer_freq` is a static backfill; new corpus imports would need re-backfill (acceptable: corpus is effectively static).
