# Blind-Spot Learning System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the dead Study tool with in-loop learning: serve-time-pregenerated per-clue insights (auto on Wrong with a teaching pause; Explain elsewhere) and LLM-clustered blind-spot packs (primer + one-tap drill).

**Architecture:** A shared `openai.rs` chat helper feeds two modules — `insights.rs` (pure prompt/parse functions + a single-flight `ensure_insight` used by both a GET endpoint and fire-and-forget pregeneration in the `next` handlers) and `blindspots.rs` (pure parse/staleness functions + pack generation validated against the drill's full-text index). Frontend: Practice/Drill gain a Wrong-pause with the insight panel; a nav-less `/blindspots` page and a dashboard card replace Study; `/drill` auto-starts from `?q=`.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8 runtime-checked, reqwest, tokio), OpenAI Chat Completions (`gpt-4o-mini` insights / `gpt-4o` packs), PostgreSQL, SvelteKit SPA (Svelte 5 runes), Tailwind.

## Global Constraints

- **No DB access from this environment** — migration `0004` is file-only here, applied manually on Tower before deploy. sqlx is runtime-checked (bind order vs `$N` is a runtime risk — verify by eye).
- Insight contract: STRICT JSON `{"insight": "...", "hook": "..."}`; model `gpt-4o-mini`; temperature 0.4; cached per clue globally in `clue_insights` (UNIQUE question_id, `ON CONFLICT DO NOTHING` backstop); single-flight guard per process.
- Pack contract: STRICT JSON `{"packs": [{"theme","diagnosis","primer","search_query"}]}`; model `gpt-4o`; temperature 0.7; input = last 30 days of misses (≥ 10 to generate, ≤ 12 per category in the prompt); keep packs whose `search_query` matches **≥ 10** non-archived clues via `search_tsv @@ websearch_to_tsquery('english', $q)`; new set supersedes old in one transaction.
- Staleness rule (verbatim): never generated → refresh when `total_recent_misses >= 10`; else refresh when `age > 7 days AND new_misses_since >= 25`.
- Disabled mode: empty `OPENAI_API_KEY` → pregeneration skips, `GET /api/insight/{id}` returns 404, `GET /api/blindspots` reports `configured: false`; UI hides/annotates accordingly.
- Wrong-pause: grade is recorded immediately via the EXISTING `POST /api/practice/grade` (unchanged); only the advance is deferred. Correct grades keep instant advance. Keyboard Space/Enter/1/2/3 advances past the pause.
- Removed: `routes/study.rs`, `models/study.rs`, the three `/api/study/*` routes, the `/study` page (replaced by a redirect stub to `/blindspots`, same pattern as `/review` → `/cards`). `study_recommendations` table stays (inert).
- Gates: `cargo test` green, clippy clean except the 2 baseline warnings (`field email is never read`, `struct QuestionAttempt is never constructed`); `npm run check` 0 errors, `npm run build` succeeds (repo commits `frontend/build`). Live LLM verification is post-deploy only (no key/DB here) — note deferrals in reports.

---

### Task 1: Migration 0004 + prisma parity

**Files:**
- Create: `backend/migrations/0004_blindspot_learning.sql`
- Modify: `prisma/schema.prisma`

**Interfaces:**
- Produces: tables `clue_insights` and `blindspot_packs` exactly as below.

- [ ] **Step 1: Write the migration**

Create `backend/migrations/0004_blindspot_learning.sql`:

```sql
-- Blind-spot learning: per-clue insight cache (global, permanent) and
-- per-user blind-spot packs (primer + drill search query).
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
    miss_count   INTEGER NOT NULL DEFAULT 0,
    superseded   BOOLEAN NOT NULL DEFAULT false,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_blindspot_packs_user_active
  ON blindspot_packs (user_id, superseded, created_at DESC);
```

- [ ] **Step 2: Prisma parity (documentation only — Prisma is not the runtime)**

In `prisma/schema.prisma` add two models (and back-relations):

```prisma
model clue_insights {
  id          Int                @id @default(autoincrement())
  question_id Int                @unique
  content     Json               @db.JsonB
  model       String
  created_at  DateTime           @default(now()) @db.Timestamptz(6)
  question    jeopardy_questions @relation(fields: [question_id], references: [id])
}

model blindspot_packs {
  id           Int      @id @default(autoincrement())
  user_id      Int
  theme        String
  diagnosis    String
  primer       String
  search_query String
  match_count  Int
  miss_count   Int      @default(0)
  superseded   Boolean  @default(false)
  created_at   DateTime @default(now()) @db.Timestamptz(6)
  user         users    @relation(fields: [user_id], references: [id], onDelete: Cascade)

  @@index([user_id, superseded, created_at(sort: Desc)])
}
```

Add `clue_insights       clue_insights[]` inside `model jeopardy_questions` and `blindspot_packs       blindspot_packs[]` inside `model users`.

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/0004_blindspot_learning.sql prisma/schema.prisma
git commit -m "feat(blindspot): clue_insights + blindspot_packs migration"
```

---

### Task 2: `openai.rs` helper + `insights.rs` module (TDD) + AppState fields

**Files:**
- Create: `backend/src/openai.rs`
- Create: `backend/src/insights.rs`
- Modify: `backend/src/main.rs` (mods + AppState fields + construction)
- Test: inline `#[cfg(test)]` in `backend/src/insights.rs`

**Interfaces:**
- Produces:
  - `openai::chat_json(api_key: &str, model: &str, system: &str, user: &str, temperature: f64) -> Result<serde_json::Value, AppError>` — calls Chat Completions with `response_format: json_object`, returns the assistant content parsed as JSON.
  - `insights::InsightContent { pub insight: String, pub hook: String }`
  - `insights::parse_insight(v: &serde_json::Value) -> Result<InsightContent, String>` (pure)
  - `insights::insight_user_prompt(clue: &str, response: &str, category: &str, air_date: Option<&str>) -> String` (pure)
  - `insights::ensure_insight(state: &Arc<AppState>, question_id: i32) -> Result<Option<InsightContent>, AppError>` — cached-or-generate with single-flight; `Ok(None)` when key unconfigured or clue not found.
  - `AppState` gains `pub insight_inflight: tokio::sync::Mutex<std::collections::HashSet<i32>>` and `pub blindspot_inflight: std::sync::atomic::AtomicBool`.

- [ ] **Step 1: Write the failing tests**

Create `backend/src/insights.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_valid_insight() {
        let v = json!({"insight": "Because X leads to Y.", "hook": "X → Y, always."});
        let c = parse_insight(&v).unwrap();
        assert_eq!(c.insight, "Because X leads to Y.");
        assert_eq!(c.hook, "X → Y, always.");
    }

    #[test]
    fn parse_rejects_missing_or_empty_fields() {
        assert!(parse_insight(&json!({"insight": "x"})).is_err());
        assert!(parse_insight(&json!({"insight": "", "hook": "h"})).is_err());
        assert!(parse_insight(&json!({"insight": "x", "hook": "   "})).is_err());
        assert!(parse_insight(&json!("just a string")).is_err());
    }

    #[test]
    fn user_prompt_contains_all_clue_context() {
        let p = insight_user_prompt("This Corsican...", "Napoleon", "EMPERORS", Some("1998-05-02"));
        assert!(p.contains("This Corsican..."));
        assert!(p.contains("Napoleon"));
        assert!(p.contains("EMPERORS"));
        assert!(p.contains("1998-05-02"));
        let p2 = insight_user_prompt("c", "r", "CAT", None);
        assert!(!p2.contains("aired"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Add `mod insights;` and `mod openai;` to `backend/src/main.rs` (after `mod adaptive;`). Run: `cd backend && cargo test insights::`
Expected: FAIL — `cannot find function parse_insight` (and `openai` module missing → create it in Step 3 first if the compiler stops there).

- [ ] **Step 3: Implement `openai.rs`**

Create `backend/src/openai.rs` (extracted from the pattern in the old `routes/study.rs`):

```rust
//! Minimal OpenAI Chat Completions helper: JSON-mode call, returns the
//! assistant message content parsed as JSON.
use serde_json::Value;

use crate::error::AppError;

pub async fn chat_json(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f64,
) -> Result<Value, AppError> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "temperature": temperature,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("OpenAI request failed: {e}")))?;

    let body: Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse OpenAI response: {e}")))?;

    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| AppError::Internal(format!("No content in OpenAI response: {body}")))?;

    serde_json::from_str(content)
        .map_err(|e| AppError::Internal(format!("LLM returned non-JSON content: {e}")))
}
```

- [ ] **Step 4: Implement `insights.rs` above the test module**

```rust
//! Per-clue insights: why the answer is what it is, plus a memory hook.
//! Generated once per clue (global cache), pregenerated at serve time.
use std::sync::Arc;
use serde_json::Value;

use crate::error::AppError;
use crate::AppState;

pub const INSIGHT_MODEL: &str = "gpt-4o-mini";

pub const INSIGHT_SYSTEM_PROMPT: &str = r#"You are a Jeopardy!-aware tutor. Given one clue and its correct response, teach the player in miniature.

Rules:
- Output ONLY valid JSON: {"insight": "...", "hook": "..."}
- "insight": 60-90 words. Explain why the response is correct, the key fact behind it, and the pattern Jeopardy! uses for this kind of clue (wordplay, eponym, signature fact).
- "hook": ONE short, memorable line that cements the association. Vivid beats formal.
- Never restate the clue. Never say "the answer is". Teach the connection."#;

#[derive(Debug, Clone)]
pub struct InsightContent {
    pub insight: String,
    pub hook: String,
}

/// Validate the LLM's JSON into non-empty insight + hook.
pub fn parse_insight(v: &Value) -> Result<InsightContent, String> {
    let get = |key: &str| -> Result<String, String> {
        let s = v
            .get(key)
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("missing field: {key}"))?
            .trim()
            .to_string();
        if s.is_empty() {
            return Err(format!("empty field: {key}"));
        }
        Ok(s)
    };
    Ok(InsightContent { insight: get("insight")?, hook: get("hook")? })
}

/// Pure prompt builder (unit-tested; the LLM call itself is not).
pub fn insight_user_prompt(clue: &str, response: &str, category: &str, air_date: Option<&str>) -> String {
    let aired = air_date.map(|d| format!(" (aired {d})")).unwrap_or_default();
    format!(
        "Category: {category}{aired}\nClue: \"{clue}\"\nCorrect response: \"{response}\"\n\nReturn the JSON now."
    )
}

#[derive(sqlx::FromRow)]
struct ClueForInsight {
    answer: Option<String>,   // clue text shown to the player
    question: Option<String>, // expected response
    category: Option<String>,
    air_date: Option<chrono::NaiveDate>,
}

/// Cached-or-generate. Ok(None) when the key is unconfigured or the clue is
/// missing/incomplete. Single-flight: concurrent callers for the same clue
/// wait briefly for the winner's cache write instead of double-calling the LLM.
pub async fn ensure_insight(
    state: &Arc<AppState>,
    question_id: i32,
) -> Result<Option<InsightContent>, AppError> {
    // 1) Cache hit?
    if let Some(c) = read_cached(state, question_id).await? {
        return Ok(Some(c));
    }
    if state.config.openai_api_key.is_empty() {
        return Ok(None);
    }

    // 2) Single-flight: if another task is generating this clue, poll the cache.
    {
        let mut inflight = state.insight_inflight.lock().await;
        if !inflight.insert(question_id) {
            drop(inflight);
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Some(c) = read_cached(state, question_id).await? {
                    return Ok(Some(c));
                }
            }
            return Ok(None); // generation elsewhere failed or is very slow; give up quietly
        }
    }

    // From here on we own the flight; always release the guard.
    let result = generate_and_store(state, question_id).await;
    state.insight_inflight.lock().await.remove(&question_id);
    result
}

async fn read_cached(state: &Arc<AppState>, question_id: i32) -> Result<Option<InsightContent>, AppError> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT content FROM clue_insights WHERE question_id = $1")
            .bind(question_id)
            .fetch_optional(&state.pool)
            .await?;
    Ok(row.and_then(|(v,)| parse_insight(&v).ok()))
}

async fn generate_and_store(
    state: &Arc<AppState>,
    question_id: i32,
) -> Result<Option<InsightContent>, AppError> {
    let clue: Option<ClueForInsight> = sqlx::query_as(
        "SELECT answer, question, category, air_date FROM jeopardy_questions
         WHERE id = $1 AND archived = false",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some(clue) = clue else { return Ok(None) };
    let (Some(clue_text), Some(response)) = (clue.answer, clue.question) else {
        return Ok(None);
    };
    let category = clue.category.unwrap_or_else(|| "UNKNOWN".to_string());
    let air_date = clue.air_date.map(|d| d.to_string());

    let user = insight_user_prompt(&clue_text, &response, &category, air_date.as_deref());
    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        INSIGHT_MODEL,
        INSIGHT_SYSTEM_PROMPT,
        &user,
        0.4,
    )
    .await?;
    let content = parse_insight(&v).map_err(AppError::Internal)?;

    sqlx::query(
        "INSERT INTO clue_insights (question_id, content, model) VALUES ($1, $2, $3)
         ON CONFLICT (question_id) DO NOTHING",
    )
    .bind(question_id)
    .bind(serde_json::json!({ "insight": content.insight, "hook": content.hook }))
    .bind(INSIGHT_MODEL)
    .execute(&state.pool)
    .await?;

    Ok(Some(content))
}
```

- [ ] **Step 5: AppState fields**

In `backend/src/main.rs`, change the struct and construction:

```rust
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: config::Config,
    pub insight_inflight: tokio::sync::Mutex<std::collections::HashSet<i32>>,
    pub blindspot_inflight: std::sync::atomic::AtomicBool,
}
```

```rust
    let state = Arc::new(AppState {
        pool,
        config,
        insight_inflight: tokio::sync::Mutex::new(std::collections::HashSet::new()),
        blindspot_inflight: std::sync::atomic::AtomicBool::new(false),
    });
```

- [ ] **Step 6: Run tests + lint**

Run: `cd backend && cargo test insights:: && cargo clippy --all-targets 2>&1 | tail -3`
Expected: 3 tests PASS; no new warnings beyond the 2 baseline (`ensure_insight`/`chat_json` are consumed in Tasks 3–4; if clippy flags them unused at this stage, that is expected cross-task state — note it and proceed, as with prior phases).

- [ ] **Step 7: Commit**

```bash
git add backend/src/openai.rs backend/src/insights.rs backend/src/main.rs
git commit -m "feat(blindspot): openai helper + insight module with single-flight cache"
```

---

### Task 3: Insight endpoint + serve-time pregeneration

**Files:**
- Create: `backend/src/routes/insight.rs`
- Modify: `backend/src/routes/mod.rs` (add `pub mod insight;`)
- Modify: `backend/src/main.rs` (route)
- Modify: `backend/src/routes/practice.rs` (ClueRow.id visibility + pregen hooks in `next`)
- Modify: `backend/src/routes/drill.rs` (pregen hooks in `next`)

**Interfaces:**
- Consumes: `insights::ensure_insight` (Task 2).
- Produces: `GET /api/insight/{question_id}` → `{insight, hook}` | 404 (`AppError::NotFound`) when disabled/unavailable; pregeneration spawned in both `next` handlers.

- [ ] **Step 1: The endpoint**

Create `backend/src/routes/insight.rs`:

```rust
use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub async fn get_insight(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(question_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let _ = auth; // authenticated endpoint
    match crate::insights::ensure_insight(&state, question_id).await? {
        Some(c) => Ok(Json(json!({ "insight": c.insight, "hook": c.hook }))),
        None => Err(AppError::NotFound("No insight available".to_string())),
    }
}
```

Register: `pub mod insight;` in `backend/src/routes/mod.rs`; in `backend/src/main.rs` add
`.route("/api/insight/{id}", get(routes::insight::get_insight))` near the practice routes.

- [ ] **Step 2: Expose the clue id to drill.rs**

In `backend/src/routes/practice.rs`, change the ClueRow `id` field only:

```rust
pub(crate) struct ClueRow {
    pub(crate) id: i32,
```

(All other fields stay private.)

- [ ] **Step 3: Pregeneration hooks**

Add this small helper to `backend/src/routes/practice.rs` (near `clue_json`):

```rust
/// Fire-and-forget insight pregeneration: by the time the user reads,
/// reveals, and grades the card, the insight is cached. No-op when the
/// OpenAI key is unconfigured; failures are logged and swallowed.
pub(crate) fn pregenerate_insight(state: &Arc<AppState>, question_id: i32) {
    if state.config.openai_api_key.is_empty() {
        return;
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::insights::ensure_insight(&st, question_id).await {
            tracing::warn!("insight pregeneration failed for {question_id}: {e:?}");
        }
    });
}
```

Note: `AppError` has no `Debug` derive today — add `#[derive(Debug)]` to `pub enum AppError` in `backend/src/error.rs` (harmless, enables the `{e:?}` log).

In `practice::next`, immediately before EACH of the two card-returning `return Ok(Json(...)))` calls (the due-review branch and the new-clue branch), insert:

```rust
        pregenerate_insight(&state, row.id);
```

(place it before `clue_json(row)` consumes the row). In `drill::next` do the same before its two card returns (`crate::routes::practice::pregenerate_insight(&state, row.id);`).

- [ ] **Step 4: Verify**

Run: `cd backend && cargo test 2>&1 | grep "test result" && cargo clippy --all-targets 2>&1 | tail -3`
Expected: 28 tests pass (25 prior + 3 insights); clippy clean except the 2 baseline warnings.

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/insight.rs backend/src/routes/mod.rs backend/src/main.rs backend/src/routes/practice.rs backend/src/routes/drill.rs backend/src/error.rs
git commit -m "feat(blindspot): insight endpoint + serve-time pregeneration"
```

---

### Task 4: Blind-spot packs backend + study removal

**Files:**
- Create: `backend/src/blindspots.rs`
- Create: `backend/src/routes/blindspots.rs`
- Delete: `backend/src/routes/study.rs`, `backend/src/models/study.rs`
- Modify: `backend/src/main.rs` (mods + routes), `backend/src/routes/mod.rs`, `backend/src/models/mod.rs`, `backend/src/routes/drill.rs` (`match_predicate` → `pub(crate)`)
- Test: inline `#[cfg(test)]` in `backend/src/blindspots.rs`

**Interfaces:**
- Consumes: `openai::chat_json`, `drill::match_predicate` (made `pub(crate)`), `AppState.blindspot_inflight`.
- Produces:
  - `blindspots::PackDraft { pub theme: String, pub diagnosis: String, pub primer: String, pub search_query: String }`
  - `blindspots::parse_packs(v: &Value) -> Result<Vec<PackDraft>, String>` (pure)
  - `blindspots::needs_refresh(last_generated: Option<DateTime<Utc>>, new_misses_since: i64, total_recent_misses: i64, now: DateTime<Utc>) -> bool` (pure)
  - `blindspots::generate_packs_for_user(state, user_id) -> Result<GenOutcome, AppError>` where `pub enum GenOutcome { Generated(usize), InsufficientData }`
  - `GET /api/blindspots` → `{ packs: [{id, theme, diagnosis, primer, searchQuery, matchCount}], generatedAt, stale, insufficientData, configured }`
  - `POST /api/blindspots/generate` → same shape after a synchronous refresh.

- [ ] **Step 1: Failing tests**

Create `backend/src/blindspots.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn parse_valid_packs() {
        let v = json!({"packs": [
            {"theme": "Opera", "diagnosis": "8 misses", "primer": "P...", "search_query": "opera or aria"},
            {"theme": "VPs", "diagnosis": "5 misses", "primer": "Q...", "search_query": "\"vice president\""}
        ]});
        let packs = parse_packs(&v).unwrap();
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].theme, "Opera");
        assert_eq!(packs[1].search_query, "\"vice president\"");
    }

    #[test]
    fn parse_rejects_bad_shapes() {
        assert!(parse_packs(&json!({})).is_err());
        assert!(parse_packs(&json!({"packs": []})).is_err());
        assert!(parse_packs(&json!({"packs": [{"theme": "x"}]})).is_err());
        assert!(parse_packs(&json!({"packs": [{"theme": "", "diagnosis": "d", "primer": "p", "search_query": "q"}]})).is_err());
    }

    #[test]
    fn refresh_rules() {
        let now = Utc.with_ymd_and_hms(2026, 7, 2, 12, 0, 0).unwrap();
        // Never generated: needs >= 10 recent misses.
        assert!(!needs_refresh(None, 0, 9, now));
        assert!(needs_refresh(None, 0, 10, now));
        // Generated recently: never stale regardless of misses.
        let recent = now - Duration::days(3);
        assert!(!needs_refresh(Some(recent), 100, 100, now));
        // Old but quiet: not stale.
        let old = now - Duration::days(8);
        assert!(!needs_refresh(Some(old), 24, 100, now));
        // Old and active: stale.
        assert!(needs_refresh(Some(old), 25, 100, now));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Add `mod blindspots;` to `backend/src/main.rs`. Run: `cd backend && cargo test blindspots::`
Expected: FAIL — missing types/functions.

- [ ] **Step 3: Implement `blindspots.rs`**

```rust
//! Blind-spot packs: LLM-clustered themes from recent misses, each with a
//! primer and a drill-ready search query, validated against the search index.
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::error::AppError;
use crate::AppState;

pub const PACK_MODEL: &str = "gpt-4o";
pub const MIN_MISSES_TO_GENERATE: i64 = 10;
pub const MIN_PACK_MATCHES: i64 = 10;

pub const PACK_SYSTEM_PROMPT: &str = r#"You are a Jeopardy! training analyst. From the player's missed clues, identify their blind spots as FINE-GRAINED themes (e.g. "opera", "vice presidents", "European rivers") — never broad subject names like "History & Politics".

Rules:
- Output ONLY valid JSON: {"packs": [{"theme","diagnosis","primer","search_query"}]}
- 3 to 5 packs, most damaging blind spots first.
- "diagnosis": one sentence citing roughly how many misses show this gap and what specifically goes wrong.
- "primer": 80-120 words teaching the 4-6 facts or patterns that actually recur in Jeopardy! clues on this theme. Concrete names and connections, no fluff.
- "search_query": plain full-text search terms that would find such clues — bare words, "quoted phrases", or the word or between alternatives. No operators like AND, no punctuation beyond quotes."#;

#[derive(Debug, Clone)]
pub struct PackDraft {
    pub theme: String,
    pub diagnosis: String,
    pub primer: String,
    pub search_query: String,
}

pub enum GenOutcome {
    Generated(usize),
    InsufficientData,
}

/// Validate the LLM's JSON into 1..=8 packs with non-empty fields.
pub fn parse_packs(v: &Value) -> Result<Vec<PackDraft>, String> {
    let arr = v
        .get("packs")
        .and_then(|p| p.as_array())
        .ok_or("missing packs array")?;
    if arr.is_empty() || arr.len() > 8 {
        return Err(format!("bad pack count: {}", arr.len()));
    }
    arr.iter()
        .map(|p| {
            let get = |key: &str| -> Result<String, String> {
                let s = p
                    .get(key)
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| format!("missing field: {key}"))?
                    .trim()
                    .to_string();
                if s.is_empty() {
                    return Err(format!("empty field: {key}"));
                }
                Ok(s)
            };
            Ok(PackDraft {
                theme: get("theme")?,
                diagnosis: get("diagnosis")?,
                primer: get("primer")?,
                search_query: get("search_query")?,
            })
        })
        .collect()
}

/// Staleness rule (spec §2): never generated → refresh once >= 10 recent
/// misses exist; else refresh when older than 7 days AND >= 25 new misses.
pub fn needs_refresh(
    last_generated: Option<DateTime<Utc>>,
    new_misses_since: i64,
    total_recent_misses: i64,
    now: DateTime<Utc>,
) -> bool {
    match last_generated {
        None => total_recent_misses >= MIN_MISSES_TO_GENERATE,
        Some(t) => now - t > chrono::Duration::days(7) && new_misses_since >= 25,
    }
}

#[derive(sqlx::FromRow)]
struct MissRow {
    clue: Option<String>,
    response: Option<String>,
    show_category: Option<String>,
    classifier_category: Option<String>,
}

/// Generate, validate, and store a new pack set (superseding the old one).
pub async fn generate_packs_for_user(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<GenOutcome, AppError> {
    // Misses from the last 30 days, newest first.
    let misses: Vec<MissRow> = sqlx::query_as(
        "SELECT jq.answer AS clue, jq.question AS response, jq.category AS show_category,
                jq.classifier_category
         FROM question_attempts qa
         JOIN jeopardy_questions jq ON jq.id = qa.question_id
         WHERE qa.user_id = $1 AND qa.correct = false
           AND qa.answered_at >= now() - interval '30 days'
         ORDER BY qa.answered_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;

    if (misses.len() as i64) < MIN_MISSES_TO_GENERATE {
        return Ok(GenOutcome::InsufficientData);
    }
    let miss_count = misses.len() as i32;

    // <= 12 per classifier category in the prompt.
    let mut per_cat: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut lines = String::new();
    for m in &misses {
        let cat = m.classifier_category.clone().unwrap_or_else(|| "Uncategorized".into());
        let n = per_cat.entry(cat.clone()).or_insert(0);
        if *n >= 12 {
            continue;
        }
        *n += 1;
        lines.push_str(&format!(
            "[{}] Clue: \"{}\" Response: \"{}\" (show category: {})\n",
            cat,
            m.clue.as_deref().unwrap_or(""),
            m.response.as_deref().unwrap_or(""),
            m.show_category.as_deref().unwrap_or("?"),
        ));
    }

    let user_prompt = format!(
        "The player missed {miss_count} clues in the last 30 days. The misses:\n\n{lines}\nReturn the JSON now."
    );

    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        PACK_MODEL,
        PACK_SYSTEM_PROMPT,
        &user_prompt,
        0.7,
    )
    .await?;
    let drafts = parse_packs(&v).map_err(AppError::Internal)?;

    // Validate each search query against the index; keep packs with enough clues.
    let mut kept: Vec<(PackDraft, i64)> = Vec::new();
    for d in drafts {
        let pred = crate::routes::drill::match_predicate("", 1, None, &[]);
        let sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {pred}");
        let count: i64 = sqlx::query_scalar(&sql)
            .bind(&d.search_query)
            .fetch_one(&state.pool)
            .await?;
        if count >= MIN_PACK_MATCHES {
            kept.push((d, count));
        }
    }
    if kept.is_empty() {
        return Err(AppError::Internal(
            "No generated pack matched enough clues; try refreshing again".to_string(),
        ));
    }

    // Supersede + insert atomically.
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE blindspot_packs SET superseded = true WHERE user_id = $1 AND superseded = false")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let n = kept.len();
    for (d, count) in kept {
        sqlx::query(
            "INSERT INTO blindspot_packs
               (user_id, theme, diagnosis, primer, search_query, match_count, miss_count)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(user_id)
        .bind(&d.theme)
        .bind(&d.diagnosis)
        .bind(&d.primer)
        .bind(&d.search_query)
        .bind(count as i32)
        .bind(miss_count)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(GenOutcome::Generated(n))
}
```

- [ ] **Step 4: Make the drill predicate reusable**

In `backend/src/routes/drill.rs`, change `fn match_predicate(` to `pub(crate) fn match_predicate(` (nothing else).

- [ ] **Step 5: Routes**

Create `backend/src/routes/blindspots.rs`:

```rust
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::blindspots::{generate_packs_for_user, needs_refresh, GenOutcome};
use crate::error::AppError;
use crate::AppState;

#[derive(sqlx::FromRow)]
struct PackRow {
    id: i32,
    theme: String,
    diagnosis: String,
    primer: String,
    search_query: String,
    match_count: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

async fn load_state(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<(Vec<PackRow>, Option<chrono::DateTime<chrono::Utc>>, i64, i64), AppError> {
    let packs: Vec<PackRow> = sqlx::query_as(
        "SELECT id, theme, diagnosis, primer, search_query, match_count, created_at
         FROM blindspot_packs
         WHERE user_id = $1 AND superseded = false
         ORDER BY id ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let generated_at = packs.first().map(|p| p.created_at);

    let total_recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_attempts
         WHERE user_id = $1 AND correct = false AND answered_at >= now() - interval '30 days'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let new_since: i64 = match generated_at {
        Some(t) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM question_attempts
             WHERE user_id = $1 AND correct = false AND answered_at >= $2",
        )
        .bind(user_id)
        .bind(t)
        .fetch_one(&state.pool)
        .await?,
        None => 0,
    };
    Ok((packs, generated_at, new_since, total_recent))
}

fn response_json(
    packs: &[PackRow],
    generated_at: Option<chrono::DateTime<chrono::Utc>>,
    stale: bool,
    insufficient: bool,
    configured: bool,
) -> Value {
    json!({
        "packs": packs.iter().map(|p| json!({
            "id": p.id,
            "theme": p.theme,
            "diagnosis": p.diagnosis,
            "primer": p.primer,
            "searchQuery": p.search_query,
            "matchCount": p.match_count,
        })).collect::<Vec<_>>(),
        "generatedAt": generated_at,
        "stale": stale,
        "insufficientData": insufficient,
        "configured": configured,
    })
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let configured = !state.config.openai_api_key.is_empty();
    let (packs, generated_at, new_since, total_recent) = load_state(&state, user_id).await?;
    let stale = needs_refresh(generated_at, new_since, total_recent, chrono::Utc::now());
    let insufficient = generated_at.is_none() && total_recent < crate::blindspots::MIN_MISSES_TO_GENERATE;

    // Background auto-refresh: fire once, guarded; the CURRENT set is returned.
    if stale && configured && !state.blindspot_inflight.swap(true, Ordering::SeqCst) {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = generate_packs_for_user(&st, user_id).await {
                tracing::warn!("blindspot auto-refresh failed: {e:?}");
            }
            st.blindspot_inflight.store(false, Ordering::SeqCst);
        });
    }

    Ok(Json(response_json(&packs, generated_at, stale, insufficient, configured)))
}

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let configured = !state.config.openai_api_key.is_empty();
    if !configured {
        return Err(AppError::BadRequest(
            "Blind-spot analysis is disabled (no OPENAI_API_KEY configured).".to_string(),
        ));
    }
    let outcome = generate_packs_for_user(&state, user_id).await?;
    let (packs, generated_at, _new_since, _total) = load_state(&state, user_id).await?;
    let insufficient = matches!(outcome, GenOutcome::InsufficientData);
    Ok(Json(response_json(&packs, generated_at, false, insufficient, configured)))
}
```

Register: `pub mod blindspots;` in `routes/mod.rs`; in `main.rs` add
`.route("/api/blindspots", get(routes::blindspots::list))` and
`.route("/api/blindspots/generate", post(routes::blindspots::generate))`.

- [ ] **Step 6: Delete the study tool**

- Delete `backend/src/routes/study.rs` and `backend/src/models/study.rs`.
- Remove `pub mod study;` from `backend/src/routes/mod.rs` AND from `backend/src/models/mod.rs`.
- Remove the three `/api/study/*` routes from `backend/src/main.rs`.

- [ ] **Step 7: Verify**

Run: `cd backend && cargo test 2>&1 | grep "test result" && cargo clippy --all-targets 2>&1 | tail -3 && grep -rn "routes::study\|models::study\|mod study" src/ | wc -l`
Expected: 31 tests pass (28 + 3 blindspots); clippy clean except 2 baseline warnings; grep count 0.

- [ ] **Step 8: Commit**

```bash
git add backend/src/blindspots.rs backend/src/routes/blindspots.rs backend/src/routes/mod.rs backend/src/models/mod.rs backend/src/main.rs backend/src/routes/drill.rs
git rm backend/src/routes/study.rs backend/src/models/study.rs 2>/dev/null; git add -u backend/src
git commit -m "feat(blindspot): pack generation + endpoints; retire study tool"
```

---

### Task 5: Frontend — Wrong-pause + insight panel in Practice and Drill

**Files:**
- Modify: `frontend/src/lib/components/QuestionCard.svelte` (pause mode)
- Modify: `frontend/src/routes/practice/+page.svelte`
- Modify: `frontend/src/routes/drill/+page.svelte`

**Interfaces:**
- Consumes: `GET /api/insight/{question_id}` → `{insight, hook}` | 404.
- Produces: `QuestionCard` gains optional `paused?: boolean` and `pausePanel?: Snippet` — when `showAnswer && paused && pausePanel`, the pause panel renders INSTEAD of the grade-button row.

- [ ] **Step 1: QuestionCard pause mode**

In the `$props()` type block add `paused?: boolean;` and `pausePanel?: Snippet;` (and both names to the destructuring, `paused = false,`). Wrap the grade-buttons area: where the current code has the `{#if onGotIt}` 3-button / `{:else}` legacy 2-button block, change the outer structure to:

```svelte
      {#if paused && pausePanel}
        {@render pausePanel()}
      {:else if onGotIt}
        <!-- (existing 3-button grid unchanged) -->
      {:else}
        <!-- (existing legacy 2-button row unchanged) -->
      {/if}
```

- [ ] **Step 2: Practice page — pause flow**

In `frontend/src/routes/practice/+page.svelte`:

Add state:

```ts
  let pausedForInsight = $state(false);
  let insight = $state<{ insight: string; hook: string } | null>(null);
  let insightLoading = $state(false);
  let insightShown = $state(false); // Explain-on-correct inline display
```

Replace `handleGrade` body's advance behavior (the grade POST stays identical):

```ts
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
      if (rating === 'wrong') {
        // Teaching pause: stay on the card and show the insight.
        pausedForInsight = true;
        fetchInsight(question.id);
      } else {
        showAnswer = false;
        await fetchQuestion();
      }
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  async function fetchInsight(questionId: number) {
    insight = null;
    insightLoading = true;
    try {
      insight = await api.get(`/api/insight/${questionId}`);
    } catch {
      insight = null; // 404 (disabled) or failure: pause just shows Next
    } finally {
      insightLoading = false;
    }
  }

  async function advanceFromPause() {
    pausedForInsight = false;
    insight = null;
    insightShown = false;
    showAnswer = false;
    await fetchQuestion();
  }

  async function handleExplain() {
    if (!question || insightShown) return;
    insightShown = true;
    await fetchInsight(question.id);
  }
```

Also reset `pausedForInsight = false; insightShown = false; insight = null;` inside `fetchQuestion`'s success path (both branches) so filter changes can't strand a stale pause.

Update `handleKeydown`: when `pausedForInsight`, any of Space/Enter/Digit1/Digit2/Digit3 calls `advanceFromPause()` (with `e.preventDefault()` for Space) and returns before the existing logic.

- [ ] **Step 3: Practice page — markup**

On the `<QuestionCard ...>` add `paused={pausedForInsight}` and this snippet (alongside the existing snippets):

```svelte
          {#snippet pausePanel()}
            <div class="flex flex-col gap-3">
              {#if insightLoading}
                <div class="flex items-center gap-2 text-white/70 text-sm py-2">
                  <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-jeopardy-gold"></div>
                  Finding the lesson…
                </div>
              {:else if insight}
                <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3 text-left">
                  <p class="text-white/90 text-sm leading-relaxed">{insight.insight}</p>
                  <p class="text-jeopardy-gold text-sm font-semibold mt-2">💡 {insight.hook}</p>
                </div>
              {/if}
              <button
                onclick={advanceFromPause}
                class="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 border border-white/20 text-white font-semibold text-lg transition-colors"
              >
                Next →
              </button>
            </div>
          {/snippet}
```

Add the Explain affordance inside the existing `additionalActions` snippet (which already holds the Archive button), before it:

```svelte
              {#if !insightShown}
                <button
                  onclick={handleExplain}
                  class="w-full py-2 mb-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors"
                >
                  Explain this one
                </button>
              {:else if insightLoading}
                <p class="text-white/60 text-sm text-center py-2">Finding the lesson…</p>
              {:else if insight}
                <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3 text-left mb-2">
                  <p class="text-white/90 text-sm leading-relaxed">{insight.insight}</p>
                  <p class="text-jeopardy-gold text-sm font-semibold mt-2">💡 {insight.hook}</p>
                </div>
              {/if}
```

Update the keyboard hint line: when `pausedForInsight`, show `Press any grade key or Space for next clue` instead of the 1/2/3 hint.

- [ ] **Step 4: Drill page — same flow**

Apply the identical changes to `frontend/src/routes/drill/+page.svelte`: same four state vars, same `handleGrade` wrong-branch pause, same `fetchInsight`/`advanceFromPause`/`handleExplain` functions (drill's fetch is `fetchNext()` instead of `fetchQuestion()` inside `advanceFromPause`, and the same resets go in `fetchNext`'s success path), same keydown guard, `paused={pausedForInsight}` + the same `pausePanel` snippet on its QuestionCard, and an `additionalActions` snippet containing only the Explain affordance (drill has no Archive button today — add the snippet fresh).

- [ ] **Step 5: Verify + commit**

Run: `cd frontend && npm run check 2>&1 | tail -2 && npm run build 2>&1 | tail -2`
Expected: 0 errors; build succeeds. (Behavioral smoke is post-deploy — needs the live key.)

```bash
git add frontend/src/lib/components/QuestionCard.svelte frontend/src/routes/practice/+page.svelte frontend/src/routes/drill/+page.svelte frontend/build
git commit -m "feat(blindspot): wrong-pause with insight panel + Explain in practice/drill"
```

---

### Task 6: Frontend — /blindspots page, /study redirect, drill ?q=, dashboard card

**Files:**
- Create: `frontend/src/routes/blindspots/+page.svelte`
- Replace: `frontend/src/routes/study/` contents with redirect stubs
- Modify: `frontend/src/routes/drill/+page.svelte` (accept `?q=`)
- Modify: `frontend/src/routes/dashboard/+page.svelte` (Study card → Blind spots card)

**Interfaces:**
- Consumes: `GET /api/blindspots` → `{packs:[{id,theme,diagnosis,primer,searchQuery,matchCount}], generatedAt, stale, insufficientData, configured}`; `POST /api/blindspots/generate` (same shape).

- [ ] **Step 1: The /blindspots page**

Create `frontend/src/routes/blindspots/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  interface Pack {
    id: number;
    theme: string;
    diagnosis: string;
    primer: string;
    searchQuery: string;
    matchCount: number;
  }

  let packs = $state<Pack[]>([]);
  let generatedAt = $state<string | null>(null);
  let insufficientData = $state(false);
  let configured = $state(true);
  let loading = $state(true);
  let refreshing = $state(false);
  let error = $state('');

  function apply(res: any) {
    packs = res.packs ?? [];
    generatedAt = res.generatedAt ?? null;
    insufficientData = res.insufficientData ?? false;
    configured = res.configured ?? true;
  }

  onMount(async () => {
    try {
      apply(await api.get('/api/blindspots'));
    } catch (err: any) {
      error = err?.message ?? 'Failed to load blind spots';
    } finally {
      loading = false;
    }
  });

  async function refresh() {
    refreshing = true;
    error = '';
    try {
      apply(await api.post('/api/blindspots/generate'));
    } catch (err: any) {
      error = err?.message ?? 'Failed to analyze blind spots';
    } finally {
      refreshing = false;
    }
  }
</script>

<svelte:head>
  <title>Blind Spots — Jeopardy! Training</title>
</svelte:head>

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-2xl font-bold text-jeopardy-blue">Blind Spots</h1>
      {#if generatedAt}
        <span class="text-sm text-gray-500">as of {new Date(generatedAt).toLocaleDateString()}</span>
      {/if}
      <div class="flex items-center gap-2 ml-auto">
        {#if configured}
          <button
            onclick={refresh}
            disabled={refreshing}
            class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50 transition-colors"
          >
            {refreshing ? 'Analyzing…' : 'Refresh'}
          </button>
        {/if}
        <button
          onclick={() => goto('/dashboard')}
          class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          Done
        </button>
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
    {:else if !configured}
      <div class="text-center py-16 text-gray-500">
        Blind-spot analysis is not configured (missing OpenAI key).
      </div>
    {:else if packs.length === 0}
      <div class="text-center py-16 text-gray-500">
        {#if insufficientData}
          Not enough misses to analyze yet — keep practicing and check back.
        {:else}
          No blind spots analyzed yet. Hit Refresh to run the first analysis.
        {/if}
      </div>
    {:else}
      <div class="flex flex-col gap-3">
        {#each packs as pack (pack.id)}
          <div class="bg-white rounded-xl shadow-sm p-5 flex flex-col gap-2">
            <div class="flex items-center gap-2 flex-wrap">
              <h2 class="text-lg font-bold text-gray-800">{pack.theme}</h2>
              <a
                href={`/drill?q=${encodeURIComponent(pack.searchQuery)}`}
                class="ml-auto px-4 py-1.5 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors"
              >
                Drill this ({pack.matchCount} clues)
              </a>
            </div>
            <p class="text-sm text-red-600">{pack.diagnosis}</p>
            <p class="text-sm text-gray-700 leading-relaxed">{pack.primer}</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
```

- [ ] **Step 2: /study redirect stubs**

Delete the contents of `frontend/src/routes/study/` and create (same pattern as `/review`):

`+page.ts`:

```ts
import { redirect } from '@sveltejs/kit';

export function load() {
  redirect(301, '/blindspots');
}
```

`+page.svelte`:

```svelte
<p class="p-8 text-center text-gray-500">Redirecting…</p>
```

- [ ] **Step 3: Drill accepts `?q=`**

In `frontend/src/routes/drill/+page.svelte`, add `import { page } from '$app/state';` and at the END of the existing `onMount` (after categories/prefs load):

```ts
    const q = page.url.searchParams.get('q');
    if (q && q.trim()) {
      queryInput = q;
      await startDrill();
    }
```

- [ ] **Step 4: Dashboard — Study card becomes Blind spots card**

In `frontend/src/routes/dashboard/+page.svelte`:

Replace the `lastStudy` state + `/api/study/latest` fetch with:

```ts
  let blindspots = $state<{
    packs: Array<{ id: number; theme: string; diagnosis: string }>;
    insufficientData: boolean;
    configured: boolean;
  } | null>(null);
```

```ts
    api
      .get('/api/blindspots')
      .then((b) => (blindspots = b))
      .catch(() => (blindspots = null));
```

Replace the entire Study-sheets `<a href="/study" ...>...</a>` card with:

```svelte
    <!-- Blind spots -->
    {#if blindspots && blindspots.configured}
      <a
        href="/blindspots"
        class="bg-white rounded-xl shadow-sm p-5 mb-8 flex items-center justify-between hover:bg-gray-50 transition-colors group block"
      >
        <div>
          <p class="font-semibold text-gray-800">Blind spots</p>
          {#if blindspots.packs.length > 0}
            <p class="text-sm text-gray-500 mt-0.5">
              {blindspots.packs.slice(0, 3).map((p) => p.theme).join(' · ')}
            </p>
          {:else if blindspots.insufficientData}
            <p class="text-sm text-gray-500 mt-0.5">Keep practicing — analysis unlocks after a few more misses.</p>
          {:else}
            <p class="text-sm text-gray-500 mt-0.5">Analyze your recent misses for patterns.</p>
          {/if}
        </div>
        <span class="text-gray-400 group-hover:text-gray-600 text-lg">&rarr;</span>
      </a>
    {/if}
```

- [ ] **Step 5: Verify + commit**

Run: `cd frontend && npm run check 2>&1 | tail -2 && npm run build 2>&1 | tail -2 && grep -rn 'href="/study\|/api/study' src/ | grep -v routes/study`
Expected: 0 errors; build succeeds; grep shows no remaining study references outside the redirect stub directory.

```bash
git add frontend/src/routes/blindspots frontend/src/routes/study frontend/src/routes/drill frontend/src/routes/dashboard frontend/build
git commit -m "feat(blindspot): /blindspots page, study redirect, drill ?q=, dashboard card"
```

---

## Notes for the implementer

- **Deploy order:** apply `backend/migrations/0004_blindspot_learning.sql` on Tower BEFORE the container swap (additive, instant). The `OPENAI_API_KEY` already lives in the container's `.env`.
- **Post-deploy live verification** (the part no test covers): grade a clue Wrong → pause shows an insight within ~2s (or instantly on second encounter); `/blindspots` Refresh generates packs whose Drill buttons drop into a running drill; `GET /api/insight/{id}` on a cached clue is instant.
- The `qa.answered_at` column is `timestamp` (no tz) compared against `now() - interval` — fine under the app's UTC-server assumption, consistent with existing queries.
- `parse_packs` accepts 1–8 packs even though the prompt asks for 3–5 — the validation filter may drop some, and rejecting a 2-pack response outright would waste a good generation.
