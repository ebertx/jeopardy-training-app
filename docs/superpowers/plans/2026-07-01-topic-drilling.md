# Topic Drilling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add keyword topic drilling: a full-text search over clues that serves matching clues (due-first, then new, cap-bypassing) into the existing 3-button grade flow, so drilled clues feed the SRS queue.

**Architecture:** A Postgres `tsvector` GIN index enables `websearch_to_tsquery` search over clue+response+category. A new `GET /api/drill/next` endpoint (in `routes/drill.rs`) builds an injection-safe match predicate via a pure, unit-tested helper and returns the next matching clue plus counts; grading reuses `POST /api/practice/grade` untouched. A new `/drill` SvelteKit page drives it with the existing `QuestionCard`.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8 runtime-checked `query_as`), PostgreSQL full-text search, SvelteKit (Svelte 5 runes), TailwindCSS.

## Global Constraints

- Rust edition 2021; sqlx 0.8 using runtime-checked `sqlx::query`/`query_as::<_, T>` (no compile-time `query!` macros; no live DB at build time).
- **Never run migrations or tests against the shared production DB.** The migration SQL is applied manually on Tower; DB-dependent endpoint behavior is verified by build/clippy + (deferred) manual curl against a scratch DB only.
- All user-controlled values (`q`, `category`) are passed as bound parameters (`$n`). Only whitelisted static fragments (game-type clauses, the `""`/`"jq."` table prefix) and computed integers may be interpolated into SQL — mirrors the existing `quiz::random`/`practice` pickers.
- Search matches `search_tsv @@ websearch_to_tsquery('english', $n)` over clue text (`answer`), response (`question`), and show `category`.
- Grading reuses `POST /api/practice/grade` **unchanged**; do not add a new grade endpoint.
- Drilling is **not gated** by the daily new-card cap, but drilled new cards still create normal `srs_cards` (so they count toward the day's new total via `created_at`).
- Frontend uses Svelte 5 runes (`$state`/`$derived`/`$props`/`$effect`) and Tailwind, matching existing pages.

---

### Task 1: Migration — full-text search index

**Files:**
- Create: `backend/migrations/0002_search_index.sql`
- Modify (doc parity only): `prisma/schema.prisma`

**Interfaces:**
- Produces: column `jeopardy_questions.search_tsv tsvector` (generated) + GIN index `idx_jq_search_tsv`.

- [ ] **Step 1: Write the migration SQL**

Create `backend/migrations/0002_search_index.sql`:

```sql
-- Full-text search index over clue text + response + show-category name,
-- powering keyword topic drilling (websearch_to_tsquery).
--
-- NOTE: a STORED generated column triggers a one-time full-table rewrite
-- (ACCESS EXCLUSIVE lock) on ~530k rows plus the GIN build — order of tens of
-- seconds. Apply during low use.
ALTER TABLE jeopardy_questions ADD COLUMN IF NOT EXISTS search_tsv tsvector
  GENERATED ALWAYS AS (
    to_tsvector('english',
      coalesce(answer, '') || ' ' || coalesce(question, '') || ' ' || coalesce(category, ''))
  ) STORED;

CREATE INDEX IF NOT EXISTS idx_jq_search_tsv ON jeopardy_questions USING GIN (search_tsv);
```

- [ ] **Step 2: Add a documentation note to `prisma/schema.prisma`**

Prisma cannot model a generated `tsvector` column; do NOT add a typed field (it would break `prisma validate`/`generate`). Instead add a comment line inside `model jeopardy_questions { ... }`, right after the `archived_at` field:

```prisma
  // search_tsv tsvector GENERATED ALWAYS AS to_tsvector(answer||question||category) STORED
  //   + GIN index idx_jq_search_tsv — managed by migration 0002_search_index.sql, not modeled here.
```

- [ ] **Step 3: Sanity-check the SQL by eye (no DB execution)**

There is no scratch/test Postgres here and the repo `DATABASE_URL` is shared production — do NOT run this migration or connect to any database. Confirm by eye: balanced parens, valid `GENERATED ALWAYS AS (...) STORED` syntax, `IF NOT EXISTS` on both statements.

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0002_search_index.sql prisma/schema.prisma
git commit -m "feat(drill): add full-text search index migration"
```

---

### Task 2: `GET /api/drill/next` endpoint

**Files:**
- Create: `backend/src/routes/drill.rs`
- Modify: `backend/src/routes/practice.rs` (expose `ClueRow` + `clue_json` as `pub(crate)`)
- Modify: `backend/src/routes/mod.rs` (add `pub mod drill;`)
- Modify: `backend/src/main.rs` (mount the route)
- Test: inline `#[cfg(test)]` in `backend/src/routes/drill.rs`

**Interfaces:**
- Consumes: `crate::routes::practice::{ClueRow, clue_json}`, `crate::auth::middleware::AuthUser`, `crate::error::AppError`, `crate::AppState`.
- Produces: `GET /api/drill/next?q=&category=&gameTypes=` → `{ done, isNew?, card?, matchCount, remaining }`; pure helper `fn match_predicate(prefix: &str, q_param: usize, cat_param: Option<usize>, game_types: &[&str]) -> String`.

- [ ] **Step 1: Expose the shared clue helpers from `practice.rs`**

In `backend/src/routes/practice.rs`, change ONLY the visibility of the `ClueRow` struct and the `clue_json` fn (added in the SRS work) from private to `pub(crate)`. The struct's fields stay private — `drill.rs` only names the type (for `query_as::<_, ClueRow>`) and passes rows to `clue_json`; it never reads fields, and the derived `FromRow` impl + `clue_json` both live in `practice.rs` where private fields are accessible.

Change the struct declaration line:

```rust
pub(crate) struct ClueRow {
```

and the function signature line:

```rust
pub(crate) fn clue_json(row: ClueRow) -> Value {
```

Leave the `#[derive(sqlx::FromRow)]` attribute, the field list, and the `clue_json` body exactly as they are.

- [ ] **Step 2: Write the failing test for `match_predicate`**

Create `backend/src/routes/drill.rs` with just the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::match_predicate;

    #[test]
    fn base_predicate_binds_query_and_has_no_category() {
        let p = match_predicate("", 1, None, &[]);
        assert!(p.contains("search_tsv @@ websearch_to_tsquery('english', $1)"));
        assert!(p.contains("archived = false"));
        assert!(!p.contains("classifier_category ="));
        // q is bound, never interpolated
        assert!(!p.contains("websearch_to_tsquery('english', '"));
    }

    #[test]
    fn prefixed_predicate_with_category_uses_given_bind_positions() {
        let p = match_predicate("jq.", 2, Some(3), &[]);
        assert!(p.contains("jq.search_tsv @@ websearch_to_tsquery('english', $2)"));
        assert!(p.contains("jq.classifier_category = $3"));
        assert!(p.contains("jq.archived = false"));
    }

    #[test]
    fn game_types_expand_to_whitelisted_clauses_only() {
        let p = match_predicate("", 1, None, &["kids", "Teen", "college", "bogus"]);
        assert!(p.contains("NOT (notes ILIKE '%Kids%'"));
        assert!(p.contains("NOT (notes ILIKE '%Teen%')"));
        assert!(p.contains("NOT (notes ILIKE '%College%')"));
        // unknown game types contribute nothing
        assert!(!p.to_lowercase().contains("bogus"));
    }
}
```

- [ ] **Step 3: Run the test to verify it fails to compile**

Run: `cd backend && cargo test drill::`
Expected: FAIL — `cannot find function match_predicate`.

- [ ] **Step 4: Implement the helper and the `next` handler**

Prepend to `backend/src/routes/drill.rs` (above the test module):

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
use crate::routes::practice::{clue_json, ClueRow};
use crate::AppState;

const CLUE_COLS: &str =
    "id, question, answer, category, classifier_category, clue_value, round, air_date, notes";

/// Build the "clue matches the search + filters" SQL predicate.
///
/// `prefix` is the table alias prefix (`""` for a bare `jeopardy_questions`
/// query, `"jq."` when joined). `q_param` is the 1-based bind position of the
/// search string; `cat_param` the bind position of the classifier category when
/// filtering. `q` and category are ALWAYS bound (never interpolated); game-type
/// clauses are a fixed whitelist and the prefix is caller-controlled — so the
/// returned fragment carries no user-controlled string.
fn match_predicate(
    prefix: &str,
    q_param: usize,
    cat_param: Option<usize>,
    game_types: &[&str],
) -> String {
    let p = prefix;
    let mut c = vec![
        format!("{p}question IS NOT NULL"),
        format!("{p}answer IS NOT NULL"),
        format!("{p}classifier_category IS NOT NULL"),
        format!("{p}air_date IS NOT NULL"),
        format!("{p}archived = false"),
        format!("{p}search_tsv @@ websearch_to_tsquery('english', ${q_param})"),
    ];
    if let Some(ci) = cat_param {
        c.push(format!("{p}classifier_category = ${ci}"));
    }
    for gt in game_types {
        match *gt {
            "kids" | "Kids" => {
                c.push(format!("NOT ({p}notes ILIKE '%Kids%' OR {p}notes ILIKE '%Kid''s%')"))
            }
            "teen" | "Teen" => c.push(format!("NOT ({p}notes ILIKE '%Teen%')")),
            "college" | "College" => c.push(format!("NOT ({p}notes ILIKE '%College%')")),
            _ => {}
        }
    }
    c.join(" AND ")
}

pub async fn next(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let q = params.get("q").map(|s| s.trim()).unwrap_or("");
    if q.is_empty() {
        return Err(AppError::BadRequest("q (search query) is required".into()));
    }
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";
    let game_types_str = params.get("gameTypes").map(|s| s.as_str()).unwrap_or("");
    let game_types: Vec<&str> = game_types_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // Total matches (for the "N clues match" header). Binds: $1 = q, $2 = category?.
    let count_pred = match_predicate("", 1, if use_category { Some(2) } else { None }, &game_types);
    let count_sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {count_pred}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql).bind(q);
    if use_category {
        cq = cq.bind(category);
    }
    let match_count: i64 = cq.fetch_one(&state.pool).await?;

    // Tier-1: due matches (already-scheduled clues on this topic that are due).
    // Binds: $1 = user_id, $2 = q, $3 = category?.
    let due_pred = match_predicate("jq.", 2, if use_category { Some(3) } else { None }, &game_types);
    let due_join = format!(
        "FROM srs_cards sc JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND sc.suspended = false AND sc.due <= now() AND {due_pred}"
    );
    let due_count_sql = format!("SELECT COUNT(*) {due_join}");
    let mut dcq = sqlx::query_scalar::<_, i64>(&due_count_sql).bind(user_id).bind(q);
    if use_category {
        dcq = dcq.bind(category);
    }
    let due_count: i64 = dcq.fetch_one(&state.pool).await?;

    // Tier-2: new matches (clue not yet in this user's SRS pool).
    // Binds: $1 = user_id, $2 = q, $3 = category?.
    let new_pred = match_predicate("jq.", 2, if use_category { Some(3) } else { None }, &game_types);
    let new_where = format!(
        "FROM jeopardy_questions jq \
         WHERE {new_pred} AND jq.id NOT IN (SELECT question_id FROM srs_cards WHERE user_id = $1)"
    );
    let new_count_sql = format!("SELECT COUNT(*) {new_where}");
    let mut ncq = sqlx::query_scalar::<_, i64>(&new_count_sql).bind(user_id).bind(q);
    if use_category {
        ncq = ncq.bind(category);
    }
    let new_count: i64 = ncq.fetch_one(&state.pool).await?;

    let remaining = due_count + new_count;

    // Serve tier-1 (soonest due) first.
    if due_count > 0 {
        let sql = format!(
            "SELECT {} {due_join} ORDER BY sc.due ASC LIMIT 1",
            prefixed_cols("jq.")
        );
        let mut q1 = sqlx::query_as::<_, ClueRow>(&sql).bind(user_id).bind(q);
        if use_category {
            q1 = q1.bind(category);
        }
        if let Some(row) = q1.fetch_optional(&state.pool).await? {
            return Ok(Json(json!({
                "done": false, "isNew": false, "card": clue_json(row),
                "matchCount": match_count, "remaining": remaining,
            })));
        }
    }

    // Then a new match, recency-biased (same exponential offset as the Practice picker).
    if new_count > 0 {
        use rand::Rng;
        let r: f64 = rand::rng().random();
        let lambda = 3.5_f64;
        let normalized = (-(1.0 - r).ln() / lambda).min(1.0);
        let offset = (normalized * new_count as f64).floor() as i64;
        let sql = format!(
            "SELECT {} {new_where} ORDER BY jq.air_date DESC LIMIT 1 OFFSET {offset}",
            prefixed_cols("jq.")
        );
        let mut q2 = sqlx::query_as::<_, ClueRow>(&sql).bind(user_id).bind(q);
        if use_category {
            q2 = q2.bind(category);
        }
        if let Some(row) = q2.fetch_optional(&state.pool).await? {
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "matchCount": match_count, "remaining": remaining,
            })));
        }
    }

    Ok(Json(json!({ "done": true, "matchCount": match_count, "remaining": 0 })))
}

/// The clue column list, each prefixed with the table alias.
fn prefixed_cols(prefix: &str) -> String {
    CLUE_COLS
        .split(", ")
        .map(|col| format!("{prefix}{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}
```

- [ ] **Step 5: Register the module and route**

In `backend/src/routes/mod.rs` add: `pub mod drill;`

In `backend/src/main.rs`, inside the `api_routes` builder (near the practice routes), add:

```rust
        .route("/api/drill/next", get(routes::drill::next))
```

- [ ] **Step 6: Run the helper tests and verify they pass**

Run: `cd backend && cargo test drill::`
Expected: PASS — 3 tests (`base_predicate_binds_query_and_has_no_category`, `prefixed_predicate_with_category_uses_given_bind_positions`, `game_types_expand_to_whitelisted_clauses_only`).

- [ ] **Step 7: Build and lint**

Run: `cd backend && cargo build 2>&1 | tail -5 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: clean build. Acceptable pre-existing baseline warnings only (`field email is never read`, `struct QuestionAttempt is never constructed`). No new warnings.

- [ ] **Step 8: (Deferred) manual verification note**

The behavioral curl check needs a live server + scratch Postgres with the `search_tsv` index applied — unavailable here (production DB is off-limits). Record in the report that `GET /api/drill/next?q=napoleon` verification is deferred to a live environment; expected shape: `{ done:false, isNew:true|false, card:{...}, matchCount:<n>, remaining:<n> }`, and `?q=` (empty) → HTTP 400.

- [ ] **Step 9: Commit**

```bash
git add backend/src/routes/drill.rs backend/src/routes/practice.rs backend/src/routes/mod.rs backend/src/main.rs
git commit -m "feat(drill): GET /api/drill/next full-text drill picker"
```

---

### Task 3: `/drill` page + Nav link

**Files:**
- Create: `frontend/src/routes/drill/+page.svelte`
- Modify: `frontend/src/lib/components/Nav.svelte`

**Interfaces:**
- Consumes: `GET /api/drill/next` (Task 2), `POST /api/practice/grade`, `GET /api/categories`, `GET /api/preferences`; `QuestionCard`, `CategoryFilter`, `api`.

- [ ] **Step 1: Create the drill page**

Create `frontend/src/routes/drill/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import QuestionCard from '$lib/components/QuestionCard.svelte';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let queryInput = $state('');
  let activeQuery = $state('');
  let started = $state(false);

  let question = $state<any>(null);
  let isNew = $state(false);
  let matchCount = $state(0);
  let remaining = $state(0);
  let done = $state(false);
  let showAnswer = $state(false);
  let loading = $state(false);
  let error = $state('');
  let sessionId = $state<number | null>(null);
  let submitting = $state(false);
  let runningStats = $state({ total: 0, correct: 0 });

  let categories = $state<Array<{ name: string; count: number }>>([]);
  let selectedCategory = $state('all');
  let gameTypeFilters = $state<string[]>([]);
  let filtersOpen = $state(false);
  let filterGen = $state(0);

  let accuracy = $derived(
    runningStats.total > 0 ? Math.round((runningStats.correct / runningStats.total) * 100) : 0
  );

  function buildParams(): URLSearchParams {
    const params = new URLSearchParams();
    params.set('q', activeQuery);
    if (selectedCategory !== 'all') params.set('category', selectedCategory);
    if (gameTypeFilters.length > 0) params.set('gameTypes', gameTypeFilters.join(','));
    return params;
  }

  async function fetchNext() {
    const gen = filterGen;
    loading = true;
    error = '';
    try {
      const res = await api.get(`/api/drill/next?${buildParams()}`);
      if (gen !== filterGen) return;
      matchCount = res.matchCount ?? 0;
      remaining = res.remaining ?? 0;
      if (res.done) {
        done = true;
        question = null;
      } else {
        done = false;
        isNew = res.isNew;
        question = res.card;
      }
    } catch (err: any) {
      if (gen !== filterGen) return;
      error = err?.message ?? 'Failed to load clue';
    } finally {
      if (gen === filterGen) loading = false;
    }
  }

  async function startDrill() {
    const q = queryInput.trim();
    if (!q) return;
    activeQuery = q;
    started = true;
    filterGen++;
    done = false;
    showAnswer = false;
    runningStats = { total: 0, correct: 0 };
    await fetchNext();
  }

  async function handleGrade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (submitting || !question) return;
    submitting = true;
    try {
      const result = await api.post('/api/practice/grade', {
        questionId: question.id,
        rating,
        sessionId,
      });
      sessionId = result.sessionId;
      runningStats.total++;
      if (rating !== 'wrong') runningStats.correct++;
      showAnswer = false;
      await fetchNext();
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
    if (!started) return;
    if (e.code === 'Space' && !showAnswer) {
      e.preventDefault();
      showAnswer = true;
    } else if (showAnswer && !submitting) {
      if (e.code === 'Digit1') handleGrade('wrong');
      else if (e.code === 'Digit2') handleGrade('got_it');
      else if (e.code === 'Digit3') handleGrade('too_easy');
    }
  }

  onMount(async () => {
    try {
      const [cats, prefs] = await Promise.all([
        api.get('/api/categories'),
        api.get('/api/preferences'),
      ]);
      categories = cats ?? [];
      gameTypeFilters = prefs?.gameTypeFilters ?? [];
    } catch {
      // Non-critical
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="min-h-screen bg-gray-50 py-3 sm:py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-3 sm:gap-4">
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Drill</h1>
      {#if started && !done}
        <div class="text-sm font-medium text-gray-600">
          <span class="font-bold text-jeopardy-blue">{matchCount}</span> match ·
          <span class="font-bold text-jeopardy-blue">{remaining}</span> to hit now
        </div>
      {/if}
      {#if runningStats.total > 0}
        <div class="text-sm text-gray-500 ml-auto">
          {runningStats.correct}/{runningStats.total} ({accuracy}%)
        </div>
      {/if}
    </div>

    <!-- Search + filters -->
    <form onsubmit={(e) => { e.preventDefault(); startDrill(); }} class="flex flex-col gap-3 bg-white rounded-xl shadow-sm px-4 py-3">
      <div class="flex gap-2">
        <input
          type="search"
          bind:value={queryInput}
          placeholder="Search a topic — e.g. Impressionism, Marie Curie"
          class="flex-1 rounded-lg border border-gray-300 px-3 py-2 text-sm focus:border-jeopardy-blue focus:outline-none focus:ring-1 focus:ring-jeopardy-blue"
        />
        <button type="submit" class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors">
          Drill
        </button>
        <button type="button" onclick={() => (filtersOpen = !filtersOpen)} class="px-3 py-2 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100">
          Filters
        </button>
      </div>
      {#if filtersOpen}
        <div class="flex flex-col sm:flex-row sm:items-center gap-4 border-t border-gray-100 pt-3">
          <div class="flex-1">
            <p class="block text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Category</p>
            <CategoryFilter {categories} selected={selectedCategory} onchange={(v) => (selectedCategory = v)} />
          </div>
          <div>
            <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">Exclude Game Types</p>
            <div class="flex flex-wrap gap-3">
              {#each ['Kids', 'Teen', 'College'] as type}
                <label class="flex items-center gap-1.5 text-sm text-gray-700 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={gameTypeFilters.includes(type)}
                    onchange={() => {
                      gameTypeFilters = gameTypeFilters.includes(type)
                        ? gameTypeFilters.filter((t) => t !== type)
                        : [...gameTypeFilters, type];
                    }}
                    class="w-4 h-4 rounded border-gray-300 text-jeopardy-blue focus:ring-jeopardy-blue"
                  />
                  {type}
                </label>
              {/each}
            </div>
          </div>
        </div>
      {/if}
    </form>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    {#if loading && !question}
      <div class="flex justify-center py-20">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if question}
      <div class="min-h-[420px]">
        <QuestionCard
          clue={question.answer}
          answer={question.question}
          category={question.category}
          classifierCategory={question.classifier_category ?? question.category}
          clueValue={question.clue_value ?? null}
          round={question.round ?? null}
          airDate={question.air_date ?? null}
          {showAnswer}
          onRevealAnswer={() => { showAnswer = true; }}
          onWrong={() => handleGrade('wrong')}
          onGotIt={() => handleGrade('got_it')}
          onTooEasy={() => handleGrade('too_easy')}
          {submitting}
        />
      </div>
      <p class="hidden sm:block text-center text-xs text-gray-400">
        {#if !showAnswer}
          Press <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">Space</kbd> to reveal
        {:else}
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">1</kbd> Wrong ·
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">2</kbd> Got it ·
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">3</kbd> Too easy
        {/if}
      </p>
    {:else if started && done}
      <div class="text-center py-16 text-gray-600">
        🎯 You've drilled everything due or new for “{activeQuery}”.
      </div>
    {:else}
      <div class="text-center py-16 text-gray-500">
        Search a topic above to start drilling.
      </div>
    {/if}
  </div>
</div>
```

- [ ] **Step 2: Add the Nav link**

In `frontend/src/lib/components/Nav.svelte`, add a `Drill` entry to the `links` array, right after the Practice entry:

```ts
    { href: '/practice', label: 'Practice' },
    { href: '/drill', label: 'Drill' },
```

- [ ] **Step 3: Type-check and build**

Run: `cd frontend && npm run check 2>&1 | tail -15 && npm run build 2>&1 | tail -5`
Expected: 0 errors; build succeeds.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/routes/drill frontend/src/lib/components/Nav.svelte
git commit -m "feat(drill): /drill search page + nav link"
```

---

## Notes for the implementer

- **Applying the migration to production** is a manual step outside this plan: apply `backend/migrations/0002_search_index.sql` to the `jeopardy` DB on Tower (via psql) during low use — it rewrites the ~530k-row table and builds the GIN index. Do this BEFORE the new container ships (drill queries reference `search_tsv`), same discipline as migration 0001.
- The `/drill` grade path reuses `POST /api/practice/grade`; a drilled new clue therefore counts toward the day's new-card total (via `srs_cards.created_at`), which is intended — Practice's auto-trickle backs off accordingly.
- Reuses `ClueRow`/`clue_json` (now `pub(crate)`) so the drill and practice endpoints return an identical card shape to the frontend.
