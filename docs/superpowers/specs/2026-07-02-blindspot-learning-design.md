# Blind-Spot Learning System — Design Spec

**Date:** 2026-07-02
**Status:** Approved (design, incl. serve-time pregeneration amendment), pending implementation plan
**Replaces:** the AI Study Recommendations tool (used 4 times ever, last 2025-11-21 — its
output was homework: external reading lists at a separate destination, with no follow-through).

## 1. Goal

Fold blind-spot learning into the surfaces the user already lives in, at two moments:

- **Insights** — teach at the moment of error, inside Practice and Drill.
- **Blind-spot packs** — convert recurring miss patterns into a 30-second primer plus a
  one-tap drill of matching clues.

No separate study destination the user must remember to visit; nothing that hands the
user off to Wikipedia.

## 2. Decisions (locked)

| Decision | Choice |
| --- | --- |
| Insight trigger | Auto on **Wrong** (Practice + Drill); on-demand **Explain** link on any revealed card |
| Miss flow | Grading Wrong **pauses** on the card (insight + Next button); correct grades keep instant advance |
| Latency | **Serve-time pregeneration** (amendment): `practice/next` / `drill/next` fire-and-forget insight generation when serving an uncached clue, so the insight is cached before the user can grade. Worst case degrades to a 1–2s inline load; never an error |
| Insight cache | Per **clue**, global (not per user), permanent: `clue_insights` keyed by `question_id` |
| Insight model | `gpt-4o-mini`, JSON `{insight, hook}` (~60–90 words + a one-line memory hook) |
| Pack detection | LLM (`gpt-4o`) clusters the last 30 days of misses into 3–5 **fine-grained** themes (finer than the 13 classifier categories) |
| Pack contract | Per pack: `theme`, `diagnosis` (one line, cites miss count), `primer` (~100 words: the recurring facts/patterns), `search_query` (websearch_to_tsquery-compatible) |
| Pack validation | Backend runs each `search_query` against the full-text index; packs with **≥ 10** matching clues survive, others are dropped |
| Pack lifecycle | Auto-refresh in background when the active set is **> 7 days old AND ≥ 25 new misses** accumulated since generation (checked on `GET /api/blindspots`; in-process in-flight guard prevents duplicate runs). Manual refresh button also exists. New set supersedes old |
| Surfacing | Dashboard **Blind spots** card (replaces the Study card): top 2–3 packs. Nav-less **`/blindspots`** page: all active packs, full primers, drill buttons. `/study` redirects there |
| Drill integration | `/drill` accepts `?q=<query>` and auto-starts; a pack's button is simply a link to that |
| Old tool | All `/api/study/*` endpoints and the Study page removed; `study_recommendations` table kept as inert history |
| Disabled mode | Empty `OPENAI_API_KEY`: pregeneration skips, `GET /api/insight` returns 404, UI hides insight areas and the Blind spots card shows a "not configured" note |

## 3. Insights — detail

**UX.** After reveal, the grade row appears as today plus a small **Explain** link.
Pressing **Got it**/**Too easy** advances instantly (unchanged). Pressing **Wrong**
records the grade immediately (existing `POST /api/practice/grade` — unchanged), then
stays on the card showing the insight panel under the answer: the `insight` text with
the `hook` styled distinctly, plus a **Next** button. Keyboard: Space/Enter/1/2/3
advances past the pause. Explain fetches the same insight inline without grading.

**Endpoint.** `GET /api/insight/{question_id}` → `{insight, hook, cached: bool}`.
Cached row → instant. Uncached → generate, store, return (1–2s). A per-process
single-flight guard (keyed by question_id) prevents duplicate generation when the
pregeneration task and an eager request race; the DB insert uses
`ON CONFLICT (question_id) DO NOTHING` as the backstop.

**Pregeneration.** In `practice::next` and `drill::next`, after selecting the card to
return: if no `clue_insights` row exists for it, spawn a background task that calls the
same generate-and-store function. The serving response is never delayed by this.

**Prompt contract** (pinned in code): system prompt instructs a Jeopardy-aware tutor;
input is clue text, expected response, show category, air date; output STRICT JSON
`{"insight": "...", "hook": "..."}` — insight explains why the answer is what it is and
what pattern the clue used; hook is one memorable line. `response_format: json_object`,
temperature 0.4.

## 4. Blind-spot packs — detail

**Input.** Incorrect attempts from the last 30 days joined to clue text/categories,
grouped by classifier category, capped at 12 misses per category in the prompt (same
shape as the old tool's query). Minimum to generate: 10 misses total — below that,
`GET /api/blindspots` reports `insufficientData: true` and the UI says "keep practicing".

**LLM contract** (pinned): output STRICT JSON
`{"packs": [{"theme", "diagnosis", "primer", "search_query"}]}`, 3–5 packs, themes must
be specific (opera, vice presidents, European rivers — never the broad category names),
`search_query` must be plain search terms usable with `websearch_to_tsquery` (bare
words, quoted phrases, `or` allowed).

**Validation.** For each pack, count matches with the same predicate the drill uses
(`search_tsv @@ websearch_to_tsquery('english', $q)`, non-archived). Keep packs with
`match_count >= 10`; store `match_count` for display ("Drill this — 74 clues").

**Storage.** New set inserts rows and marks the previous set `superseded = true` in one
transaction. `GET /api/blindspots` returns only unsuperseded rows plus
`{generatedAt, stale, insufficientData}`.

## 5. Schema (migration 0004 — additive only)

```sql
CREATE TABLE IF NOT EXISTS clue_insights (
    id           SERIAL PRIMARY KEY,
    question_id  INTEGER NOT NULL UNIQUE REFERENCES jeopardy_questions(id),
    content      JSONB NOT NULL,            -- {"insight": "...", "hook": "..."}
    model        TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS blindspot_packs (
    id           SERIAL PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    theme        TEXT NOT NULL,
    diagnosis    TEXT NOT NULL,
    primer       TEXT NOT NULL,
    search_query TEXT NOT NULL,
    match_count  INTEGER NOT NULL,
    miss_count   INTEGER NOT NULL DEFAULT 0, -- misses analyzed when generated
    superseded   BOOLEAN NOT NULL DEFAULT false,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_blindspot_packs_user_active
  ON blindspot_packs (user_id, superseded, created_at DESC);
```

## 6. API summary

- `GET /api/insight/{question_id}` — cached-or-generate (single-flight); 404 when the
  key is unconfigured.
- `GET /api/blindspots` — active packs + `{generatedAt, stale, insufficientData}`;
  when stale, kicks a background regeneration (AtomicBool in-flight guard in AppState)
  and still returns the current set.
- `POST /api/blindspots/generate` — synchronous manual refresh (page button).
- **Removed:** `POST /api/study/generate`, `GET /api/study/latest`,
  `GET /api/study/history`, `routes/study.rs`, and the `/study` page (redirect stub →
  `/blindspots`, same pattern as `/review` → `/cards`).

## 7. Frontend

- **Practice + Drill:** insight panel in `QuestionCard`'s answer area (new optional
  snippet or a sibling panel — implementation's choice, consistent across both pages);
  Wrong-pause with Next; Explain link post-reveal; keyboard advance.
- **`/blindspots`** (nav-less): pack list — theme, diagnosis, primer, "Drill this
  (N clues)" → `/drill?q=<encoded query>`; refresh button; generated-at line;
  insufficient-data and not-configured states. Title tag per app convention; Done →
  dashboard.
- **`/drill`:** on mount, if `?q=` present, set the search box and auto-start that
  drill.
- **Dashboard:** the Study card becomes the **Blind spots** card — top 2–3 packs
  (theme + diagnosis, click-through to `/blindspots`); "keep practicing" note when
  insufficient data.

## 8. Testing

- Pure units: staleness rule (age + new-miss threshold), LLM JSON parsing/validation
  for both contracts (good, malformed, missing-field, non-JSON inputs), pack
  validation filter (match-count threshold), search-query encoding for the drill link.
- LLM calls live behind two small functions (`generate_insight`, `generate_packs`)
  with pinned request/response contracts; no live-call tests (no key in CI; live
  verification post-deploy).
- Usual gates: `cargo test`/clippy (2 baseline warnings), `npm run check`/`build`.
- Deploy: migration 0004 applied manually on Tower BEFORE the container swap.

## 9. Cost

`gpt-4o-mini` at $0.15/$0.60 per M tokens → ~$0.00016 per insight (~530 tokens).
Serve-time pregeneration generates per clue *seen* (once ever, globally cached) —
under a penny/day, decaying as the cache fills. Pack refresh (`gpt-4o`) ~ a few cents,
roughly weekly. Total comfortably under $1/month.

## 10. Out of scope

- Lesson cards mixed into the Practice queue.
- Insight regeneration/versioning (an insight, once cached, is permanent).
- Configurable models/providers; streaming responses.
- Coryat integration (game mode stays pure).
