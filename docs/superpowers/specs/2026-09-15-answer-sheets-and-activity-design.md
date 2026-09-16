# Answer Sheets, Fact Cards, and Active Days — Design

**Date:** 2026-09-15
**Status:** Approved (Christian chose: automatic sheet on Pavlov Wrong; fact
cards live in the existing Pavlov deck; active-days strip on the dashboard)

## Why

Diagnosis from the three mocks (18 / 21 / 20): 90% of mock answers had never
been met in any training, and cards that HAD been drilled still missed when
the clue came from a different angle (Volkswagen, Visigoths, House of the
Seven Gables). The unit of learning is the clue or the cue, not the answer.
Two fixes:

1. **Answer sheets** — a compact, corpus-grounded identity for an answer
   ("the four things everyone knows"), shown automatically when a Pavlov
   card is rated Wrong and inside the practice teaching pause on a miss.
2. **Fact cards** — the sheet's four facts become four extra Pavlov cards
   (prompt → response) so the topic is drilled in depth, not just the cue.

Plus **active days** on the dashboard, because consistency (19 active days in
the 8 weeks between mocks 2 and 3) is the single biggest lever.

Typed answers in practice were considered and rejected by Christian
(friction). External study (canon lists) deferred until this ships.

## 1. Data (migration 0015)

```sql
CREATE TABLE IF NOT EXISTS answer_sheets (
  answer_norm TEXT PRIMARY KEY,          -- same normalization as pavlov_answers.answer_norm
  answer      TEXT NOT NULL,             -- display form used in the prompt
  content     JSONB NOT NULL,            -- {"identity": "...", "facts": [{"prompt","response"} ×4]}
  model       TEXT NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE pavlov_answers
  ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'answer'
    CHECK (kind IN ('answer', 'fact')),
  ADD COLUMN IF NOT EXISTS parent_norm TEXT;
CREATE INDEX IF NOT EXISTS idx_pavlov_answers_kind_parent ON pavlov_answers (kind, parent_norm);

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS pavlov_auto_facts BOOLEAN NOT NULL DEFAULT false;
```

Fact-card rows in `pavlov_answers`:

| column | value |
|---|---|
| `kind` | `'fact'` |
| `answer_norm` | `"<parent answer_norm>::<n>"`, n = 1..4 (UNIQUE-safe, never collides with a real answer) |
| `parent_norm` | parent's `answer_norm` |
| `answer` | the fact's `response` |
| `phrases` / `phrase_tiers` | `[prompt]` / `['standard']` |
| `meta_category`, `score`, `answer_freq`, `example_clue_ids` | copied from the parent |

Every existing consumer of `pavlov_answers` must decide what facts mean to it:

- `pick_new_card` and the generation stale-cleanup (`DELETE ... WHERE answer_norm NOT IN (...)`) filter `kind = 'answer'` — facts are never introduced by the daily allowance and never deleted by regeneration.
- Deck progress `deckTotal` counts `kind = 'answer'` only. `touched` counts the user's `pavlov_cards` whose answer is `kind = 'answer'`. Fact cards are extra depth, not deck coverage.
- Deck buckets, forecast, due counts, review log, category rollups include facts (they are real reviews in the queue).
- `/pavlov/list` shows facts under their category with the parent label.

## 2. Sheet generation (`backend/src/sheets.rs`, mirrors `insights.rs`)

`ensure_sheet(state, answer_norm) -> Result<Option<SheetContent>>`: cache hit →
return; no API key → `None`; single-flight via an `AppState` inflight set
keyed by `answer_norm`; else generate, validate, insert, return.

Model: `gpt-4o-mini` (`SHEET_MODEL`). Grounding: up to 15 clues for the
answer from `jeopardy_questions` (normalized response = `answer_norm`,
`archived = false`), spread across air dates (order by `air_date`, pick
evenly). The display `answer` is the parent's `pavlov_answers.answer` when it
exists, else the first clue's `question`.

System prompt (fixed text, unit-tested only via the pure builder):

- Output ONLY JSON `{"identity": "...", "facts": [ {"prompt": "...", "response": "..."} ]}` with EXACTLY four facts.
- `identity`: one sentence, ≤ 25 words, who/what this is — proper nouns, dates, numbers; never restate a clue.
- Each fact: the thing Jeopardy writers re-use about this answer that the supplied clues show — author/creator, signature work, date/first, place, counterpart, nickname. `prompt` is a 2–6 word label as it would appear in a cue ("author", "narrator", "year published", "capital city"). `response` is a short gradeable answer: a name, number, place, or title — ≤ 6 words, never a sentence, never the answer itself.
- Facts must be distinct from each other and from the cue phrases already used for this answer (the parent's `phrases` are supplied so the sheet adds depth rather than repeating the cue).
- BANNED: generic sentences about trivia, Jeopardy, or "recognizing"; hedges ("possibly", "often").

User prompt: `Answer: "<answer>"\nCategory: <meta_category>\nExisting cue phrases: <phrases>\nClues Jeopardy has written for this answer:\n- "<clue>" (<category>, <year>)\n...\nReturn the JSON now.`

`parse_sheet(v) -> Result<SheetContent, String>` rejects: not exactly four facts; any empty `identity`/`prompt`/`response`; a `response` whose normalized form equals `answer_norm`; duplicate prompts. A rejected sheet is NOT cached; the endpoint returns `None` and the UI shows nothing (same as insights).

Prefetch: `drill_next` spawns `ensure_sheet` for the served card (fire-and-forget, like practice does for insights). Practice requests the sheet only inside the teaching pause.

## 3. API

| route | behaviour |
|---|---|
| `GET /api/sheet/answer/{answer_norm}` | `{ identity, facts, factsAdded }` or 204 when unavailable |
| `GET /api/sheet/question/{question_id}` | resolves the clue's normalized response, then as above |
| `POST /api/pavlov/facts` `{ answerNorm }` | upserts the 4 fact rows (`ON CONFLICT (answer_norm) DO UPDATE` the content) and inserts a `pavlov_cards` row (due now, learning) for each the user lacks. Returns `{ added }`. Idempotent. 404 when no sheet exists. |
| `drill_next`, `drill_check` | responses gain `kind` and, for facts, `parent` (display answer) |
| `drill_grade` | when `rating = wrong`, `kind = answer`, and `users.pavlov_auto_facts`, performs the fact insert server-side before responding; response gains `factsAdded: n` |
| preferences | `pavlovAutoFacts` read/write |
| `GET /api/activity` | `{ streak, activeLast28, days: [{ date, active }] × 28 }` in the user's zone |

`factsAdded` = the user has `pavlov_cards` rows for all four `"<norm>::n"` answers.

Active day definition: any `question_attempts` row (any kind, mock included)
OR any `pavlov_reviews` row that local day, OR (history before the review log)
a `pavlov_cards` row created or last reviewed that day. Streak counts
consecutive active days ending today, or ending yesterday when today is not
yet active.

## 4. UI

**Pavlov drill (`/pavlov`)**

- After reveal: `e` or "Learn this answer" toggles the sheet under the answer box: identity line, then the four facts as `prompt → response`. When `factsAdded` is false a "Drill these 4" button (`d`) posts to `/api/pavlov/facts`; afterwards it reads "In your deck".
- Rating **Wrong** on a `kind = answer` card does not advance. The card stays with the sheet open (auto-fetched; if it is not ready yet a small spinner, then the sheet) and a Next button (`Space` / `Enter`). If auto-add is on, the sheet header says "4 fact cards added". Wrong on a fact card advances normally (no nested depth).
- A fact card's prompt renders as usual with a muted "↳ <parent answer>" line above it, on both the prompt and reveal states.
- Countdown timer unchanged.

**Practice (`/practice`)**: inside the existing teaching pause, under the insight: the sheet (same component) with the same "Drill these 4" button. Both requests fire together; the pause does not wait for the sheet.

**Settings**: checkbox "Auto-add fact cards when I miss a Pavlov card" under the Pavlov fields.

**Dashboard**: page-level strip under the header, above the action buttons: "Streak <n> · <k> of last 28 days" and a 28-dot row (today rightmost; active = blue, inactive = gray). Count colour: green ≥ 24, amber ≥ 16, red below.

Shared component `AnswerSheet.svelte` (props: `sheet`, `factsAdded`, `onAddFacts`, `dark: bool` for the Pavlov surface).

## 5. Testing

- Unit (pure): `sheet_user_prompt`; `parse_sheet` (exactly-four, empty fields, response == answer, duplicate prompts); `fact_norm(parent, n)`; `activity::summarize(days: &[bool]) -> (streak, active)` including the today-not-yet-active rule and all-inactive.
- `scripts/verify-sheets.sql`: activity query shape; fact-card upsert + card insert inside `BEGIN … ROLLBACK`; assertion that `kind='fact'` rows are excluded from the deck-total query used by progress.
- Visual via the dev mock API (serves a sheet and an activity series): Pavlov Wrong pause with sheet, fact-card prompt with parent label, practice teaching pause, dashboard strip at 1280 and 390 wide.

## 6. Rollout

Apply 0015 (additive) → push main over SSH → Watchtower → smoke: one real
Pavlov Wrong generates a sheet and pauses; "Drill these 4" creates four
due-now cards; `/api/activity` returns 28 days; Settings toggle persists.
Cost: one gpt-4o-mini call per newly missed answer, cached forever.

## Out of scope (deferred)

Browsable canon library of sheets (item 5 from the review); typed answers in
practice (rejected); sheets for answers outside the Pavlov deck are still
generated on demand from practice misses, but there is no bulk pregeneration.
