# Topic Drilling — Design Spec

**Date:** 2026-07-01
**Status:** Approved (design), pending implementation plan
**Builds on:** the SRS Practice engine (`docs/superpowers/specs/2026-06-30-srs-practice-engine-design.md`), now deployed. This is Phase 2a of the question-selection engine; it also lays the full-text search substrate that adaptive weakness-targeting (Phase 2b) will reuse.

## 1. Problem & goal

Practice paces *passive* discovery — a capped daily trickle of new clues plus due reviews. It gives no way to **deliberately study a specific area**. Goal: let the user type a keyword (e.g. "Impressionism", "Marie Curie", "napoleon") and drill matching clues on demand, with every grade feeding the existing SRS scheduler so the effort compounds into the normal review queue.

## 2. Decisions (locked)

| Decision | Choice |
| --- | --- |
| SRS relationship | Drilled grades **feed SRS** — reuse `POST /api/practice/grade` untouched; drilled clues create/update `srs_cards`. |
| Aiming a drill | **Keyword search + filters** over clue text + response + show-category name; reuse Practice's `classifier_category` + game-type filters. |
| Search tech | Postgres full-text: a generated `tsvector` column + GIN index, queried with `websearch_to_tsquery`. |
| Session ordering | **Due matches first** (soonest-due), then **new matches** (recency-biased). |
| Daily new-card cap | Drilling is **not gated** by the cap (always serves new matches), but drilled new cards **do** count toward the day's new-introduced total, so Practice's auto-trickle backs off. No schema flag needed. |
| Surface | A new **`/drill`** route, separate from `/practice`. |

## 3. Search & index

Add to `jeopardy_questions` a stored generated column and GIN index:

```sql
ALTER TABLE jeopardy_questions ADD COLUMN IF NOT EXISTS search_tsv tsvector
  GENERATED ALWAYS AS (
    to_tsvector('english',
      coalesce(answer, '') || ' ' || coalesce(question, '') || ' ' || coalesce(category, ''))
  ) STORED;

CREATE INDEX IF NOT EXISTS idx_jq_search_tsv ON jeopardy_questions USING GIN (search_tsv);
```

- In this DB, `answer` holds the clue text shown to the player and `question` holds the expected response; both plus the show `category` name are indexed.
- Queries match with `search_tsv @@ websearch_to_tsquery('english', $q)` — Google-style syntax (bare terms, `"quoted phrases"`, `-exclusions`, `or`), relevance-rankable, fast on ~530k rows.
- **Migration cost:** a STORED generated column triggers a one-time full-table rewrite (ACCESS EXCLUSIVE lock) plus the GIN build — order of tens of seconds on this table. Apply manually on Tower during low use, like migration 0001.

## 4. Session behavior

A drill serves matching clues one at a time until the user stops or the pool is exhausted. Within the matches (all: `question`/`answer`/`classifier_category`/`air_date` non-null, `archived = false`, passing filters, matching the tsquery):

1. **Due matches** — a matching clue that has a non-suspended `srs_card` with `due <= now()`; served soonest-due first (reinforce what's already scheduled on the topic).
2. **New matches** — a matching clue with no `srs_card` for this user; served with the same recency-biased exponential offset as the Practice new-clue picker. **Not gated by the daily new-card cap.**

When both tiers are empty, the drill is done ("You've drilled everything due or new for this topic"). Clues already in the pool but *not yet due* are intentionally **not** re-served — the SRS schedule handles them; cramming already-known clues is out of scope (documented future toggle).

Grading reuses `POST /api/practice/grade` exactly as-is: it creates/updates the `srs_card` (`created_at` defaults to now, so a drilled new card counts toward the day's new total) and records a `question_attempts` row.

## 5. Endpoint

`GET /api/drill/next?q=<query>&category=<all|classifier>&gameTypes=<csv>`

- Validates `q` is non-empty (400 otherwise).
- Returns one of:
  - `{ done: false, isNew: bool, card: {id, question, answer, category, classifier_category, clue_value, round, air_date, notes}, matchCount: i64, remaining: i64 }`
  - `{ done: true, matchCount: i64, remaining: 0 }`
- `matchCount` = total clues matching `q` + filters (for the "312 clues match" header).
- `remaining` = count of tier-1 (due) + tier-2 (new) matches right now; decreases as you grade (new → in-pool, due → future-due), giving a stateless "how many left to hit now".
- Reuses `ClueRow`/`clue_json` from `routes/practice.rs`.
- No new grade endpoint. `q` and `category` are always passed as bound parameters (`$n`); only whitelisted game-type fragments and computed integers are interpolated — same injection-safe pattern as the existing pickers.

## 6. Frontend

New `/drill` route (`frontend/src/routes/drill/+page.svelte`):
- A search box + the existing filter controls (reuse `CategoryFilter`; replicate the small game-type checkbox group from the Practice page).
- Submitting the search calls `/api/drill/next`; the header shows `matchCount` ("312 clues match") and `remaining`.
- The clue renders in the existing `QuestionCard` (3-button mode: `onWrong`/`onGotIt`/`onTooEasy` → `POST /api/practice/grade`); after grading, fetch the next via `/api/drill/next` with the same query.
- Empty/`done` state: "You've drilled everything due or new for '<query>'."
- `Nav.svelte` gains a `{ href: '/drill', label: 'Drill' }` entry.

## 7. Migration, deploy & testing

- New migration `backend/migrations/0002_search_index.sql` (the tsvector column + GIN index). Applied manually on Tower (§3 caveat). Deploy via the standard push → GHCR → Watchtower flow; **apply the migration before the container swaps**, as with 0001.
- Tests:
  - Backend: unit-test any pure helper (e.g. query-param parsing / empty-`q` rejection); the DB-bound `drill/next` handler is verified by `cargo build`/`clippy` + manual curl against a scratch DB (deferred here, no scratch DB) — consistent with the SRS backend tasks.
  - Frontend: `npm run check` = 0 errors, `npm run build` succeeds.

## 8. Out of scope (documented)

- **Cram already-known clues** (re-serving in-pool, not-yet-due matches) — deferred toggle.
- **Live count-as-you-type** — the count comes back with the first `drill/next`; a debounced `/api/drill/count` is a later nice-to-have.
- **Adaptive weakness targeting** (Phase 2b) — will reuse this search/subtopic substrate.
