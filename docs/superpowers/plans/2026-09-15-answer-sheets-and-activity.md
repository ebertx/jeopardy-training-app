# Answer Sheets, Fact Cards, and Active Days Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a corpus-grounded "answer sheet" (identity + four drillable facts) when a Pavlov card is rated Wrong or a practice clue is missed, let the four facts become extra Pavlov cards, and put an active-days strip on the dashboard.

**Architecture:** `sheets.rs` mirrors the existing `insights.rs` (cached-or-generate, single-flight, gpt-4o-mini) but is keyed by normalized answer and grounded in that answer's real clues. Fact cards are ordinary `pavlov_answers` rows flagged `kind='fact'` with a `parent_norm`, so the existing drill/grade/stats pipeline serves them unchanged; a handful of `kind='answer'` filters keep them out of new-card introduction, regeneration cleanup, and deck progress. Activity is one SQL series over attempts + Pavlov reviews, summarized by a pure function.

**Tech Stack:** Rust (Axum 0.8, sqlx 0.8 runtime queries, chrono/chrono-tz, reqwest → OpenAI JSON mode), PostgreSQL 15, SvelteKit + Svelte 5 runes/snippets, Tailwind 4.

**Spec:** `docs/superpowers/specs/2026-09-15-answer-sheets-and-activity-design.md`

## Global Constraints

- Backend `backend/`, frontend `frontend/`; root-level Next.js `app/`, `prisma/`, root `package.json` are dead leftovers — never touch them. Schema source of truth is `backend/migrations/*.sql`; migration file is `0015_answer_sheets.sql`; additive only; applied manually with `scripts/apply-migration.sh` before push (Task 7).
- Normalized answer = SQL `lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))` (the `idx_jq_answer_norm` expression); `pavlov_answers.answer_norm` and `answer_sheets.answer_norm` use this form.
- Sheet JSON: `{"identity": string, "facts": [{"prompt": string, "response": string}] }` with EXACTLY 4 facts. Model `gpt-4o-mini`, temperature 0.4. A rejected sheet is never cached.
- Fact rows: `kind='fact'`, `answer_norm = "<parent_norm>::<n>"` (n = 1..4), `parent_norm = <parent_norm>`, `answer = response`, `phrases = [prompt]`, `phrase_tiers = ['standard']`, category/score/answer_freq/example_clue_ids copied from the parent.
- Filters that MUST include `kind = 'answer'`: `pick_new_card` (all three queries), `drill_next`'s `more_new_available`, the generation stale-cleanup DELETE, deck progress `deckTotal` and `touched`. Everything else (due queue, buckets, forecast, review log, category rollups, list page) includes facts.
- Active day = any `question_attempts` row (any kind) OR any `pavlov_reviews` row OR a `pavlov_cards` row created or last-reviewed that local day. Streak counts back from today, or from yesterday when today is not yet active.
- Dashboard strip colour: green ≥ 24 of 28, amber ≥ 16, red below. 28 dots, today rightmost.
- Keys on the Pavlov reveal: `e` toggles the sheet, `d` adds the four fact cards, `Space`/`Enter` = Next while paused. Wrong on `kind='answer'` pauses with the sheet; Wrong on a fact card advances normally.
- Repo test style: pure logic in `#[cfg(test)] mod tests`; SQL verified by read-only scripts under `scripts/verify-*.sql`; frontend by `npm run check` + `npm run build` + mock-API screenshots. Baselines: `cargo test` 88 passing; `npm run check` 0 errors, one pre-existing CountdownTimer warning.
- `frontend/build/` is a stale tracked dir that `npm run build` dirties: run `git checkout -- frontend/build` before committing.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File map

| File | Responsibility |
|---|---|
| `backend/migrations/0015_answer_sheets.sql` | Create: sheets table, `kind`/`parent_norm`, `pavlov_auto_facts` |
| `backend/src/sheets.rs` | Create: prompt, parse/validate, normalization, fact-norm, even sampling, `ensure_sheet` |
| `backend/src/activity.rs` | Create: pure `summarize` |
| `backend/src/routes/sheet.rs` | Create: `GET /api/sheet/answer/{norm}`, `GET /api/sheet/question/{id}` |
| `backend/src/routes/pavlov_facts.rs` | Create: `add_fact_cards` helper + `POST /api/pavlov/facts` |
| `backend/src/routes/activity.rs` | Create: `GET /api/activity` |
| `backend/src/routes/pavlov.rs` | Modify: `DrillAnswerRow` gains norm/kind/parent; prefetch sheet; `kind='answer'` filters; auto-add on Wrong; list emits kind/parent |
| `backend/src/routes/pavlov_stats.rs` | Modify: `deckTotal`/`touched` exclude facts |
| `backend/src/pavlov.rs` | Modify: stale cleanup keeps facts |
| `backend/src/routes/preferences.rs` | Modify: `pavlovAutoFacts` |
| `backend/src/main.rs`, `backend/src/routes/mod.rs` | Modify: modules, state, routes |
| `scripts/verify-sheets.sql` | Create: read-only checks |
| `frontend/src/lib/components/AnswerSheet.svelte` | Create: shared sheet renderer |
| `frontend/src/lib/components/ActivityStrip.svelte` | Create: streak + 28 dots |
| `frontend/src/routes/pavlov/+page.svelte` | Modify: pause on Wrong, `e`/`d`, parent label |
| `frontend/src/routes/practice/+page.svelte` | Modify: sheet in teaching pause |
| `frontend/src/routes/settings/+page.svelte` | Modify: auto-facts toggle |
| `frontend/src/routes/pavlov/list/+page.svelte` | Modify: parent label |
| `frontend/src/routes/dashboard/+page.svelte` | Modify: activity strip |
| `scripts/dev-mock-api.mjs` | Modify: sheet, activity, drill, practice routes |

---

### Task 1: Migration 0015 and the pure sheet logic

**Files:**
- Create: `backend/migrations/0015_answer_sheets.sql`
- Create: `backend/src/sheets.rs` (pure part only; `ensure_sheet` is Task 2)
- Modify: `backend/src/main.rs` (add `mod sheets;` after `mod pavlov_stats;`)

**Interfaces:**
- Produces: `sheets::SHEET_MODEL`, `sheets::SHEET_SYSTEM_PROMPT`, `sheets::Fact { prompt, response }`, `sheets::SheetContent { identity, facts: Vec<Fact> }`, `sheets::normalize_answer(&str) -> String`, `sheets::fact_norm(&str, usize) -> String`, `sheets::sheet_user_prompt(answer, category, phrases: &[String], clues: &[GroundingClue]) -> String`, `sheets::GroundingClue { clue, category, year: Option<i32> }`, `sheets::parse_sheet(&Value, answer_norm) -> Result<SheetContent, String>`, `sheets::sample_evenly<T: Clone>(&[T], n) -> Vec<T>`, `sheets::sheet_json(&SheetContent) -> Value`.

- [ ] **Step 1: Write the migration**

```sql
-- 0015: answer sheets (LLM "identity + four facts" per answer, corpus-grounded,
-- global cache), fact cards as flagged pavlov_answers rows, and the user's
-- auto-add-facts preference. Additive only.

CREATE TABLE IF NOT EXISTS answer_sheets (
  answer_norm TEXT PRIMARY KEY,
  answer      TEXT NOT NULL,
  content     JSONB NOT NULL,   -- {"identity": "...", "facts": [{"prompt","response"} x4]}
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

- [ ] **Step 2: Write the failing tests**

Create `backend/src/sheets.rs` containing only the imports and the test module:

```rust
//! Answer sheets: a corpus-grounded identity + four drillable facts per answer.
//! Pure logic here (prompt, parsing, normalization); generation in the second half.

use serde::Serialize;
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({
            "identity": "Emily Brontë's only novel, 1847, a doomed romance on the Yorkshire moors.",
            "facts": [
                {"prompt": "author", "response": "Emily Brontë"},
                {"prompt": "antihero", "response": "Heathcliff"},
                {"prompt": "setting", "response": "Yorkshire moors"},
                {"prompt": "year published", "response": "1847"}
            ]
        })
    }

    #[test]
    fn normalize_matches_the_sql_expression() {
        assert_eq!(normalize_answer("The Jungle"), "jungle");
        assert_eq!(normalize_answer("An Apple"), "apple");
        assert_eq!(normalize_answer("A Midsummer Night's Dream"), "midsummer night's dream");
        assert_eq!(normalize_answer("Aesop"), "aesop"); // no article stripped
        assert_eq!(normalize_answer("  Wuthering Heights "), "wuthering heights");
    }

    #[test]
    fn fact_norm_shape() {
        assert_eq!(fact_norm("wuthering heights", 3), "wuthering heights::3");
    }

    #[test]
    fn parse_accepts_a_valid_sheet() {
        let s = parse_sheet(&good(), "wuthering heights").unwrap();
        assert_eq!(s.facts.len(), 4);
        assert_eq!(s.facts[0].prompt, "author");
        assert_eq!(s.facts[3].response, "1847");
        assert!(s.identity.starts_with("Emily"));
    }

    #[test]
    fn parse_rejects_wrong_fact_count() {
        let mut v = good();
        v["facts"].as_array_mut().unwrap().pop();
        assert!(parse_sheet(&v, "wuthering heights").is_err());
        let mut v = good();
        v["facts"].as_array_mut().unwrap().push(json!({"prompt": "x", "response": "y"}));
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_empty_fields() {
        let mut v = good();
        v["facts"][1]["response"] = json!("  ");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
        let mut v = good();
        v["identity"] = json!("");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_response_equal_to_answer() {
        let mut v = good();
        v["facts"][2]["response"] = json!("The Wuthering Heights");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_duplicate_prompts() {
        let mut v = good();
        v["facts"][2]["prompt"] = json!("Author");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn user_prompt_lists_answer_phrases_and_clues() {
        let clues = vec![
            GroundingClue { clue: "Heathcliff loves Cathy here".into(), category: "NOVELS".into(), year: Some(2001) },
            GroundingClue { clue: "Emily's only novel".into(), category: "BRONTËS".into(), year: None },
        ];
        let p = sheet_user_prompt("Wuthering Heights", "Literature & Language", &["Heathcliff".to_string()], &clues);
        assert!(p.contains("Answer: \"Wuthering Heights\""));
        assert!(p.contains("Category: Literature & Language"));
        assert!(p.contains("Existing cue phrases: Heathcliff"));
        assert!(p.contains("- \"Heathcliff loves Cathy here\" (NOVELS, 2001)"));
        assert!(p.contains("- \"Emily's only novel\" (BRONTËS)"));
        assert!(p.trim_end().ends_with("Return the JSON now."));
    }

    #[test]
    fn sample_evenly_keeps_ends_and_count() {
        let v: Vec<i32> = (0..30).collect();
        let s = sample_evenly(&v, 15);
        assert_eq!(s.len(), 15);
        assert_eq!(s[0], 0);
        assert_eq!(*s.last().unwrap(), 29);
        assert_eq!(sample_evenly(&v, 100).len(), 30); // fewer than n → all
        assert!(sample_evenly(&Vec::<i32>::new(), 5).is_empty());
    }

    #[test]
    fn sheet_json_round_trips() {
        let s = parse_sheet(&good(), "wuthering heights").unwrap();
        let v = sheet_json(&s);
        assert_eq!(v["facts"].as_array().unwrap().len(), 4);
        assert_eq!(v["facts"][0]["prompt"], "author");
        assert!(parse_sheet(&v, "wuthering heights").is_ok());
    }
}
```

- [ ] **Step 3: Register the module and run the tests to see them fail**

Add `mod sheets;` to `backend/src/main.rs` directly after `mod pavlov_stats;`.

Run: `cd backend && cargo test sheets:: 2>&1 | tail -5`
Expected: compile errors — `normalize_answer`, `parse_sheet`, etc. not found.

- [ ] **Step 4: Implement the pure part**

Insert between the `use` lines and the test module:

```rust
pub const SHEET_MODEL: &str = "gpt-4o-mini";

pub const SHEET_SYSTEM_PROMPT: &str = r#"You are a Jeopardy! coach building an ANSWER SHEET: the handful of facts that let a player recognize this answer from any clue angle.

Rules:
- Output ONLY valid JSON: {"identity": "...", "facts": [{"prompt": "...", "response": "..."}, ...]} with EXACTLY four facts.
- "identity": one sentence, at most 25 words, saying who or what this answer is. Proper nouns, dates and numbers only. Never restate a clue.
- Each fact is something Jeopardy writers re-use about this answer, drawn from the supplied clues where possible: creator or author, signature work, key date or first, place, counterpart or rival, nickname, unit or symbol.
- "prompt" is a 2-6 word label as it would appear in a cue: "author", "narrator", "year published", "capital city", "wrote the music".
- "response" is a short gradeable answer — a name, number, place or title — at most 6 words. Never a sentence. Never the answer itself.
- The four facts must be distinct from each other and must NOT repeat the existing cue phrases you are given; they add depth beyond the cue.
- BANNED: generic sentences about trivia, Jeopardy or "recognizing"; hedges such as "possibly" or "often"."#;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fact {
    pub prompt: String,
    pub response: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetContent {
    pub identity: String,
    pub facts: Vec<Fact>,
}

/// A clue used to ground the sheet. `clue` is the clue text shown to players
/// (jeopardy_questions.answer), `year` from air_date.
#[derive(Debug, Clone)]
pub struct GroundingClue {
    pub clue: String,
    pub category: String,
    pub year: Option<i32>,
}

/// Rust twin of the SQL `lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))`.
/// Strips ONE leading article (followed by a space), lowercases, trims.
pub fn normalize_answer(s: &str) -> String {
    let lower = s.to_lowercase();
    let stripped = ["the ", "an ", "a "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    stripped.trim().to_string()
}

/// answer_norm of the n-th fact card under a parent (n = 1..=4).
pub fn fact_norm(parent_norm: &str, n: usize) -> String {
    format!("{parent_norm}::{n}")
}

pub fn sheet_user_prompt(
    answer: &str,
    category: &str,
    phrases: &[String],
    clues: &[GroundingClue],
) -> String {
    let mut out = format!("Answer: \"{answer}\"\nCategory: {category}\n");
    out.push_str(&format!(
        "Existing cue phrases: {}\n",
        if phrases.is_empty() { "(none)".to_string() } else { phrases.join(", ") }
    ));
    out.push_str("Clues Jeopardy has written for this answer:\n");
    for c in clues {
        match c.year {
            Some(y) => out.push_str(&format!("- \"{}\" ({}, {})\n", c.clue, c.category, y)),
            None => out.push_str(&format!("- \"{}\" ({})\n", c.clue, c.category)),
        }
    }
    out.push_str("\nReturn the JSON now.");
    out
}

/// Validate the LLM's JSON. Rejections are never cached.
pub fn parse_sheet(v: &Value, answer_norm: &str) -> Result<SheetContent, String> {
    let identity = v
        .get("identity")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("missing or empty identity")?
        .to_string();
    let arr = v.get("facts").and_then(|f| f.as_array()).ok_or("missing facts")?;
    if arr.len() != 4 {
        return Err(format!("expected 4 facts, got {}", arr.len()));
    }
    let mut facts = Vec::with_capacity(4);
    let mut seen_prompts = std::collections::HashSet::new();
    for f in arr {
        let get = |k: &str| -> Result<String, String> {
            let s = f
                .get(k)
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("fact missing {k}"))?;
            Ok(s.to_string())
        };
        let prompt = get("prompt")?;
        let response = get("response")?;
        if normalize_answer(&response) == answer_norm {
            return Err(format!("fact response restates the answer: {response}"));
        }
        if !seen_prompts.insert(prompt.to_lowercase()) {
            return Err(format!("duplicate fact prompt: {prompt}"));
        }
        facts.push(Fact { prompt, response });
    }
    Ok(SheetContent { identity, facts })
}

/// Pick `n` items spread evenly across a chronologically ordered slice, always
/// keeping the first and last. Returns everything when the slice has ≤ n items.
pub fn sample_evenly<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    if items.len() <= n || n == 0 {
        return items.to_vec();
    }
    let last = items.len() - 1;
    (0..n)
        .map(|i| items[(i * last) / (n - 1)].clone())
        .collect()
}

pub fn sheet_json(s: &SheetContent) -> Value {
    serde_json::json!({ "identity": s.identity, "facts": s.facts })
}
```

- [ ] **Step 5: Run the tests**

Run: `cd backend && cargo test sheets:: 2>&1 | tail -5`
Expected: `test result: ok. 9 passed`.

- [ ] **Step 6: Commit**

```bash
git add backend/migrations/0015_answer_sheets.sql backend/src/sheets.rs backend/src/main.rs
git commit -m "feat(sheets): migration 0015 + pure sheet logic (prompt, parse, normalize, fact norm)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Sheet generation and the sheet endpoints

**Files:**
- Modify: `backend/src/sheets.rs` (append `ensure_sheet` + helpers above the test module)
- Create: `backend/src/routes/sheet.rs`
- Modify: `backend/src/main.rs` (`AppState.sheet_inflight`, its initializer, two routes)
- Modify: `backend/src/routes/mod.rs` (`pub mod sheet;`)

**Interfaces:**
- Consumes: Task 1's types; `crate::openai::chat_json(api_key, model, system, user, temperature) -> Result<Value, AppError>`.
- Produces: `sheets::ensure_sheet(state: &Arc<AppState>, answer_norm: &str) -> Result<Option<SheetContent>, AppError>`; `sheets::pregenerate_sheet(state: &Arc<AppState>, answer_norm: String)` (fire-and-forget); `sheets::facts_added(state, user_id: i32, answer_norm: &str) -> Result<bool, AppError>`; `GET /api/sheet/answer/{answer_norm}` and `GET /api/sheet/question/{question_id}` returning `{ answerNorm, answer, identity, facts, factsAdded }` or 204.

- [ ] **Step 1: Append generation to `sheets.rs`**

Add `use std::sync::Arc; use crate::error::AppError; use crate::AppState;` to the imports, then insert before `#[cfg(test)]`:

```rust
#[derive(sqlx::FromRow)]
struct ParentRow {
    answer: String,
    meta_category: String,
    phrases: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct ClueRow {
    clue: Option<String>,
    response: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    year: Option<i32>,
}

/// Cached-or-generate. Ok(None) when the key is unconfigured, the answer has
/// no clues in the corpus, or the LLM output failed validation. Single-flight
/// per answer_norm, same shape as insights::ensure_insight.
pub async fn ensure_sheet(
    state: &Arc<AppState>,
    answer_norm: &str,
) -> Result<Option<SheetContent>, AppError> {
    if let Some(c) = read_cached(state, answer_norm).await? {
        return Ok(Some(c));
    }
    if state.config.openai_api_key.is_empty() {
        return Ok(None);
    }
    {
        let mut inflight = state.sheet_inflight.lock().await;
        if !inflight.insert(answer_norm.to_string()) {
            drop(inflight);
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Some(c) = read_cached(state, answer_norm).await? {
                    return Ok(Some(c));
                }
            }
            return Ok(None);
        }
    }
    let result = generate_and_store(state, answer_norm).await;
    state.sheet_inflight.lock().await.remove(answer_norm);
    result
}

/// Fire-and-forget warm-up (used by drill_next so the sheet is ready by reveal).
pub fn pregenerate_sheet(state: &Arc<AppState>, answer_norm: String) {
    if state.config.openai_api_key.is_empty() {
        return;
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = ensure_sheet(&st, &answer_norm).await {
            tracing::warn!("sheet pregeneration failed for {answer_norm}: {e:?}");
        }
    });
}

/// Display answer for a norm: the Pavlov deck's display form when present.
pub async fn display_answer(state: &Arc<AppState>, answer_norm: &str) -> Result<Option<String>, AppError> {
    let s: Option<String> = sqlx::query_scalar(
        "SELECT answer FROM answer_sheets WHERE answer_norm = $1",
    )
    .bind(answer_norm)
    .fetch_optional(&state.pool)
    .await?;
    Ok(s)
}

/// True when the user already holds all four fact cards for this answer.
pub async fn facts_added(state: &Arc<AppState>, user_id: i32, answer_norm: &str) -> Result<bool, AppError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND pa.kind = 'fact' AND pa.parent_norm = $2",
    )
    .bind(user_id)
    .bind(answer_norm)
    .fetch_one(&state.pool)
    .await?;
    Ok(n >= 4)
}

async fn read_cached(state: &Arc<AppState>, answer_norm: &str) -> Result<Option<SheetContent>, AppError> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT content FROM answer_sheets WHERE answer_norm = $1")
            .bind(answer_norm)
            .fetch_optional(&state.pool)
            .await?;
    Ok(row.and_then(|(v,)| parse_sheet(&v, answer_norm).ok()))
}

async fn generate_and_store(
    state: &Arc<AppState>,
    answer_norm: &str,
) -> Result<Option<SheetContent>, AppError> {
    // Up to 15 clues spread across air dates; the norm join uses the same
    // expression as idx_jq_answer_norm so it is index-backed.
    let all: Vec<ClueRow> = sqlx::query_as(
        "SELECT answer AS clue, question AS response, category, classifier_category,
                EXTRACT(year FROM air_date)::int AS year
         FROM jeopardy_questions
         WHERE lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) = $1
           AND archived = false AND answer IS NOT NULL AND question IS NOT NULL
         ORDER BY air_date",
    )
    .bind(answer_norm)
    .fetch_all(&state.pool)
    .await?;
    if all.is_empty() {
        return Ok(None);
    }
    let picked = sample_evenly(&all, 15);

    let parent: Option<ParentRow> = sqlx::query_as(
        "SELECT answer, meta_category, phrases FROM pavlov_answers
         WHERE answer_norm = $1 AND kind = 'answer'",
    )
    .bind(answer_norm)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, category, phrases) = match parent {
        Some(p) => (p.answer, p.meta_category, p.phrases),
        None => (
            all[all.len() - 1].response.clone().unwrap_or_else(|| answer_norm.to_string()),
            all[all.len() - 1].classifier_category.clone().unwrap_or_else(|| "Miscellaneous".to_string()),
            vec![],
        ),
    };
    let clues: Vec<GroundingClue> = picked
        .into_iter()
        .map(|c| GroundingClue {
            clue: c.clue.unwrap_or_default(),
            category: c.category.unwrap_or_else(|| "UNKNOWN".to_string()),
            year: c.year,
        })
        .collect();

    let user = sheet_user_prompt(&answer, &category, &phrases, &clues);
    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        SHEET_MODEL,
        SHEET_SYSTEM_PROMPT,
        &user,
        0.4,
    )
    .await?;
    let content = match parse_sheet(&v, answer_norm) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("sheet rejected for {answer_norm}: {e}");
            return Ok(None);
        }
    };
    sqlx::query(
        "INSERT INTO answer_sheets (answer_norm, answer, content, model) VALUES ($1, $2, $3, $4)
         ON CONFLICT (answer_norm) DO NOTHING",
    )
    .bind(answer_norm)
    .bind(&answer)
    .bind(sheet_json(&content))
    .bind(SHEET_MODEL)
    .execute(&state.pool)
    .await?;
    Ok(Some(content))
}
```

- [ ] **Step 2: AppState and wiring**

In `backend/src/main.rs`:
- add to `pub struct AppState`: `pub sheet_inflight: tokio::sync::Mutex<std::collections::HashSet<String>>,`
- add to the `Arc::new(AppState { ... })` literal: `sheet_inflight: tokio::sync::Mutex::new(std::collections::HashSet::new()),`
- after the `/api/insight/{id}` route add:

```rust
        .route("/api/sheet/answer/{norm}", get(routes::sheet::by_answer))
        .route("/api/sheet/question/{id}", get(routes::sheet::by_question))
```

In `backend/src/routes/mod.rs` add `pub mod sheet;`.

- [ ] **Step 3: The route file**

Create `backend/src/routes/sheet.rs`:

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::sheets;
use crate::AppState;

async fn respond(state: &Arc<AppState>, user_id: i32, answer_norm: &str) -> Result<Response, AppError> {
    match sheets::ensure_sheet(state, answer_norm).await? {
        Some(c) => {
            let added = sheets::facts_added(state, user_id, answer_norm).await?;
            let answer = sheets::display_answer(state, answer_norm)
                .await?
                .unwrap_or_else(|| answer_norm.to_string());
            Ok(Json(json!({
                "answerNorm": answer_norm, "answer": answer,
                "identity": c.identity, "facts": c.facts, "factsAdded": added,
            }))
            .into_response())
        }
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub async fn by_answer(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(norm): Path<String>,
) -> Result<Response, AppError> {
    respond(&state, auth.user_id, &norm).await
}

/// Resolve a clue's normalized response server-side (same SQL expression as
/// idx_jq_answer_norm) so practice never has to know the normalization.
pub async fn by_question(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(question_id): Path<i32>,
) -> Result<Response, AppError> {
    let norm: Option<String> = sqlx::query_scalar(
        "SELECT lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))
         FROM jeopardy_questions WHERE id = $1 AND question IS NOT NULL",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await?;
    match norm {
        Some(n) => respond(&state, auth.user_id, &n).await,
        None => Err(AppError::NotFound("No such question".into())),
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cd backend && cargo build 2>&1 | grep -E "^error" | head; cargo test 2>&1 | grep "test result"`
Expected: no errors; `97 passed` (88 + 9).

- [ ] **Step 5: Commit**

```bash
git add backend/src/sheets.rs backend/src/routes/sheet.rs backend/src/routes/mod.rs backend/src/main.rs
git commit -m "feat(sheets): cached, single-flight, corpus-grounded sheet generation + /api/sheet endpoints

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Fact cards and the `kind='answer'` filters

**Files:**
- Create: `backend/src/routes/pavlov_facts.rs`
- Modify: `backend/src/routes/pavlov.rs` (`DrillAnswerRow`, `drill_card_json`, `drill_next` queries + prefetch, `pick_new_card` filters, `more_new_available`, `drill_check`, `drill_grade` auto-add, `answers` list)
- Modify: `backend/src/pavlov.rs` (stale cleanup)
- Modify: `backend/src/routes/pavlov_stats.rs` (`deckTotal`, `touched`)
- Modify: `backend/src/routes/preferences.rs` (`pavlovAutoFacts`)
- Modify: `backend/src/main.rs`, `backend/src/routes/mod.rs` (route + module)
- Create: `scripts/verify-sheets.sql`

**Interfaces:**
- Consumes: `sheets::{parse_sheet, fact_norm, facts_added, pregenerate_sheet}`.
- Produces: `pavlov_facts::add_fact_cards(state, user_id, answer_norm) -> Result<Option<i64>, AppError>` (None = no sheet cached); `POST /api/pavlov/facts { answerNorm } -> { added }`; drill payloads gain `card.kind`, `card.parent`, `card.answerNorm`; `drill_check` gains `kind`, `parent`, `answerNorm`; `drill_grade` gains `factsAdded`; preferences gain `pavlovAutoFacts`.

- [ ] **Step 1: `pavlov_facts.rs`**

```rust
//! Fact cards: the four facts of an answer sheet become four extra Pavlov
//! cards (kind='fact') under the parent answer, so the topic is drilled in
//! depth. Idempotent: re-adding is a no-op.
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::sheets::{fact_norm, parse_sheet};
use crate::AppState;

#[derive(sqlx::FromRow)]
struct ParentRow {
    meta_category: String,
    score: f32,
    answer_freq: i32,
    example_clue_ids: Vec<i32>,
}

/// Upsert the four fact rows and give the user a due-now card for each one
/// they lack. Returns Ok(None) when no sheet is cached for this answer.
pub async fn add_fact_cards(
    state: &Arc<AppState>,
    user_id: i32,
    answer_norm: &str,
) -> Result<Option<i64>, AppError> {
    let sheet: Option<(Value,)> =
        sqlx::query_as("SELECT content FROM answer_sheets WHERE answer_norm = $1")
            .bind(answer_norm)
            .fetch_optional(&state.pool)
            .await?;
    let Some((content,)) = sheet else { return Ok(None) };
    let sheet = match parse_sheet(&content, answer_norm) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };

    // Parent metadata: the deck row when present, else derived from the corpus
    // (practice misses can be answers outside the Pavlov deck).
    let parent: Option<ParentRow> = sqlx::query_as(
        "SELECT meta_category, score, answer_freq, example_clue_ids FROM pavlov_answers
         WHERE answer_norm = $1 AND kind = 'answer'",
    )
    .bind(answer_norm)
    .fetch_optional(&state.pool)
    .await?;
    let parent = match parent {
        Some(p) => p,
        None => {
            let (cat, freq, ids): (Option<String>, i32, Vec<i32>) = sqlx::query_as(
                "SELECT max(classifier_category), COALESCE(max(answer_freq), 1),
                        COALESCE(array_agg(id ORDER BY air_date DESC), '{}')
                 FROM (SELECT classifier_category, answer_freq, id, air_date FROM jeopardy_questions
                       WHERE lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) = $1
                         AND archived = false LIMIT 5) q",
            )
            .bind(answer_norm)
            .fetch_one(&state.pool)
            .await?;
            ParentRow {
                meta_category: cat.unwrap_or_else(|| "Miscellaneous".to_string()),
                score: 0.0,
                answer_freq: freq,
                example_clue_ids: ids,
            }
        }
    };

    let mut added = 0i64;
    for (i, fact) in sheet.facts.iter().enumerate() {
        let norm = fact_norm(answer_norm, i + 1);
        let answer_id: i32 = sqlx::query_scalar(
            "INSERT INTO pavlov_answers
               (answer_norm, answer, meta_category, phrases, phrase_tiers, score,
                example_clue_ids, answer_freq, kind, parent_norm)
             VALUES ($1, $2, $3, $4, '{standard}', $5, $6, $7, 'fact', $8)
             ON CONFLICT (answer_norm) DO UPDATE SET
               answer = EXCLUDED.answer, phrases = EXCLUDED.phrases,
               meta_category = EXCLUDED.meta_category
             RETURNING id",
        )
        .bind(&norm)
        .bind(&fact.response)
        .bind(&parent.meta_category)
        .bind(vec![fact.prompt.clone()])
        .bind(parent.score)
        .bind(&parent.example_clue_ids)
        .bind(parent.answer_freq)
        .bind(answer_norm)
        .fetch_one(&state.pool)
        .await?;
        let res = sqlx::query(
            "INSERT INTO pavlov_cards (user_id, answer_id) VALUES ($1, $2)
             ON CONFLICT (user_id, answer_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(answer_id)
        .execute(&state.pool)
        .await?;
        added += res.rows_affected() as i64;
    }
    Ok(Some(added))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactsBody {
    pub answer_norm: String,
}

pub async fn add_facts(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<FactsBody>,
) -> Result<Json<Value>, AppError> {
    match add_fact_cards(&state, auth.user_id, &body.answer_norm).await? {
        Some(added) => Ok(Json(json!({ "added": added }))),
        None => Err(AppError::NotFound("No sheet for that answer yet".into())),
    }
}
```

Note: `pavlov_cards (user_id, answer_id)` has a UNIQUE constraint (the grade upsert relies on `ON CONFLICT (user_id, answer_id)`), so the card insert is idempotent. New rows use the table defaults: `state='learning'`, `due=now()`, `last_review NULL` — a due-now learning card. Because `last_review` is NULL it does not count against the daily new-card allowance.

Wire it: `pub mod pavlov_facts;` in `routes/mod.rs`; in `main.rs` after the `/api/pavlov/drill/grade` route add
`.route("/api/pavlov/facts", post(routes::pavlov_facts::add_facts))`.

- [ ] **Step 2: `DrillAnswerRow` carries norm, kind, parent; all drill queries select them; prefetch**

In `backend/src/routes/pavlov.rs` replace the struct and json fn:

```rust
#[derive(sqlx::FromRow)]
struct DrillAnswerRow {
    id: i32,
    answer_norm: String,
    kind: String,
    parent: Option<String>,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    meta_category: String,
}

fn drill_card_json(r: &DrillAnswerRow) -> Value {
    let phrases: Vec<Value> = r
        .phrases
        .iter()
        .zip(r.phrase_tiers.iter())
        .map(|(text, tier)| json!({ "text": text, "tier": tier }))
        .collect();
    json!({
        "answerId": r.id, "answerNorm": r.answer_norm, "kind": r.kind, "parent": r.parent,
        "phrases": phrases, "category": r.meta_category,
    })
}

/// The SELECT list every drill query shares (parent = the parent's display answer).
const DRILL_COLS: &str = "pa.id, pa.answer_norm, pa.kind,
    (SELECT p2.answer FROM pavlov_answers p2 WHERE p2.answer_norm = pa.parent_norm) AS parent,
    pa.phrases, pa.phrase_tiers, pa.meta_category";
```

Then:
- In `pick_new_card`: the `available` query gains `AND kind = 'answer'`; `PICK_IN_CAT` and `PICK_ANY` become `format!`-built strings using `DRILL_COLS` and add `AND pa.kind = 'answer'` (alias the table `pavlov_answers pa`). Concretely:

```rust
    let pick_in_cat = format!(
        "SELECT {DRILL_COLS} FROM pavlov_answers pa
         WHERE pa.meta_category = $2 AND pa.kind = 'answer'
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY pa.answer_freq DESC, pa.score DESC, pa.id LIMIT 1"
    );
    let pick_any = format!(
        "SELECT {DRILL_COLS} FROM pavlov_answers pa
         WHERE pa.kind = 'answer'
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY pa.answer_freq DESC, pa.score DESC, pa.id LIMIT 1"
    );
```

  and use `sqlx::query_as::<_, DrillAnswerRow>(&pick_in_cat)` / `(&pick_any)`.
- In `drill_next`: `fetch_due` becomes `format!("SELECT {DRILL_COLS} FROM pavlov_cards ca JOIN pavlov_answers pa ON pa.id = ca.answer_id WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now() ORDER BY ca.due ASC LIMIT 1")` used as `&fetch_due`; `more_new_available` adds `AND pa.kind = 'answer'`. At each of the three `return Ok(Json(json!({ "done": false, ... "card": drill_card_json(row) ...` sites, first call `crate::sheets::pregenerate_sheet(&state, row.answer_norm.clone());` for `kind == "answer"` rows, and pass `&row` to `drill_card_json`:

```rust
            if row.kind == "answer" {
                crate::sheets::pregenerate_sheet(&state, row.answer_norm.clone());
            }
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": drill_card_json(&row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
```

- [ ] **Step 3: `drill_check` and `drill_grade`**

`drill_check`: change the first query to
`"SELECT answer, example_clue_ids, answer_norm, kind, (SELECT p2.answer FROM pavlov_answers p2 WHERE p2.answer_norm = pavlov_answers.parent_norm) FROM pavlov_answers WHERE id = $1"` with row type `Option<(String, Vec<i32>, String, String, Option<String>)>`, and return
`json!({ "correct": correct, "answer": answer, "answerNorm": answer_norm, "kind": kind, "parent": parent, "examples": examples })`.

`drill_grade`: after `tx.commit().await?;` add

```rust
    // Auto-add fact cards on a Wrong for a real answer when the user opted in.
    let mut facts_added: i64 = 0;
    if rating == Rating::Wrong {
        let (kind, norm): (String, String) = sqlx::query_as(
            "SELECT kind, answer_norm FROM pavlov_answers WHERE id = $1",
        )
        .bind(body.answer_id)
        .fetch_one(&state.pool)
        .await?;
        let auto: bool = sqlx::query_scalar("SELECT pavlov_auto_facts FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;
        if kind == "answer" && auto {
            // The sheet is normally prefetched by drill_next; ensure it exists.
            if crate::sheets::ensure_sheet(&state, &norm).await?.is_some() {
                facts_added = crate::routes::pavlov_facts::add_fact_cards(&state, user_id, &norm)
                    .await?
                    .unwrap_or(0);
            }
        }
    }
```

and add `"factsAdded": facts_added` to the response JSON. `Rating` needs `PartialEq`: check `backend/src/srs.rs` line 22 — if the enum lacks `#[derive(PartialEq)]`, add `PartialEq` to its derive list (it is `Copy` already).

- [ ] **Step 4: List handler, stale cleanup, deck progress, preferences**

`answers` list (`routes/pavlov.rs`): add `kind: String, parent: Option<String>` to `AnswerListRow`; SELECT gains `pa.kind, (SELECT p2.answer FROM pavlov_answers p2 WHERE p2.answer_norm = pa.parent_norm) AS parent`; the JSON gains `"kind": r.kind, "parent": r.parent`.

`backend/src/pavlov.rs` stale cleanup: `"DELETE FROM pavlov_answers WHERE kind = 'answer' AND answer_norm NOT IN (...)"` (keep the subquery as is).

`backend/src/routes/pavlov_stats.rs`: `deck_total` query becomes `SELECT COUNT(*) FROM pavlov_answers WHERE kind = 'answer'`; replace `let touched = learning + maturing + mastered + struggling + banished;` with

```rust
    // Deck coverage counts real answers only; fact cards are depth, not progress.
    let touched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND pa.kind = 'answer'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
```

but keep `"total": learning + maturing + mastered + struggling + banished` in the `deck` JSON (buckets include facts). Also add `AND pa.kind = 'answer'` to the `created_14d` query? **No** — leave it: the trailing pace counts cards you worked, and fact cards are due-now work you chose. (Spec: only `deckTotal`/`touched` exclude facts.)

`routes/preferences.rs`: SELECT gains `pavlov_auto_facts` as the 7th column (`bool`), JSON gains `"pavlovAutoFacts": row.6`; body gains `pub pavlov_auto_facts: Option<bool>,`; update block:

```rust
    if let Some(b) = body.pavlov_auto_facts {
        sqlx::query("UPDATE users SET pavlov_auto_facts = $1 WHERE id = $2")
            .bind(b)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }
```

- [ ] **Step 5: Build and test**

Run: `cd backend && cargo build 2>&1 | grep -E "^(error|warning: unused)" | head; cargo test 2>&1 | grep "test result"`
Expected: no errors, no new warnings, `97 passed`.

- [ ] **Step 6: Verify script**

Create `scripts/verify-sheets.sql`:

```sql
-- Read-only checks for answer sheets / fact cards / activity (PG15). Run after
-- migration 0015: docker run --rm -i postgres:16 psql "$DB_URL" -f - < scripts/verify-sheets.sql
-- "expect 0" checks fail when nonzero. user_id = 1.

-- A. Tables and columns exist.
SELECT 'answer_sheets' AS t, count(*) AS rows FROM answer_sheets;
SELECT kind, count(*) FROM pavlov_answers GROUP BY 1;
SELECT pavlov_auto_facts FROM users WHERE id = 1;

-- B. expect 0: fact rows with a missing/unknown parent.
SELECT 'fact_without_parent' AS check, count(*) AS fail_rows
FROM pavlov_answers f WHERE f.kind = 'fact'
  AND NOT EXISTS (SELECT 1 FROM pavlov_answers p WHERE p.answer_norm = f.parent_norm AND p.kind = 'answer')
  AND NOT EXISTS (SELECT 1 FROM answer_sheets s WHERE s.answer_norm = f.parent_norm);

-- C. expect 0: deck-progress total must never count facts (mirror of pavlov_stats deck_total).
SELECT 'facts_in_deck_total' AS check,
       (SELECT count(*) FROM pavlov_answers) - (SELECT count(*) FROM pavlov_answers WHERE kind = 'answer')
       - (SELECT count(*) FROM pavlov_answers WHERE kind = 'fact') AS fail_rows;

-- D. Fact-card upsert + card insert shape, rolled back (uses a synthetic parent).
BEGIN;
INSERT INTO answer_sheets (answer_norm, answer, content, model) VALUES
 ('__verify_parent', 'Verify Parent',
  '{"identity":"x","facts":[{"prompt":"a","response":"1"},{"prompt":"b","response":"2"},{"prompt":"c","response":"3"},{"prompt":"d","response":"4"}]}', 'test')
ON CONFLICT DO NOTHING;
INSERT INTO pavlov_answers (answer_norm, answer, meta_category, phrases, phrase_tiers, score, example_clue_ids, answer_freq, kind, parent_norm)
VALUES ('__verify_parent::1', '1', 'Miscellaneous', '{a}', '{standard}', 0, '{}', 1, 'fact', '__verify_parent')
ON CONFLICT (answer_norm) DO UPDATE SET answer = EXCLUDED.answer RETURNING id, kind, parent_norm;
INSERT INTO pavlov_cards (user_id, answer_id)
SELECT 1, id FROM pavlov_answers WHERE answer_norm = '__verify_parent::1'
ON CONFLICT (user_id, answer_id) DO NOTHING;
SELECT 'verify_card_due_now' AS check, count(*) FILTER (WHERE due > now() OR state <> 'learning' OR last_review IS NOT NULL) AS fail_rows
FROM pavlov_cards ca JOIN pavlov_answers pa ON pa.id = ca.answer_id WHERE pa.answer_norm = '__verify_parent::1';
ROLLBACK;

-- E. Activity series shape (mirror of routes/activity.rs).
WITH d AS (SELECT generate_series((now() AT TIME ZONE 'America/Los_Angeles')::date - 27, (now() AT TIME ZONE 'America/Los_Angeles')::date, '1 day')::date AS day),
act AS (
  SELECT (answered_at AT TIME ZONE 'America/Los_Angeles')::date AS day FROM question_attempts WHERE user_id = 1
  UNION SELECT (reviewed_at AT TIME ZONE 'America/Los_Angeles')::date FROM pavlov_reviews WHERE user_id = 1
  UNION SELECT (created_at AT TIME ZONE 'America/Los_Angeles')::date FROM pavlov_cards WHERE user_id = 1
  UNION SELECT (last_review AT TIME ZONE 'America/Los_Angeles')::date FROM pavlov_cards WHERE user_id = 1 AND last_review IS NOT NULL)
SELECT d.day, EXISTS (SELECT 1 FROM act WHERE act.day = d.day) AS active FROM d ORDER BY d.day;

-- F. Sheet-by-question norm resolution matches pavlov_answers for a deck answer (expect > 0 matches).
SELECT count(*) AS deck_answers_resolvable
FROM pavlov_answers pa WHERE pa.kind = 'answer' AND EXISTS (
  SELECT 1 FROM jeopardy_questions jq WHERE jq.id = pa.example_clue_ids[1]
    AND lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = pa.answer_norm);
```

- [ ] **Step 7: Commit**

```bash
git add backend/src/routes/pavlov_facts.rs backend/src/routes/pavlov.rs backend/src/pavlov.rs backend/src/routes/pavlov_stats.rs backend/src/routes/preferences.rs backend/src/routes/mod.rs backend/src/main.rs backend/src/srs.rs scripts/verify-sheets.sql
git commit -m "feat(pavlov): fact cards (kind='fact' answers), auto-add on Wrong, drill payload kind/parent, kind filters

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

(Drop `backend/src/srs.rs` from the `git add` if it needed no change.)

---

### Task 4: Activity summary and endpoint

**Files:**
- Create: `backend/src/activity.rs`
- Create: `backend/src/routes/activity.rs`
- Modify: `backend/src/main.rs` (`mod activity;`, route), `backend/src/routes/mod.rs` (`pub mod activity;`)

**Interfaces:**
- Consumes: `crate::routes::practice::day_start_utc` is NOT needed; the route computes the local date with chrono-tz directly.
- Produces: `activity::summarize(days: &[bool]) -> Summary { streak: i64, active: i64 }` (days oldest→newest, last element = today); `GET /api/activity -> { streak, activeLast28, days: [{date, active}] }`.

- [ ] **Step 1: Failing tests**

Create `backend/src/activity.rs`:

```rust
//! Active-days summary for the dashboard strip.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub streak: i64,
    pub active: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_active_and_streak_ending_today() {
        let d = [false, true, true, true];
        assert_eq!(summarize(&d), Summary { streak: 3, active: 3 });
    }

    #[test]
    fn today_not_yet_active_counts_from_yesterday() {
        let d = [true, true, true, false];
        assert_eq!(summarize(&d), Summary { streak: 3, active: 3 });
    }

    #[test]
    fn gap_before_yesterday_breaks_streak() {
        let d = [true, true, false, true, false];
        assert_eq!(summarize(&d), Summary { streak: 1, active: 3 });
    }

    #[test]
    fn all_inactive_is_zero() {
        assert_eq!(summarize(&[false, false, false]), Summary { streak: 0, active: 0 });
        assert_eq!(summarize(&[]), Summary { streak: 0, active: 0 });
    }

    #[test]
    fn streak_can_span_the_whole_window() {
        let d = vec![true; 28];
        assert_eq!(summarize(&d), Summary { streak: 28, active: 28 });
    }
}
```

- [ ] **Step 2: Register and watch it fail**

`mod activity;` in `main.rs` after `mod adaptive;`. Run: `cd backend && cargo test activity:: 2>&1 | tail -3` → compile error, `summarize` not found.

- [ ] **Step 3: Implement**

Insert above the test module:

```rust
/// `days` is oldest → newest with the last element being today. The streak
/// counts consecutive active days ending today, or ending yesterday when
/// today is not active yet (so the number does not reset every morning).
pub fn summarize(days: &[bool]) -> Summary {
    let active = days.iter().filter(|d| **d).count() as i64;
    let mut idx = days.len();
    if idx > 0 && !days[idx - 1] {
        idx -= 1; // skip a not-yet-active today
    }
    let streak = days[..idx].iter().rev().take_while(|d| **d).count() as i64;
    Summary { streak, active }
}
```

Run: `cargo test activity:: 2>&1 | tail -3` → `5 passed`.

- [ ] **Step 4: Route**

Create `backend/src/routes/activity.rs`:

```rust
use axum::{extract::State, Json};
use chrono::{NaiveDate, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::activity::summarize;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

const WINDOW_DAYS: i64 = 28;

pub async fn activity(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let tz: Option<String> = sqlx::query_scalar("SELECT timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;
    let zone: chrono_tz::Tz = tz
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let today = Utc::now().with_timezone(&zone).date_naive();

    // One row per local day in the window; active when any attempt, Pavlov
    // review, or (pre-review-log history) Pavlov card creation/review landed
    // on that day.
    let rows: Vec<(NaiveDate, bool)> = sqlx::query_as(
        "WITH d AS (SELECT generate_series($2::date - ($4::int - 1), $2::date, '1 day')::date AS day),
         act AS (
           SELECT (answered_at AT TIME ZONE $3)::date AS day FROM question_attempts WHERE user_id = $1
           UNION SELECT (reviewed_at AT TIME ZONE $3)::date FROM pavlov_reviews WHERE user_id = $1
           UNION SELECT (created_at AT TIME ZONE $3)::date FROM pavlov_cards WHERE user_id = $1
           UNION SELECT (last_review AT TIME ZONE $3)::date FROM pavlov_cards WHERE user_id = $1 AND last_review IS NOT NULL)
         SELECT d.day, EXISTS (SELECT 1 FROM act WHERE act.day = d.day) AS active
         FROM d ORDER BY d.day",
    )
    .bind(user_id)
    .bind(today)
    .bind(zone.name())
    .bind(WINDOW_DAYS as i32)
    .fetch_all(&state.pool)
    .await?;

    let flags: Vec<bool> = rows.iter().map(|(_, a)| *a).collect();
    let s = summarize(&flags);
    let days: Vec<Value> = rows.into_iter().map(|(d, a)| json!({ "date": d, "active": a })).collect();
    Ok(Json(json!({ "streak": s.streak, "activeLast28": s.active, "days": days })))
}
```

Wire: `pub mod activity;` in `routes/mod.rs`; in `main.rs` after the `/api/stats` route add `.route("/api/activity", get(routes::activity::activity))`.

- [ ] **Step 5: Build, test, commit**

Run: `cd backend && cargo build 2>&1 | grep -E "^error" | head; cargo test 2>&1 | grep "test result"` → `102 passed`.

```bash
git add backend/src/activity.rs backend/src/routes/activity.rs backend/src/routes/mod.rs backend/src/main.rs
git commit -m "feat(activity): active-days summary (streak, last-28) + GET /api/activity

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Frontend — AnswerSheet component, Pavlov pause, practice pause, settings, list

**Files:**
- Create: `frontend/src/lib/components/AnswerSheet.svelte`
- Modify: `frontend/src/routes/pavlov/+page.svelte`
- Modify: `frontend/src/routes/practice/+page.svelte` (script: state + fetch; markup: `pausePanel` snippet)
- Modify: `frontend/src/routes/settings/+page.svelte`
- Modify: `frontend/src/routes/pavlov/list/+page.svelte`

**Interfaces:**
- Consumes: `GET /api/sheet/answer/{norm}` / `GET /api/sheet/question/{id}` → `{ answerNorm, answer, identity, facts: [{prompt, response}], factsAdded }` or 204 (the `api` helper returns `null` on an empty body); `POST /api/pavlov/facts { answerNorm } -> { added }`; drill payloads `card.kind`, `card.parent`, `card.answerNorm`; `drill_check` `kind`, `parent`, `answerNorm`; `drill_grade` `factsAdded`; preferences `pavlovAutoFacts`.
- Produces: `AnswerSheet` props `{ sheet: Sheet | null, loading: boolean, factsAdded: boolean, addedNow?: number, onAddFacts: () => Promise<void> }`; exported type `Sheet`.

- [ ] **Step 1: `AnswerSheet.svelte`**

```svelte
<script lang="ts" module>
  export interface Sheet {
    answerNorm: string;
    answer: string;
    identity: string;
    facts: Array<{ prompt: string; response: string }>;
    factsAdded: boolean;
  }
</script>

<script lang="ts">
  let {
    sheet,
    loading,
    factsAdded,
    addedNow = 0,
    onAddFacts,
  }: {
    sheet: Sheet | null;
    loading: boolean;
    factsAdded: boolean;
    addedNow?: number;
    onAddFacts: () => Promise<void>;
  } = $props();

  let adding = $state(false);
  async function add() {
    if (adding || factsAdded) return;
    adding = true;
    try {
      await onAddFacts();
    } finally {
      adding = false;
    }
  }
</script>

<!-- Dark-surface renderer (both the Pavlov card and the practice card are blue). -->
{#if loading && !sheet}
  <div class="flex items-center gap-2 text-white/70 text-sm py-2">
    <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-jeopardy-gold"></div>
    Building the answer sheet…
  </div>
{:else if sheet}
  <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3 text-left">
    <p class="text-xs uppercase tracking-wide text-white/50 mb-1">Answer sheet</p>
    <p class="text-white/90 text-sm leading-relaxed">{sheet.identity}</p>
    <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
      {#each sheet.facts as f (f.prompt)}
        <dt class="text-white/60">{f.prompt}</dt>
        <dd class="text-jeopardy-gold font-semibold">→ {f.response}</dd>
      {/each}
    </dl>
    <div class="mt-3 flex items-center gap-3">
      {#if factsAdded}
        <span class="text-xs text-green-300">{addedNow > 0 ? `${addedNow} fact cards added` : 'In your deck'}</span>
      {:else}
        <button
          onclick={add}
          disabled={adding}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue text-xs font-bold hover:bg-yellow-400 disabled:opacity-50 transition-colors"
        >
          Drill these 4 <span class="font-normal opacity-70">(d)</span>
        </button>
      {/if}
    </div>
  </div>
{/if}
```

- [ ] **Step 2: Pavlov page — script**

Replace the `card`, `result` state declarations and the `grade`/`onKeydown` functions, and add sheet state. The full new script body is:

```ts
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import CountdownTimer from '$lib/components/CountdownTimer.svelte';
  import AnswerSheet, { type Sheet } from '$lib/components/AnswerSheet.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let card = $state<{
    answerId: number;
    answerNorm: string;
    kind: 'answer' | 'fact';
    parent: string | null;
    phrases: Array<{ text: string; tier: string }>;
    category: string;
  } | null>(null);
  let isNew = $state(false);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);
  let nextDueAt = $state<string | null>(null);
  let dueSoonCount = $state(0);
  let moreNewAvailable = $state(false);
  let extraMode = $state(false); // past-allowance drilling; resets on reload
  let result = $state<{
    answer: string;
    answerNorm: string;
    kind: 'answer' | 'fact';
    parent: string | null;
    examples: Array<{ clue: string; category: string | null; airDate: string | null }>;
  } | null>(null);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state('');
  let session = $state({ total: 0, correct: 0 });

  // Answer sheet: shown on demand (e) after reveal, and automatically on a
  // Wrong for a real answer card (paused until Next).
  let sheet = $state<Sheet | null>(null);
  let sheetLoading = $state(false);
  let sheetOpen = $state(false);
  let paused = $state(false);
  let addedNow = $state(0);

  async function fetchNext() {
    loading = true;
    error = '';
    result = null;
    sheet = null;
    sheetOpen = false;
    paused = false;
    addedNow = 0;
    try {
      const res = await api.get(`/api/pavlov/drill/next${extraMode ? '?extra=true' : ''}`);
      dueCount = res.dueCount ?? 0;
      newRemaining = res.newRemaining ?? 0;
      if (res.done) {
        done = true;
        card = null;
        nextDueAt = res.nextDueAt ?? null;
        dueSoonCount = res.dueSoonCount ?? 0;
        moreNewAvailable = res.moreNewAvailable ?? false;
      } else {
        done = false;
        card = res.card;
        isNew = res.isNew;
      }
    } catch (e: any) {
      error = e.message || 'Failed to load';
    } finally {
      loading = false;
    }
  }

  async function reveal() {
    if (!card || submitting) return;
    submitting = true;
    error = '';
    try {
      result = await api.post('/api/pavlov/drill/check', { answerId: card.answerId });
    } catch (e: any) {
      error = e.message || 'Reveal failed';
    } finally {
      submitting = false;
    }
  }

  async function fetchSheet() {
    if (!card || card.kind !== 'answer' || sheet || sheetLoading) return;
    sheetLoading = true;
    try {
      sheet = await api.get(`/api/sheet/answer/${encodeURIComponent(card.answerNorm)}`);
    } catch {
      sheet = null; // no key / rejected sheet: nothing to show
    } finally {
      sheetLoading = false;
    }
  }

  function toggleSheet() {
    if (!result || !card || card.kind !== 'answer') return;
    sheetOpen = !sheetOpen;
    if (sheetOpen) fetchSheet();
  }

  async function addFacts() {
    if (!card) return;
    try {
      const res = await api.post('/api/pavlov/facts', { answerNorm: card.answerNorm });
      addedNow = res.added ?? 0;
      if (sheet) sheet = { ...sheet, factsAdded: true };
    } catch (e: any) {
      error = e.message || 'Could not add fact cards';
    }
  }

  async function grade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (!card || submitting || paused) return;
    submitting = true;
    try {
      const res = await api.post('/api/pavlov/drill/grade', { answerId: card.answerId, rating });
      session = {
        total: session.total + 1,
        correct: session.correct + (rating === 'wrong' ? 0 : 1),
      };
      if (rating === 'wrong' && card.kind === 'answer') {
        // Teaching pause: stay on the card with the sheet open.
        paused = true;
        sheetOpen = true;
        addedNow = res.factsAdded ?? 0;
        await fetchSheet();
        if (addedNow > 0 && sheet) sheet = { ...sheet, factsAdded: true };
      } else {
        await fetchNext();
      }
    } catch (e: any) {
      error = e.message || 'Grade failed';
    } finally {
      submitting = false;
    }
  }

  async function advance() {
    if (!paused) return;
    await fetchNext();
  }

  async function banish() {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post(`/api/pavlov/answers/${card.answerId}/suspend`, { suspended: true });
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Banish failed';
    } finally {
      submitting = false;
    }
  }

  // Space/Enter reveals (or advances while paused); 1/2/3 self-grade after
  // reveal (honesty mode); b banishes anytime; e toggles the sheet; d adds facts.
  function onKeydown(e: KeyboardEvent) {
    if (!card || submitting) return;
    if (e.key === 'b' || e.key === 'B') {
      e.preventDefault();
      banish();
    } else if (paused && (e.key === ' ' || e.key === 'Enter')) {
      e.preventDefault();
      advance();
    } else if (!result && (e.key === ' ' || e.key === 'Enter')) {
      e.preventDefault();
      reveal();
    } else if (result && (e.key === 'e' || e.key === 'E')) {
      e.preventDefault();
      toggleSheet();
    } else if (result && (e.key === 'd' || e.key === 'D')) {
      e.preventDefault();
      if (sheet && !sheet.factsAdded) addFacts();
    } else if (result && !paused) {
      if (e.key === '1') grade('wrong');
      else if (e.key === '2') grade('got_it');
      else if (e.key === '3') grade('too_easy');
    }
  }

  function keepGoing() {
    extraMode = true;
    fetchNext();
  }

  onMount(fetchNext);
```

- [ ] **Step 3: Pavlov page — markup**

Inside the card header, directly under the category/new badge `<div class="flex items-center gap-2">…</div>` (still inside the header's first child), add nothing; instead, replace the cue-phrases block so fact cards show their parent:

```svelte
        <!-- Cue phrases (the question); fact cards name their parent answer -->
        <div class="flex flex-col items-center justify-center px-6 py-8 gap-3">
          {#if card.kind === 'fact' && card.parent}
            <p class="text-sm text-white/60">↳ {card.parent}</p>
          {/if}
          <div class="flex flex-wrap gap-2 justify-center">
            {#each card.phrases as phrase}
              <span class="px-4 py-2 rounded-full border text-xl sm:text-2xl font-bold inline-block
                {phrase.tier === 'hint'
                  ? 'border-white/10 text-white/50'
                  : 'border-white/25 text-jeopardy-gold'}">{phrase.text}</span>
            {/each}
          </div>
        </div>
```

Replace the reveal block (`{:else}` … before `{/if}` closing `!result`) with:

```svelte
          {:else}
            <div class="bg-white rounded-xl px-5 py-4 mb-4 text-center">
              {#if result.kind === 'fact' && result.parent}
                <p class="text-xs text-gray-500 mb-1">↳ {result.parent}</p>
              {/if}
              <p class="text-gray-900 font-bold text-xl">{result.answer}</p>
            </div>
            {#if paused}
              <div class="flex flex-col gap-3">
                <AnswerSheet {sheet} loading={sheetLoading} factsAdded={sheet?.factsAdded ?? false} {addedNow} onAddFacts={addFacts} />
                <button
                  onclick={advance}
                  class="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 border border-white/20 text-white font-semibold text-lg transition-colors"
                >
                  Next →
                </button>
                <p class="text-center text-xs text-white/40">Space / Enter</p>
              </div>
            {:else}
              <div class="grid grid-cols-3 gap-2">
                <button onclick={() => grade('wrong')} disabled={submitting}
                  class="py-3 rounded-xl bg-red-500 hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Wrong</button>
                <button onclick={() => grade('got_it')} disabled={submitting}
                  class="py-3 rounded-xl bg-green-500 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Got it</button>
                <button onclick={() => grade('too_easy')} disabled={submitting}
                  class="py-3 rounded-xl bg-blue-500 hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Too easy</button>
              </div>
              <p class="mt-2 text-center text-xs text-white/40">1 / 2 / 3</p>
              {#if result.kind === 'answer'}
                {#if sheetOpen}
                  <div class="mt-3">
                    <AnswerSheet {sheet} loading={sheetLoading} factsAdded={sheet?.factsAdded ?? false} {addedNow} onAddFacts={addFacts} />
                  </div>
                {:else}
                  <button onclick={toggleSheet} class="mt-3 w-full py-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors">
                    Learn this answer <span class="opacity-60">(e)</span>
                  </button>
                {/if}
              {/if}
            {/if}
            {#if result.examples.length > 0}
              <div class="mt-4 pt-4 border-t border-white/10 text-sm text-white/80 space-y-2">
                {#each result.examples as ex}
                  <p>"{ex.clue}" <span class="text-white/50">({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</span></p>
                {/each}
              </div>
            {/if}
          {/if}
```

Also update the page's intro copy line to mention the keys: replace `Trigger keywords → answer. Train the reflex, not the clue. Banish (b) removes a bad card —` with `Trigger keywords → answer. Train the reflex, not the clue. Wrong pauses on the answer sheet; e opens it any time, d drills its four facts. Banish (b) removes a bad card —`.

- [ ] **Step 4: Practice page**

Script: add `import AnswerSheet, { type Sheet } from '$lib/components/AnswerSheet.svelte';` and state
```ts
  let sheet = $state<Sheet | null>(null);
  let sheetLoading = $state(false);
  let sheetAddedNow = $state(0);
```
In `handleGrade`, right after `fetchInsight(question.id);` add `fetchSheet(question.id);`. Add:

```ts
  async function fetchSheet(questionId: number) {
    sheet = null;
    sheetAddedNow = 0;
    sheetLoading = true;
    try {
      sheet = await api.get(`/api/sheet/question/${questionId}`);
    } catch {
      sheet = null;
    } finally {
      sheetLoading = false;
    }
  }

  async function addSheetFacts() {
    if (!sheet) return;
    try {
      const res = await api.post('/api/pavlov/facts', { answerNorm: sheet.answerNorm });
      sheetAddedNow = res.added ?? 0;
      sheet = { ...sheet, factsAdded: true };
    } catch (err: any) {
      error = err?.message ?? 'Could not add fact cards';
    }
  }
```

In `advanceFromPause` (and wherever `insight = null` is reset on the next card) also reset `sheet = null; sheetAddedNow = 0;`.

Markup: in the `pausePanel` snippet, directly after the `{:else if insight} … {/if}` block and before the Next button, add:

```svelte
              {#if sheetLoading || sheet}
                <AnswerSheet {sheet} loading={sheetLoading} factsAdded={sheet?.factsAdded ?? false} addedNow={sheetAddedNow} onAddFacts={addSheetFacts} />
              {/if}
```

- [ ] **Step 5: Settings and list**

Settings: state `let pavlovAutoFacts = $state(false);`, load `pavlovAutoFacts = prefs?.pavlovAutoFacts ?? false;`, include `pavlovAutoFacts,` in the PUT body, and after the "Pavlov target date" label add:

```svelte
        <label class="flex items-center gap-2 text-sm text-gray-700 cursor-pointer">
          <input type="checkbox" bind:checked={pavlovAutoFacts} onchange={saveSrsPrefs} />
          Auto-add fact cards when I miss a Pavlov card
        </label>
```

List (`pavlov/list/+page.svelte`): extend `type Card` with `kind: 'answer' | 'fact'; parent: string | null;` and change `<span>{card.answer}</span>` to
`<span>{#if card.kind === 'fact' && card.parent}<span class="text-gray-400 text-xs mr-1">↳ {card.parent} ·</span>{/if}{card.answer}</span>`.

- [ ] **Step 6: Check, build, commit**

Run: `cd frontend && npm run check 2>&1 | tail -2 && npm run build 2>&1 | grep -E "error|built in" | head -3; cd .. && git checkout -- frontend/build`
Expected: 0 errors (one pre-existing warning), build ok.

```bash
git add frontend/src/lib/components/AnswerSheet.svelte frontend/src/routes/pavlov/+page.svelte frontend/src/routes/practice/+page.svelte frontend/src/routes/settings/+page.svelte frontend/src/routes/pavlov/list/+page.svelte
git commit -m "feat(sheets): AnswerSheet component; Pavlov pauses on Wrong with the sheet (e/d keys); practice pause shows it; auto-facts setting; parent labels

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Dashboard activity strip, mock API, screenshots

**Files:**
- Create: `frontend/src/lib/components/ActivityStrip.svelte`
- Modify: `frontend/src/routes/dashboard/+page.svelte`
- Modify: `scripts/dev-mock-api.mjs`

**Interfaces:**
- Consumes: `GET /api/activity -> { streak, activeLast28, days: [{ date, active }] }`.

- [ ] **Step 1: `ActivityStrip.svelte`**

```svelte
<script lang="ts">
  export interface Activity {
    streak: number;
    activeLast28: number;
    days: Array<{ date: string; active: boolean }>;
  }
  let { activity }: { activity: Activity } = $props();

  // green ≥ 24 of 28, amber ≥ 16, red below (spec §4).
  let countClass = $derived(
    activity.activeLast28 >= 24 ? 'text-green-600' : activity.activeLast28 >= 16 ? 'text-amber-500' : 'text-red-500'
  );
  const dayLabel = (iso: string) => new Date(iso + 'T00:00:00').toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
</script>

<div class="bg-white rounded-xl shadow-sm px-5 py-3 mb-6 flex flex-wrap items-center gap-x-6 gap-y-2">
  <p class="text-sm text-gray-600">
    Streak <span class="text-xl font-bold text-jeopardy-blue">{activity.streak}</span>
    · <span class="text-xl font-bold {countClass}">{activity.activeLast28}</span> of last 28 days
  </p>
  <div class="flex gap-1 flex-wrap" aria-label="Active days, oldest to newest">
    {#each activity.days as d (d.date)}
      <span
        class="inline-block w-3 h-3 rounded-full {d.active ? 'bg-jeopardy-blue' : 'bg-gray-200'}"
        title="{dayLabel(d.date)}{d.active ? ' · active' : ''}"
      ></span>
    {/each}
  </div>
</div>
```

- [ ] **Step 2: Dashboard**

Script: `import ActivityStrip, { type Activity } from '$lib/components/ActivityStrip.svelte';`, state `let activity = $state<Activity | null>(null);`, and in `onMount` add `api.get('/api/activity').then((a) => (activity = a)).catch(() => (activity = null));`.

Markup: between the `<h1>Dashboard</h1>` header block and the button row insert

```svelte
    {#if activity}
      <ActivityStrip {activity} />
    {/if}
```

- [ ] **Step 3: Mock API routes**

In `scripts/dev-mock-api.mjs` add to `routes`:

```js
  '/api/activity': { streak: 3, activeLast28: 11, days: Array.from({ length: 28 }, (_, i) => ({ date: isoDay(i - 27), active: [5, 6, 8, 12, 13, 16, 19, 20, 25, 26, 27].includes(i) })) },
  '/api/pavlov/drill/next': { done: false, isNew: false, dueCount: 124, newRemaining: 40, card: { answerId: 1, answerNorm: 'wuthering heights', kind: 'answer', parent: null, category: 'Literature & Language', phrases: [{ text: 'Heathcliff', tier: 'standard' }, { text: 'Yorkshire moors', tier: 'hint' }] } },
  '/api/pavlov/drill/check': { correct: null, answer: 'Wuthering Heights', answerNorm: 'wuthering heights', kind: 'answer', parent: null, examples: [{ clue: 'Heathcliff seeks revenge in this Emily Brontë novel', category: 'NOVELS', airDate: '2019-03-04' }] },
  '/api/pavlov/drill/grade': { state: 'learning', due: new Date().toISOString(), intervalDays: 0, requeueInSession: true, factsAdded: 0 },
  '/api/pavlov/facts': { added: 4 },
  '/api/sheet/answer/wuthering%20heights': { answerNorm: 'wuthering heights', answer: 'Wuthering Heights', identity: "Emily Brontë's only novel (1847): Heathcliff and Catherine's doomed love on the Yorkshire moors.", facts: [{ prompt: 'author', response: 'Emily Brontë' }, { prompt: 'antihero', response: 'Heathcliff' }, { prompt: 'narrator', response: 'Nelly Dean' }, { prompt: 'year published', response: '1847' }], factsAdded: false },
```

and make the server tolerate `POST` (it already ignores method) — no change needed. Also, in the `MOCK_EMPTY` branch nothing changes.

- [ ] **Step 4: Screenshots**

Start `node scripts/dev-mock-api.mjs` and `cd frontend && VITE_API_PROXY=http://127.0.0.1:3999 npm run dev` in the background. With the Playwright MCP tools (`browser_navigate`, `browser_resize`, `browser_click`/`browser_press_key`, `browser_take_screenshot` with a `filename`; size the viewport to the page's `scrollHeight` rather than `fullPage`, which truncates canvases):

1. `/dashboard` at 1280 and 390 wide → strip shows "Streak 3 · 11 of last 28 days" in red, 28 dots, above the button row; wraps cleanly on the phone.
2. `/pavlov`: press Space (reveal), press `1` (Wrong) → the card stays, the answer sheet renders with four `prompt → response` lines and a gold "Drill these 4 (d)" button, then "Next →". Press `d` → "4 fact cards added". Press Space → next card loads.
3. `/pavlov` again: reveal, press `e` → sheet toggles open under the grade buttons without pausing.

Save as `.playwright-mcp/sheets-dashboard.png`, `sheets-pavlov-pause.png`, `sheets-pavlov-e.png`. Stop both processes; `git checkout -- frontend/build`; do not commit PNGs.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/components/ActivityStrip.svelte frontend/src/routes/dashboard/+page.svelte scripts/dev-mock-api.mjs
git commit -m "feat(dashboard): active-days strip (streak, last 28); mock routes for sheets/activity/drill

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Rollout

Deploy is push-to-`main` → GitHub Actions → Watchtower (polls every 5 min). HTTPS push is refused on this machine (wrong GitHub account); push over SSH. The migration MUST be applied before the push.

- [ ] **Step 1: Apply migration 0015**

```bash
scripts/apply-migration.sh backend/migrations/0015_answer_sheets.sql
```
Expected: `CREATE TABLE`, `ALTER TABLE`, `CREATE INDEX`, `ALTER TABLE`.

- [ ] **Step 2: Verify script**

```bash
DB_URL=$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')
docker run --rm -i postgres:16 psql "$DB_URL" -P pager=off -f - < scripts/verify-sheets.sql
```
Expected: every `fail_rows` 0; section D returns one fact row and `verify_card_due_now` 0 before the ROLLBACK; section E lists 28 rows; section F > 4,000.

- [ ] **Step 3: Merge and push**

```bash
git checkout main && git merge --ff-only <branch> && git push git@github.com:ebertx/jeopardy-training-app.git main
```

Watch: `gh run list -R ebertx/jeopardy-training-app --limit 1`, then `tower-ssh 'docker inspect jeopardy-server --format "{{.Image}} {{.State.StartedAt}}"'` until the image changes; `curl -s -o /dev/null -w '%{http_code}' http://tower.tail11628.ts.net:3000/api/health` → 200; `/api/activity` unauthenticated → 401.

- [ ] **Step 4: Live smoke (needs Christian logged in)**

1. Dashboard shows the activity strip with real numbers.
2. Pavlov: rate one real answer card Wrong → the sheet generates (first time takes a few seconds) and the card pauses; "Drill these 4" adds four due-now cards (`SELECT count(*) FROM pavlov_answers WHERE kind='fact'` = 4).
3. Settings: toggle auto-add on; next Wrong shows "4 fact cards added" automatically.
4. Practice: miss a clue → teaching pause shows insight then the sheet.

- [ ] **Step 5: Memory**

Append an `UPDATE` line to `~/.claude/projects/-Users-atropos-ai-jeopardy-jeopardy-training-app/memory/project_jeopardy_test_prep.md`: sheets + fact cards + activity deployed (commit), migration 0015 applied, keys (e/d), auto-facts setting, known deferrals (canon library, bulk pregeneration).

---

## Self-review

**Spec coverage.** §1 data → Task 1 (migration) + Task 3 (fact-row shape, all four `kind='answer'` filters: pick_new_card ×3, more_new_available, stale cleanup, deckTotal/touched). §2 generation → Tasks 1–2 (prompt rules, 15 evenly-sampled clues, display answer fallback, rejection never cached, single-flight, prefetch from drill_next). §3 API → Task 2 (sheet endpoints, 204), Task 3 (facts endpoint, drill payload kind/parent/answerNorm, drill_grade auto-add + factsAdded, preferences), Task 4 (activity incl. pre-log history and streak rule). §4 UI → Task 5 (Pavlov pause/e/d/parent label, practice pause, settings, list) + Task 6 (dashboard strip with thresholds, 28 dots today-rightmost). §5 testing → Task 1 (9 pure tests), Task 4 (5), Task 3 verify script incl. rolled-back upsert and facts-excluded check, Task 6 screenshots. §6 rollout → Task 7.

**Type consistency.** `Sheet` (frontend) = `{ answerNorm, answer, identity, facts[{prompt,response}], factsAdded }` = the JSON `routes/sheet.rs::respond` emits. `drill_card_json` emits `answerNorm/kind/parent`, matched by the Pavlov page's `card` type; `drill_check` emits the same three, matched by `result`. `add_fact_cards` returns `Option<i64>`; `drill_grade` unwraps to `factsAdded: i64`; the page reads `res.factsAdded`. `activity` JSON `{ streak, activeLast28, days }` = `Activity` interface. `Rating::Wrong` comparison requires `PartialEq` (Task 3 Step 3 notes the derive).

**Placeholders.** None: every code step carries its content; the one "check whether the derive exists" instruction names the file, line, and the exact edit.
