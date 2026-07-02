# Navigation Consolidation — Design Spec

**Date:** 2026-07-02
**Status:** Approved (design), pending implementation plan

## 1. Problem & goal

The nav carries eight destinations (Dashboard, Practice, Drill, Coryat, Review,
Mastered, Study, Settings), several of which are pre-SRS concepts: **Review** is a
list of not-yet-mastered cards plus an in-page quiz whose grades still post to the
legacy `/api/quiz/submit` and therefore never reach the SRS scheduler; **Mastered**
is a random viewer over long-interval cards whose only real function is the reset
button; **Study** is an occasional tool holding a daily-destination slot. Goal: a
navigation that matches how the app actually works — *Dashboard tells you about
your deck; Practice, Drill, and Coryat are the three ways to play.*

## 2. Decisions (locked)

| Decision | Choice |
| --- | --- |
| Nav | **Dashboard · Practice · Drill · Coryat · Settings** (+ Admin for admins) |
| Review + Mastered | **Deleted**, replaced by one nav-less `/cards` deck browser reached from the Dashboard |
| Study | Page survives at `/study`; leaves the nav; reached from a Dashboard card |
| Old URLs | `/review` and `/mastered` redirect to `/cards` |
| Legacy quiz mode | The Review page's in-session quiz (grades bypassed SRS) is removed, not ported |
| Schema | **No migration** — reads existing `srs_cards`/`jeopardy_questions` only |

## 3. `/cards` — the deck browser (nav-less sub-page)

- **Filters:** state chips — **Learning** (`state IN ('learning','relearning')`),
  **Due soon** (`due <= now() + interval '24 hours'`, unsuspended), **Mastered**
  (`state='review' AND interval_days >= 21`), **Struggling** (`suspended = true OR
  lapses >= 4`) — plus the existing classifier-category dropdown. Default chip:
  Learning.
- **Rows:** clue text (truncated), show-category, state label, interval/due ("due in
  3d" / "due 4:12 PM"), lapses when > 0; expandable to full clue + response +
  air date (same expand pattern as the old Review list).
- **Row actions:** **Reset progress** (existing `POST /api/mastery/reset` — clears
  state/interval/ease/lapses/suspended, due now) with the existing confirm Modal;
  **Archive** (existing `POST /api/questions/{id}/archive`).
- **Header:** count for the active filter; Done button → dashboard (per app
  convention); `<svelte:head>` title "Cards — Jeopardy! Training".

## 4. New endpoint

`GET /api/cards?state=learning|due|mastered|struggling&category=<all|classifier>`
→ `{ cards: [{ id, question, answer, category, classifier_category, clue_value,
round, air_date, state, interval_days, due, lapses, suspended }], total: i64 }`,
ordered soonest-due first, `LIMIT 200` (log-free truncation is acceptable at
current deck sizes; the `total` field tells the UI when more exist). `state` is
validated against the four values (400 otherwise); `category` is bound, never
interpolated — same injection discipline as the other pickers.

## 5. Dashboard as hub

Inside the existing SRS summary card:
- **Deck strip** under the stat tiles: "N learning · N mastered · N struggling",
  each count a link to `/cards?state=...`. Counts come from a new `deck` object on
  `GET /api/practice/status`: `{ learning, mastered, struggling }` (same SQL
  definitions as §3's filters).
- **Study card**: a small card after the SRS card — "Study sheets — generate
  targeted reading from your recent misses →" linking to `/study`, showing the
  last-generated date from `GET /api/study/latest` (already exists; non-fatal if
  it fails).

## 6. Deletions

- Pages: `frontend/src/routes/review/`, `frontend/src/routes/mastered/` — replaced
  by stub `+page.ts` redirects to `/cards` (SvelteKit `redirect(301, '/cards')`).
- Component: `MasteryBadge.svelte` (only consumer was Review).
- Backend routes + handlers: `GET /api/review` (`routes/review.rs`),
  `GET /api/mastered` (`random_mastered` in `routes/mastery.rs`). The `reset`
  handler and its route stay. Route registrations updated in `main.rs`;
  `pub mod review;` removed.
- Nav entries: Review, Mastered, Study removed from `Nav.svelte`'s links array.

## 7. Testing & verification

- Backend: `cargo test` green (no new pure logic — the cards query is
  build/clippy + deferred live check, consistent with prior phases); clippy clean
  except the 2 baseline warnings; confirm no references to the deleted
  handlers/module remain.
- Frontend: `npm run check` + `build`; `grep -rn "/review\|/mastered"
  frontend/src` shows only the redirect stubs and API-unrelated matches;
  redirects verified by type-check (SvelteKit `redirect` in `+page.ts`).
- Deploy: standard push → GHCR → container swap. **No DB migration.**

## 8. Out of scope

- Renaming or redesigning Study's own page.
- Pagination beyond LIMIT 200 on `/cards`.
- The mobile Coryat board fix and other audit items.
