# Mock Test Mode, Cold/Review Stats Breakout, SRS Interleaving — Design

**Date:** 2026-07-08
**Status:** Approved by Christian (all four sections)
**Target codebase:** `backend/` (Rust, axum + sqlx) and `frontend/` (SvelteKit, Svelte 5 runes, Chart.js, Tailwind). The root Next.js `app/` tree and `prisma/` are the dead pre-rewrite codebase — do not build there. Schema source of truth is `backend/migrations/*.sql`.

## Motivation

Christian is prepping for the Jeopardy! Anytime Test: 50 clues, one per category, 15 seconds each, typed answers, no per-question feedback, believed pass ≈35/50, one attempt per rolling 12 months. Analysis (2026-07-08) showed repeat-inflated practice accuracy of 77–81% but **first-attempt ("cold") accuracy flat at ~50% for nine months** — and the app currently cannot distinguish the two. Three features:

1. **Mock test mode** — realistic simulation with Jeopardy-style forgiving grading.
2. **Cold vs. review stats breakout** — make the test-relevant metric visible.
3. **SRS interleaving** — new cards from the start of the day, mixed with due reviews (currently all reviews are served before any new card).

## Decisions made with user

| Question | Decision |
|---|---|
| Grading | Algorithmic auto-grade (phonetic + fuzzy) with end-of-test self-override; overrides logged |
| Category mix | Mirror live show distribution from the clue DB |
| Difficulty | Mid-band only: J! $600–$1000, DJ! $800–$1200 |
| Dashboard | Cold accuracy is the headline metric; review secondary; mock readiness tile |
| SRS ordering | Probabilistic interleave `p = new_remaining / (new_remaining + due_count)` |

## 1. Data model (one migration: `backend/migrations/0005_mock_test_and_attempt_kind.sql`)

### `question_attempts.attempt_kind`

- New column `attempt_kind TEXT NOT NULL DEFAULT 'review' CHECK (attempt_kind IN ('new','review','mock'))`.
- **Backfill:** for each `(user_id, question_id)`, the earliest attempt (by `answered_at`, tie-break `id`) becomes `'new'`; all later ones `'review'`. Window-function `UPDATE`. This matches the definition used in the 2026-07-08 analysis, so history carries over.
- **Write path:** set server-side at grade time — `'review'` if a prior `question_attempts` row exists for this user+question, else `'new'`. Mock grading writes `'mock'`. Never trusted from the client. (The `isNew` flag computed in `/api/practice/next` remains display-only.)
- Index: `(user_id, attempt_kind, answered_at)` to keep stats queries cheap.

### `mock_tests`

| column | type | notes |
|---|---|---|
| id | serial PK | |
| user_id | int FK users, cascade | |
| started_at / completed_at | timestamptz / nullable | |
| question_ids | int[] | the 50 ids, in serve order, fixed at start |
| current_index | int default 0 | resume pointer |
| score | int nullable | final count of `final_correct`, set at completion |

### `mock_test_answers`

| column | type | notes |
|---|---|---|
| id | serial PK | |
| mock_test_id | int FK mock_tests, cascade | |
| question_id | int FK jeopardy_questions | |
| position | int (0–49) | unique with mock_test_id |
| typed_answer | text | raw as submitted (may be empty on timeout) |
| response_ms | int | |
| auto_correct | bool | matcher verdict, immutable |
| overridden | bool default false | user flipped the verdict on the results screen |
| final_correct | bool | `auto_correct` unless overridden |

Mock grades ALSO insert a `question_attempts` row (`attempt_kind='mock'`, `correct=final_correct`; on override, update that row) so blindspot analysis sees mock misses. Mock attempts are **excluded** from practice accuracy stats. A mock-attempted clue counts as "seen" for future mock selection but does NOT create an SRS card unless the user opts in (see §2).

## 2. Mock test

### Backend (`backend/src/routes/mock_test.rs`)

- `POST /api/mock-test` — create test. Selection: 50 clues where `archived=false`, mid-band value, `classifier_category IS NOT NULL`, no prior `question_attempts` for this user, not in the user's `srs_cards`. Category quotas: compute the mid-band category distribution once at test creation (`GROUP BY classifier_category` over the eligible pool) and apportion 50 seats by largest remainder (July 2026 snapshot: History 8, Science 6, Geography 6, Literature 6, Film/TV 5, Music 4, Phil/Religion 3, Business 3, Sports 3, Art 2, Misc 2, Tech 2, Math 1 — but computed live, not hardcoded). Random within category, then shuffle overall order. Refuse creation if an incomplete test exists (return it instead — resume).
- `GET /api/mock-test/current` — the active test's `current_index` clue (id, category display name, clue text) plus progress. Never returns future clues (no lookahead) and never returns the accepted answer mid-test.
- `POST /api/mock-test/answer` — body: `{typed_answer, response_ms}` for the current index only. Grades via the matcher, stores the row, inserts the `question_attempts` mock row, advances `current_index`. Returns only progress — no verdict.
- `POST /api/mock-test/complete` — invoked automatically after answer 50; computes `score`, sets `completed_at`.
- `GET /api/mock-test/{id}/results` — full review payload (only for completed tests): per clue: clue text, category, accepted answer, typed answer, auto/final verdicts, response_ms; summary score vs. pass line 35.
- `POST /api/mock-test/{id}/override` — body `{position, correct}`; only on completed tests; sets `overridden=true`, `final_correct`, recomputes `score`, updates the matching `question_attempts` row.
- `POST /api/mock-test/{id}/add-misses-to-srs` — opt-in; creates `srs_cards` (state `learning`, due now) for `final_correct=false` questions not already carded.
- `GET /api/mock-test/history` — list of completed tests (date, score) for the readiness tile.

Conventions: axum handler signature `async fn(State<Arc<AppState>>, auth: AuthUser, ...) -> Result<Json<Value>, AppError>`, sqlx `$n` binds, module registered in `routes/mod.rs` + `main.rs`.

### Answer matcher (`backend/src/answer_match.rs`, pure functions, unit-tested)

Accepted-answer strings use J!-Archive conventions the parser must expand first:

- `(Thomas) Cromwell` → parenthetical segments optional: accept `thomas cromwell` and `cromwell`. (~26k clues contain parentheses.)
- `the U.S.S.R. (or Soviet Union)` / `X or Y` → alternates, each graded independently.
- Escaped/straight quotes around titles → stripped.
- `rappel(ing)` → inline optional suffix: accept `rappel` and `rappeling`.

Normalization (applied to every variant and to the typed answer): Unicode NFKD + diacritic strip, lowercase, punctuation → space, collapse whitespace, drop leading articles (`the`, `a`, `an`), trim. Empty typed answer is always wrong (timeout case).

Acceptance tiers (any passes ⇒ `auto_correct=true`):
1. Exact normalized match against any variant.
2. Damerau-Levenshtein distance ≤ 1 (variant length < 8 chars) or ≤ 2 (≥ 8 chars) — typo forgiveness.
3. Double Metaphone equality, token-aligned (same token count, each token phonetically equal) — the show's "phonetically correct without adding or dropping sounds" rule.

Crates: `strsim` (Damerau-Levenshtein), `rphonetic` (Double Metaphone), `unicode-normalization` + `deunicode` (diacritics). All pure Rust, no network. Known limitation (accepted): the matcher can't recognize legitimately different alternate answers (e.g., a nickname the show would accept) — that's what the override is for.

### Frontend (`frontend/src/routes/mock/+page.svelte` + results view)

- Start screen: explains format, shows remaining unseen-clue depth, Start button. If an incomplete test exists: Resume.
- Test screen: category name, clue text, one text input (autofocused), 15-second countdown bar (client `setInterval`, drift-corrected against a `performance.now()` deadline). At 0:00 the current input auto-submits. Enter submits early. No back, no pause, no verdict shown. Progress `n/50`. Mid-test refresh: server resumes at `current_index`; the abandoned clue's answer was either already submitted or (if never submitted) is submitted as empty on resume — clue timers are not restartable.
- Results screen: score banner vs. the 35 line ("31/50 — 4 short of the commonly-cited pass line"), full 50-row review with typed vs. accepted, verdict chips, override toggle per row (calls the override endpoint, updates score live), and the add-misses-to-SRS button.
- Styling: existing Tailwind `jeopardy-blue`/`jeopardy-gold` palette; components in `$lib/components`; API via `$lib/api`.

## 3. Dashboard cold/review breakout

### Backend (`backend/src/routes/stats.rs`)

Split every accuracy aggregation by `attempt_kind`:
- `overall`: `{cold: {n, pct}, review: {n, pct}}` (mock excluded from both).
- `categoryBreakdown`: per category, cold n/pct and review n/pct.
- `dailyAccuracy` (30d): two series, cold and review.
- New `mockReadiness`: from `mock_tests` — list of `{date, score}`, best, latest.

### Frontend (`frontend/src/routes/dashboard/+page.svelte`)

- Headline stat becomes **Cold accuracy (30d)** — big number, labeled "first-attempt accuracy — the number the Anytime Test measures".
- Review accuracy as a secondary panel labeled retention.
- Trend chart: cold as the primary line, review as a muted secondary line (Chart.js, existing `StatsChart.svelte` patterns).
- Category chart driven by cold pct (review pct available on hover/secondary bars).
- New **readiness tile**: latest + best mock score against a 35/50 pass line, sparkline of mock history, link to `/mock`.

## 4. SRS interleaving (`backend/src/routes/practice.rs::next`)

Current behavior: any due review always wins; new cards only when zero reviews are due — i.e., new cards arrive at the end of the day.

New behavior: compute `due_count` and `new_remaining` (existing queries). If both > 0, draw a new card with probability `new_remaining / (new_remaining + due_count)`, else a due review; if only one is available, serve it. New cards therefore appear from the first pull of the day and spread proportionally through the queue; the daily allowance logic (`new_cards_per_day`, local-midnight reset, adaptive targeting in `pick_new_clue`) is unchanged. Applies to `/api/practice/next` only; drill tiering is unchanged.

## Error handling

- Mock answer submitted for a non-current position → 409 (client resyncs via `/current`).
- Mock creation with insufficient eligible clues in some category → borrow seats from the next-largest category (never fail; log the substitution). Pool is ~196k clues today, so this is a safety valve only.
- Matcher panics are impossible by construction (pure string ops); malformed accepted-answer strings degrade to exact-match-only.
- Override on an incomplete test → 400.

## Testing

- **Rust unit tests** for the matcher: table-driven cases covering parenthetical-optional, `or`-alternates, quoted titles, inline `(ing)` suffixes, diacritics (Häagen → haagen), articles, phonetic passes (guevara/guhvara), phonetic failures (added/dropped sounds), edit-distance boundaries, empty answer.
- **Rust unit tests** for quota apportionment (largest remainder sums to 50; borrow-on-shortfall).
- **Backfill verification query** in the migration PR: cold counts per category must match the 2026-07-08 analysis numbers.
- **sqlx integration tests** (existing pattern if present; else manual verification) for the mock lifecycle: create → 50 answers → complete → override → score recompute.
- **Manual verification** (`/verify` flow): run a full mock test in the browser, confirm timer auto-submit, refresh-resume, override, readiness tile update; confirm practice interleaving serves a new card early in the day.

## Out of scope (explicitly)

- Topic primer pages (structured study guides) — discussed, deferred to a follow-up design.
- Deleting the dead Next.js `app/`/`prisma/` trees — separate cleanup.
- Wordplay-style category simulation (Before & After etc.) — the DB's real categories already include these clue styles.
- Anti-cheat beyond no-lookahead/no-back (this is a self-honesty tool).
