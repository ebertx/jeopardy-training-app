# Deck composition bar + weekly trend — design

Date: 2026-07-15
Status: approved (user picked A+B: composition bar with real denominator, plus
weekly deltas)

## Problem

The dashboard deck strip ("143 learning · 267 mastered · 6 struggling") is not
actionable:

- No denominator, and review cards with interval < 21 days that aren't
  struggling appear in no bucket, so the numbers don't sum to the deck.
- Buckets overlap: struggling (`suspended OR lapses >= 4`) is state-independent,
  so a recovered card can count as both mastered and struggling.
- No trend — maturing-vs-treading-water is invisible.

## Design

### Buckets (mutually exclusive, sum = active deck)

Struggling takes precedence (it's the actionable bucket):

1. **struggling** — `suspended OR lapses >= 4`
2. **learning** — else `state IN ('learning','relearning')`
3. **maturing** — else `state = 'review' AND interval_days < 21`
4. **mastered** — else `state = 'review' AND interval_days >= 21`

Scope unchanged: cards on non-archived questions.

### Backend

- **Migration `0007_deck_snapshots.sql`** (additive):
  `srs_deck_snapshots(user_id FK, snap_date DATE, learning, maturing, mastered,
  struggling INTEGER NOT NULL, PRIMARY KEY (user_id, snap_date))`.
  Applied to the live DB with `scripts/apply-migration.sh` (established manual
  flow — the container does not run migrations).
- **`/api/practice/status`**: compute the four buckets in one `COUNT(*) FILTER`
  query; upsert today's row (date = user's local date via the same chrono-tz
  parse as `day_start_utc`); pick a baseline snapshot — newest with
  `snap_date <= today - 7 days`, else the oldest one before today, else none.
  Response shape:
  `deck: { learning, maturing, mastered, struggling, total,
  delta: { since: "YYYY-MM-DD", learning, maturing, mastered, struggling } | null }`
  (deltas = today − baseline). The first snapshot is written on this deploy's
  first dashboard load, so `delta` stays null until a second day exists.
- **`/api/cards`**: same exclusive predicates; add `maturing` to the state
  whitelist. `due` unchanged.

### Frontend

- Dashboard deck strip becomes: "Deck · N cards" header, a stacked composition
  bar (learning amber `#f59e0b`, maturing light blue `#60a5fa`, mastered green
  `#22c55e`, struggling red `#ef4444`), and a legend of counts linking to
  `/cards?state=...` as today. When `delta` exists, a muted line shows
  "since <date>: mastered +18 · struggling −2" (the two goal-relevant deltas).
- Cards page gains a "Maturing" tab (same whitelist word).

## Testing

- `cargo test` (bucket predicates are SQL; no new pure logic worth unit
  testing except none), `svelte-check`.
- Visual verification with the mock API at 390px and 1280px.
- After deploy: apply migration, then confirm `/api/practice/status` returns
  the new `deck` shape and a snapshot row appears.
