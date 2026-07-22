# Pavlov Cues — Design

**Date:** 2026-07-21
**Status:** Approved

## Goal

Compile a large, corpus-grounded list of Jeopardy "Pavlov cues" — the signature
keyword→answer associations clue writers reuse (e.g. "wise king" → Solomon) —
apportioned to match the Anytime Test category makeup, and drill them in a
dedicated SRS deck separate from regular clue practice.

Motivation: Colin Davy's data-science prep
(<https://colindavy.medium.com/how-i-won-jeopardy-with-data-science-c2e9b52a1958>).
His ~1,500 keyword flashcards trained exactly the reflex the 15-second Anytime
Test rewards: trigger phrase → answer, without full clue comprehension. The
app already implements his frequency-weighted study (blend module,
`answer_freq`); this adds the missing piece.

## Scope

- ~1,500 cue entries, seats apportioned by `blend::TARGET_WEIGHTS`
  (≈300 Literature & Language … ≈30 Sports & Games).
- Cue generation: statistical mining from the clue corpus + LLM polish pass.
- Drill: a separate SRS deck (`pavlov_cards`) reusing the SM-2 scheduler.
- Browse: a searchable "list" page grouped by meta-category, doubling as the
  QA surface for LLM output.

Out of scope: buzzer training, wagering strategy, any change to the main
practice queue, mock tests, or cold-accuracy stats.

## 1. Cue mining pipeline

A three-stage batch, triggered from an admin route, running as a background
job with pollable progress.

### Stage 1 — seat selection

- Reuse `blend::TARGET_WEIGHTS` and the existing `apportion()` to split 1,500
  seats across the 13 meta-categories.
- Within each category, rank **distinct normalized answers** and take the top
  N for that category's seats:
  - Normalization = the same expression as the `answer_freq` backfill
    (lowercase, trim, strip leading article).
  - Eligibility floor: `answer_freq >= 5` (below that, signature terms are
    statistically shaky).
  - Ranking mirrors `sampling_kind()`: Film/TV & Pop Culture and Sports &
    Games rank by recency-decayed frequency (6-year half-life on `air_date`,
    same decay as the mock-test blend); Music & Performing Arts splits its
    seats canon/recency via `split_seats()`; all other categories rank by raw
    `answer_freq`.

### Stage 2 — signature-term mining (no LLM)

For each selected answer:

- Aggregate lexemes from its clues' existing `search_tsv` column (Postgres has
  already stemmed and stop-worded them).
- Score terms TF-IDF-style: frequency within the answer's clues × inverse
  document frequency against the whole corpus. Exclude terms that are
  substrings/stems of the answer itself (self-referential).
- Keep the top ~8 distinctive terms and the 3 highest-value example clue ids.

### Stage 3 — LLM polish

- Batches of ~15 answers per call through the existing `openai.rs` client
  (~100 calls total, one-time).
- Input per answer: answer text, mined terms, sample clues.
- Output: strict JSON per answer:
  - `cue_phrases`: 2–4 human-readable cue phrases, each grounded in the mined
    terms/sample clues — the LLM rewrites and filters, it never invents
    associations absent from the mined input;
  - `keep`: boolean verdict; answers whose mined terms are junk (generic,
    self-referential) are stored with `status = 'dropped'`.
- Idempotent and resumable: rows are upserted per-answer as each batch
  finishes; re-runs skip answers already polished, so an interrupted batch or
  a later top-up run continues where it left off.

## 2. Data model (migration 0009)

```sql
-- Global, shared (like primers).
CREATE TABLE pavlov_cues (
  id               SERIAL PRIMARY KEY,
  answer           TEXT NOT NULL,          -- display form
  answer_norm      TEXT NOT NULL UNIQUE,   -- answer_freq normalization
  meta_category    TEXT NOT NULL,
  cue_phrases      TEXT[] NOT NULL DEFAULT '{}',  -- LLM-polished
  mined_terms      TEXT[] NOT NULL DEFAULT '{}',  -- raw, kept for audit
  example_clue_ids INTEGER[] NOT NULL DEFAULT '{}',
  answer_freq      INTEGER NOT NULL,
  status           TEXT NOT NULL DEFAULT 'active'
                     CHECK (status IN ('active', 'dropped')),
  model            TEXT NOT NULL,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Per-user drill state; same shape as srs_cards but keyed to cues.
CREATE TABLE pavlov_cards (
  id            SERIAL PRIMARY KEY,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  cue_id        INTEGER NOT NULL REFERENCES pavlov_cues(id) ON DELETE CASCADE,
  state         TEXT NOT NULL DEFAULT 'learning',
  interval_days DOUBLE PRECISION NOT NULL DEFAULT 0,
  ease          DOUBLE PRECISION NOT NULL DEFAULT 2.5,
  due           TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_review   TIMESTAMPTZ,
  reps          INTEGER NOT NULL DEFAULT 0,
  lapses        INTEGER NOT NULL DEFAULT 0,
  step_index    SMALLINT NOT NULL DEFAULT 0,
  suspended     BOOLEAN NOT NULL DEFAULT false,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, cue_id)
);
```

A separate card table (rather than widening `srs_cards`) keeps the main
practice queue, stats, and cold-accuracy tracking untouched. The SM-2
scheduling functions in `srs.rs` are reused, not duplicated.

## 3. Drill + browse

### Pavlov Drill page

- Card front: the cue phrases styled as fragments plus a meta-category chip —
  deliberately **not** the full clue.
- Typed answer, graded by the existing `answer_match` logic; the resulting
  rating drives SM-2 scheduling via the shared `srs.rs` functions.
- New-cards-per-day honors the existing `users.new_cards_per_day` setting
  (applied independently per deck).
- Reveal shows the answer plus the real example clues, so each rep also
  reinforces how the association appears in the wild.
- Drill attempts do **not** write to `question_attempts` (they are cue reps,
  not clue attempts) and never affect cold-accuracy stats.

### Cue browser ("The List")

- Grouped by meta-category in test-weight order; searchable.
- Each row: answer, cue phrases, answer frequency.
- Per-row suspend for cues the user deems worthless (sets `suspended` on the
  user's card; global `status` changes stay admin-only).
- Doubles as the QA surface for LLM output before seeding cards.

### Routes (new module `routes/pavlov.rs`)

- `POST /admin/pavlov/generate` — start/resume the batch; `GET
  /admin/pavlov/status` — progress.
- `GET /pavlov/cues` — browser listing (filter by category, search).
- `GET /pavlov/drill/next`, `POST /pavlov/drill/answer` — drill loop,
  mirroring the existing practice endpoints' shape.

## 4. Verification

- Unit tests: seat apportionment across the 13 categories sums to 1,500 and
  respects the frequency floor; LLM response JSON schema parsing (client
  mocked behind the existing `openai.rs` pattern); self-referential-term
  exclusion.
- SQL sanity script (PG15-compatible, like `scripts/` blend verification):
  seat counts per category match weights; every `active` cue has ≥2 phrases;
  no duplicate `answer_norm`; mined terms occur in that answer's clues.
- Manual QA pass over the cue browser before seeding drill cards.

## Cost

One-time LLM cost: ~1,500 answers ÷ 15 per batch ≈ 100 calls. Re-runs only
pay for unpolished answers.

## Accepted deviations

- `status` gained a `pending` value (beyond `active`/`dropped`) to support
  resumable generation across batch runs.
- The drill answer endpoint was split into `check` (grade + reveal,
  stateless) and `grade` (SM-2 rating), rather than a single
  `POST /pavlov/drill/answer`.
- The cue browser's search/grouping is implemented client-side; this is
  equivalent to server-side filtering at the current scale (~1,500 rows).
- Example clues shown on reveal are the 3 most recent for the answer, not the
  highest-value ones.
- Suspend-created card rows (which leave `last_review` NULL) are excluded
  from the new-card allowance via `last_review IS NOT NULL` in the
  `drill_next` new-today count.
