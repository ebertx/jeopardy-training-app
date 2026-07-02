# Navigation Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the nav to Dashboard · Practice · Drill · Coryat · Settings, replacing the Review and Mastered pages with one nav-less `/cards` deck browser reached from the Dashboard, and moving Study behind a Dashboard card.

**Architecture:** A new `GET /api/cards` endpoint lists SRS cards by state (whitelisted predicate match — same injection discipline as the pickers); `GET /api/practice/status` gains a `deck` count object. The Review/Mastered pages, `MasteryBadge`, and their two backend list endpoints are deleted; old URLs redirect client-side (SPA: `+page.ts` `redirect` + stub `+page.svelte`). No DB migration.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8 runtime-checked), SvelteKit SPA (Svelte 5 runes, adapter-static, `ssr=false`), Tailwind.

## Global Constraints

- **No DB migration** — reads existing `srs_cards`/`jeopardy_questions` only. Never touch any database from this environment.
- State filter definitions (verbatim, spec §3): learning = `sc.state IN ('learning','relearning')`; due = `sc.suspended = false AND sc.due <= now() + interval '24 hours'`; mastered = `sc.state = 'review' AND sc.interval_days >= 21`; struggling = `(sc.suspended = true OR sc.lapses >= 4)`. Invalid `state` → 400. `category` is always bound (`$2`), never interpolated.
- `/api/cards` response: `{ cards: [...], total }`, soonest-due first, `LIMIT 200`.
- Deletions: pages `/review` + `/mastered` (replaced by redirect stubs to `/cards`), `MasteryBadge.svelte`, backend `GET /api/review` (whole `routes/review.rs`) and `GET /api/mastered` (`random_mastered` only — **`reset` and `POST /api/mastery/reset` stay**).
- Nav links array becomes exactly: Dashboard, Practice, Drill, Coryat, Settings.
- Backend: `cargo test` green (25 tests), clippy clean except the 2 baseline warnings (`field email is never read`, `struct QuestionAttempt is never constructed`). Frontend: `npm run check` 0 errors, `npm run build` succeeds; this repo commits `frontend/build`.
- Live curl verification is deferred (no scratch DB; production off-limits) — note in reports.

---

### Task 1: Backend — `/api/cards`, deck counts, endpoint deletions

**Files:**
- Create: `backend/src/routes/cards.rs`
- Delete: `backend/src/routes/review.rs`
- Modify: `backend/src/routes/mod.rs` (drop `pub mod review;`, add `pub mod cards;`)
- Modify: `backend/src/main.rs` (route table)
- Modify: `backend/src/routes/mastery.rs` (remove `random_mastered` + its row struct)
- Modify: `backend/src/routes/practice.rs` (`status`: add `deck` counts)

**Interfaces:**
- Produces: `GET /api/cards?state=&category=` → `{ cards: [{id, question, answer, category, classifier_category, clue_value, round, air_date, state, interval_days, due, lapses, suspended}], total: i64 }`; `GET /api/practice/status` response gains `"deck": { "learning": i64, "mastered": i64, "struggling": i64 }`.
- Consumes: existing `AuthUser`, `AppError`, `AppState` patterns.

- [ ] **Step 1: Create the cards endpoint**

Create `backend/src/routes/cards.rs`:

```rust
use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(sqlx::FromRow)]
struct CardListRow {
    id: i32,
    question: Option<String>,
    answer: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    clue_value: Option<i32>,
    round: Option<i32>,
    air_date: Option<chrono::NaiveDate>,
    state: String,
    interval_days: f64,
    due: chrono::DateTime<chrono::Utc>,
    lapses: i32,
    suspended: bool,
}

/// Browse the user's SRS deck by state. The state predicate comes from a
/// fixed whitelist (never user text); `category` is always bound.
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let state_filter = params.get("state").map(|s| s.as_str()).unwrap_or("learning");
    let predicate = match state_filter {
        "learning" => "sc.state IN ('learning','relearning')",
        "due" => "sc.suspended = false AND sc.due <= now() + interval '24 hours'",
        "mastered" => "sc.state = 'review' AND sc.interval_days >= 21",
        "struggling" => "(sc.suspended = true OR sc.lapses >= 4)",
        _ => {
            return Err(AppError::BadRequest(
                "state must be learning|due|mastered|struggling".into(),
            ))
        }
    };

    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";
    let cat_clause = if use_category {
        " AND jq.classifier_category = $2"
    } else {
        ""
    };

    let count_sql = format!(
        "SELECT COUNT(*) FROM srs_cards sc \
         JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND jq.archived = false AND {predicate}{cat_clause}"
    );
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql).bind(user_id);
    if use_category {
        count_q = count_q.bind(category);
    }
    let total: i64 = count_q.fetch_one(&state.pool).await?;

    let list_sql = format!(
        "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category, \
                jq.clue_value, jq.round, jq.air_date, \
                sc.state, sc.interval_days, sc.due, sc.lapses, sc.suspended \
         FROM srs_cards sc \
         JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND jq.archived = false AND {predicate}{cat_clause} \
         ORDER BY sc.due ASC \
         LIMIT 200"
    );
    let mut list_q = sqlx::query_as::<_, CardListRow>(&list_sql).bind(user_id);
    if use_category {
        list_q = list_q.bind(category);
    }
    let rows: Vec<CardListRow> = list_q.fetch_all(&state.pool).await?;

    let cards: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "question": r.question,
                "answer": r.answer,
                "category": r.category,
                "classifier_category": r.classifier_category,
                "clue_value": r.clue_value,
                "round": r.round,
                "air_date": r.air_date,
                "state": r.state,
                "interval_days": r.interval_days,
                "due": r.due,
                "lapses": r.lapses,
                "suspended": r.suspended,
            })
        })
        .collect();

    Ok(Json(json!({ "cards": cards, "total": total })))
}
```

- [ ] **Step 2: Delete the review module and the random-mastered handler**

- Delete the file `backend/src/routes/review.rs`.
- In `backend/src/routes/mod.rs`: remove `pub mod review;`, add `pub mod cards;` (alphabetical position).
- In `backend/src/routes/mastery.rs`: delete the `random_mastered` handler and the `MasteredRow` struct it uses, plus any imports that become unused (`Query`, `HashMap`, `FromRow`, `rand` — check what remains: the file keeps only `reset` + `ResetBody`). Keep `reset` byte-identical.

- [ ] **Step 3: Update the route table**

In `backend/src/main.rs`:
- Remove: `.route("/api/review", get(routes::review::list))` and `.route("/api/mastered", get(routes::mastery::random_mastered))`.
- Keep: `.route("/api/mastery/reset", post(routes::mastery::reset))`.
- Add (near the practice routes): `.route("/api/cards", get(routes::cards::list))`.

- [ ] **Step 4: Add deck counts to `status`**

In `backend/src/routes/practice.rs`'s `status()`, before the final `Ok(Json(...)))`, add:

```rust
    // Deck strip counts for the dashboard (same definitions as /api/cards).
    let deck: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           COUNT(*) FILTER (WHERE state IN ('learning','relearning')), \
           COUNT(*) FILTER (WHERE state = 'review' AND interval_days >= 21), \
           COUNT(*) FILTER (WHERE suspended = true OR lapses >= 4) \
         FROM srs_cards WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
```

and add to the returned JSON object:

```rust
        "deck": { "learning": deck.0, "mastered": deck.1, "struggling": deck.2 },
```

- [ ] **Step 5: Verify**

Run: `cd backend && cargo test 2>&1 | grep "test result" && cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | tail -3`
Expected: 25 tests pass; clean build; only the 2 baseline warnings. Also run `grep -rn "random_mastered\|routes::review\|mod review" src/` — no matches.

- [ ] **Step 6: Commit**

```bash
git add backend/src/routes/cards.rs backend/src/routes/mod.rs backend/src/main.rs backend/src/routes/mastery.rs backend/src/routes/practice.rs
git rm backend/src/routes/review.rs 2>/dev/null; git add -u backend/src/routes
git commit -m "feat(nav): /api/cards deck browser; retire review + random-mastered endpoints"
```

---

### Task 2: Frontend — `/cards` page, redirects, deletions, Nav

**Files:**
- Create: `frontend/src/routes/cards/+page.svelte`
- Create: `frontend/src/routes/review/+page.ts` and replace `frontend/src/routes/review/+page.svelte` (redirect stub)
- Create: `frontend/src/routes/mastered/+page.ts` and replace `frontend/src/routes/mastered/+page.svelte` (redirect stub)
- Delete: `frontend/src/lib/components/MasteryBadge.svelte`
- Modify: `frontend/src/lib/components/Nav.svelte` (links array)

**Interfaces:**
- Consumes: `GET /api/cards?state=&category=` (Task 1), `POST /api/mastery/reset` `{questionId}`, `POST /api/questions/{id}/archive`, `GET /api/categories`; `CategoryFilter`, `Modal`, `api`.

- [ ] **Step 1: Create the `/cards` page**

Create `frontend/src/routes/cards/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { api } from '$lib/api';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';
  import Modal from '$lib/components/Modal.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  type StateFilter = 'learning' | 'due' | 'mastered' | 'struggling';
  const FILTERS: Array<{ key: StateFilter; label: string }> = [
    { key: 'learning', label: 'Learning' },
    { key: 'due', label: 'Due soon' },
    { key: 'mastered', label: 'Mastered' },
    { key: 'struggling', label: 'Struggling' },
  ];

  interface Card {
    id: number;
    question: string | null; // expected response
    answer: string | null; // clue text shown to the player
    category: string | null;
    classifier_category: string | null;
    clue_value: number | null;
    round: number | null;
    air_date: string | null;
    state: string;
    interval_days: number;
    due: string;
    lapses: number;
    suspended: boolean;
  }

  const initial = page.url.searchParams.get('state');
  let stateFilter = $state<StateFilter>(
    initial === 'due' || initial === 'mastered' || initial === 'struggling' ? initial : 'learning'
  );
  let selectedCategory = $state('all');
  let categories = $state<Array<{ name: string; count: number }>>([]);
  let cards = $state<Card[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let expandedId = $state<number | null>(null);
  let resetTarget = $state<Card | null>(null);
  let busyId = $state<number | null>(null);
  let fetchGen = $state(0);

  async function fetchCards() {
    const gen = ++fetchGen;
    loading = true;
    error = '';
    try {
      const params = new URLSearchParams();
      params.set('state', stateFilter);
      if (selectedCategory !== 'all') params.set('category', selectedCategory);
      const res = await api.get(`/api/cards?${params}`);
      if (gen !== fetchGen) return;
      cards = res.cards ?? [];
      total = res.total ?? 0;
    } catch (err: any) {
      if (gen !== fetchGen) return;
      error = err?.message ?? 'Failed to load cards';
    } finally {
      if (gen === fetchGen) loading = false;
    }
  }

  function setFilter(f: StateFilter) {
    stateFilter = f;
    expandedId = null;
    fetchCards();
  }

  function dueLabel(card: Card): string {
    const due = new Date(card.due);
    const now = new Date();
    const ms = due.getTime() - now.getTime();
    if (ms <= 0) return 'due now';
    const hours = ms / 3_600_000;
    if (hours < 24) return `due ${due.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`;
    return `due in ${Math.round(hours / 24)}d`;
  }

  async function handleReset() {
    if (!resetTarget) return;
    busyId = resetTarget.id;
    try {
      await api.post('/api/mastery/reset', { questionId: resetTarget.id });
      resetTarget = null;
      await fetchCards();
    } catch (err: any) {
      error = err?.message ?? 'Failed to reset card';
    } finally {
      busyId = null;
    }
  }

  async function handleArchive(card: Card) {
    busyId = card.id;
    try {
      await api.post(`/api/questions/${card.id}/archive`, {
        reason: 'Archived from deck browser',
      });
      cards = cards.filter((c) => c.id !== card.id);
      total = Math.max(0, total - 1);
    } catch (err: any) {
      error = err?.message ?? 'Failed to archive';
    } finally {
      busyId = null;
    }
  }

  onMount(async () => {
    try {
      categories = (await api.get('/api/categories')) ?? [];
    } catch {
      // Non-critical
    }
    await fetchCards();
  });
</script>

<svelte:head>
  <title>Cards — Jeopardy! Training</title>
</svelte:head>

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-2xl font-bold text-jeopardy-blue">Cards</h1>
      {#if !loading}
        <span class="text-sm text-gray-500">{total} {total === 1 ? 'card' : 'cards'}</span>
      {/if}
      <button
        onclick={() => goto('/dashboard')}
        class="ml-auto px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
      >
        Done
      </button>
    </div>

    <!-- Filters -->
    <div class="bg-white rounded-xl shadow-sm px-4 py-3 flex flex-col sm:flex-row sm:items-center gap-3">
      <div class="flex gap-1.5 flex-wrap">
        {#each FILTERS as f (f.key)}
          <button
            onclick={() => setFilter(f.key)}
            class="px-3 py-1.5 rounded-full text-sm font-medium transition-colors {stateFilter === f.key
              ? 'bg-jeopardy-blue text-white'
              : 'bg-gray-100 text-gray-600 hover:bg-gray-200'}"
          >
            {f.label}
          </button>
        {/each}
      </div>
      <div class="sm:ml-auto sm:w-56">
        <CategoryFilter
          {categories}
          selected={selectedCategory}
          onchange={(v) => {
            selectedCategory = v;
            fetchCards();
          }}
        />
      </div>
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if cards.length === 0}
      <div class="text-center py-16 text-gray-500">No cards match this filter.</div>
    {:else}
      <div class="flex flex-col gap-2">
        {#each cards as card (card.id)}
          {@const expanded = expandedId === card.id}
          <div class="bg-white rounded-xl shadow-sm">
            <button
              class="w-full text-left px-4 py-3"
              onclick={() => (expandedId = expanded ? null : card.id)}
              aria-expanded={expanded}
            >
              <div class="flex items-center gap-2 flex-wrap mb-1">
                <span class="text-xs font-semibold uppercase tracking-wide text-gray-400">
                  {card.classifier_category}
                </span>
                <span class="text-xs px-1.5 py-0.5 rounded-full bg-gray-100 text-gray-600">{card.state}</span>
                <span class="text-xs text-gray-500">{dueLabel(card)}</span>
                {#if card.lapses > 0}
                  <span class="text-xs text-red-500">{card.lapses} lapses</span>
                {/if}
                {#if card.suspended}
                  <span class="text-xs px-1.5 py-0.5 rounded-full bg-red-100 text-red-700 font-semibold">suspended</span>
                {/if}
              </div>
              <p class="text-sm text-gray-800 {expanded ? '' : 'line-clamp-2'}">{card.answer}</p>
            </button>
            {#if expanded}
              <div class="px-4 pb-4 flex flex-col gap-2 border-t border-gray-100 pt-3">
                <p class="text-sm"><span class="font-semibold text-gray-500">Response:</span> <span class="text-gray-900 font-medium">{card.question}</span></p>
                <p class="text-xs text-gray-500">
                  {card.category}
                  {#if card.air_date}&nbsp;· aired {card.air_date}{/if}
                  &nbsp;· interval {Math.round(card.interval_days)}d
                </p>
                <div class="flex gap-2 mt-1">
                  <button
                    onclick={() => (resetTarget = card)}
                    disabled={busyId === card.id}
                    class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50 transition-colors"
                  >
                    Reset progress
                  </button>
                  <button
                    onclick={() => handleArchive(card)}
                    disabled={busyId === card.id}
                    class="px-3 py-1.5 rounded-lg border border-red-200 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50 transition-colors"
                  >
                    Archive
                  </button>
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
      {#if total > cards.length}
        <p class="text-center text-xs text-gray-400">Showing first {cards.length} of {total}.</p>
      {/if}
    {/if}
  </div>
</div>

{#if resetTarget}
  <Modal onclose={() => (resetTarget = null)} ariaLabelledby="reset-card-title">
    <div class="rounded-2xl bg-white shadow-2xl p-6 flex flex-col gap-4">
      <h2 id="reset-card-title" class="text-lg font-bold text-gray-800">Reset progress?</h2>
      <p class="text-sm text-gray-600">
        This card returns to the learning queue (interval, lapses, and suspension cleared) and comes due immediately.
      </p>
      <div class="flex gap-3">
        <button
          onclick={() => (resetTarget = null)}
          class="flex-1 py-2.5 rounded-xl border border-gray-300 text-gray-700 text-sm font-medium hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={handleReset}
          class="flex-1 py-2.5 rounded-xl bg-red-500 text-white text-sm font-semibold hover:bg-red-600 transition-colors"
        >
          Reset
        </button>
      </div>
    </div>
  </Modal>
{/if}
```

- [ ] **Step 2: Replace the old pages with redirect stubs**

Delete the CONTENTS of `frontend/src/routes/review/` and `frontend/src/routes/mastered/` and create in EACH directory these two files (SPA mode: the `+page.svelte` makes the route exist; the universal load redirects client-side):

`+page.ts`:

```ts
import { redirect } from '@sveltejs/kit';

export function load() {
  redirect(301, '/cards');
}
```

`+page.svelte`:

```svelte
<p class="p-8 text-center text-gray-500">Redirecting…</p>
```

- [ ] **Step 3: Delete MasteryBadge and update the Nav**

- Delete `frontend/src/lib/components/MasteryBadge.svelte`.
- In `frontend/src/lib/components/Nav.svelte`, set the links array to exactly:

```ts
  const links = [
    { href: '/dashboard', label: 'Dashboard' },
    { href: '/practice', label: 'Practice' },
    { href: '/drill', label: 'Drill' },
    { href: '/coryat', label: 'Coryat' },
    { href: '/settings', label: 'Settings' },
  ];
```

- [ ] **Step 4: Verify**

Run: `cd frontend && npm run check 2>&1 | tail -2 && npm run build 2>&1 | tail -2`
Expected: 0 errors; build succeeds.
Then: `grep -rn "MasteryBadge" src/` → no matches; `grep -rn 'href="/review\|href="/mastered' src/` → matches ONLY in `dashboard/+page.svelte` (fixed in Task 3).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/cards frontend/src/routes/review frontend/src/routes/mastered frontend/src/lib/components/Nav.svelte frontend/build
git rm frontend/src/lib/components/MasteryBadge.svelte 2>/dev/null; git add -u frontend/src
git commit -m "feat(nav): /cards deck browser; review/mastered redirect; 5-slot nav"
```

---

### Task 3: Dashboard hub — deck strip, Study card, action buttons

**Files:**
- Modify: `frontend/src/routes/dashboard/+page.svelte`

**Interfaces:**
- Consumes: `GET /api/practice/status` `deck: {learning, mastered, struggling}` (Task 1), `GET /api/study/latest` (existing; returns the latest recommendation record or 404), `/cards?state=...` (Task 2).

- [ ] **Step 1: Extend the srs type and add study state**

In the `<script>` of `frontend/src/routes/dashboard/+page.svelte`:

Add to the `srs` state type (after `adaptiveWeights?`):

```ts
    deck?: { learning: number; mastered: number; struggling: number };
```

Add study state and fetch (in `onMount`, alongside the status fetch):

```ts
  let lastStudy = $state<string | null>(null);
```

```ts
    api
      .get('/api/study/latest')
      .then((s) => (lastStudy = s?.generated_at ?? s?.generatedAt ?? null))
      .catch(() => (lastStudy = null));
```

- [ ] **Step 2: Deck strip inside the SRS card**

After the Focus areas block (`{#if srs.adaptiveWeights ...}...{/if}`) and before the SRS card's closing `</div>`, add:

```svelte
        {#if srs.deck}
          <div class="mt-5 pt-4 border-t border-gray-100 flex flex-wrap gap-x-5 gap-y-1 text-sm">
            <a href="/cards?state=learning" class="text-jeopardy-blue hover:underline">
              <span class="font-bold">{srs.deck.learning}</span> learning
            </a>
            <a href="/cards?state=mastered" class="text-jeopardy-blue hover:underline">
              <span class="font-bold">{srs.deck.mastered}</span> mastered
            </a>
            <a href="/cards?state=struggling" class="{srs.deck.struggling > 0 ? 'text-red-600' : 'text-jeopardy-blue'} hover:underline">
              <span class="font-bold">{srs.deck.struggling}</span> struggling
            </a>
          </div>
        {/if}
```

- [ ] **Step 3: Study card + action buttons**

Immediately AFTER the SRS summary card's `{#if srs}...{/if}` block, add:

```svelte
    <!-- Study sheets -->
    <a
      href="/study"
      class="bg-white rounded-xl shadow-sm p-5 mb-8 flex items-center justify-between hover:bg-gray-50 transition-colors group block"
    >
      <div>
        <p class="font-semibold text-gray-800">Study sheets</p>
        <p class="text-sm text-gray-500 mt-0.5">
          Generate targeted reading from your recent misses{#if lastStudy}
            — last generated {new Date(lastStudy).toLocaleDateString()}{/if}
        </p>
      </div>
      <span class="text-gray-400 group-hover:text-gray-600 text-lg">&rarr;</span>
    </a>
```

In the `<!-- Action Buttons -->` row: remove the `/review` and `/mastered` anchors and add a Drill button after Practice, so the row is exactly Practice, Drill, Coryat (same classes as the existing buttons):

```svelte
      <a
        href="/drill"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Drill
      </a>
```

- [ ] **Step 4: Verify**

Run: `cd frontend && npm run check 2>&1 | tail -2 && npm run build 2>&1 | tail -2`
Expected: 0 errors; build succeeds.
Then: `grep -rn 'href="/review\|href="/mastered' src/` → no matches anywhere.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/dashboard/+page.svelte frontend/build
git commit -m "feat(nav): dashboard deck strip, study card, drill action button"
```

---

## Notes for the implementer

- **No migration and no DB access** — deploy is just push → GHCR → container swap.
- `GET /api/study/latest` may 404 for users who never generated a sheet; the dashboard fetch treats any failure as "no last date" (non-fatal `.catch`).
- The redirect stubs use SvelteKit 2's `redirect()` (no `throw` needed; it throws internally). In this SPA (`ssr = false`) the redirect runs in the client-side router — the stub `+page.svelte` only flashes if JS is disabled.
- `page` from `$app/state` (not the deprecated `$app/stores`) matches existing usage in `Nav.svelte`.
