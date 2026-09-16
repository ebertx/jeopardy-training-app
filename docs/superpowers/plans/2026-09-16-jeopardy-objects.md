# Jeopardy Objects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Pavlov card an entity with corpus-mined hooks, merge name-variant answers into one entity, import the community-vetted JBoard Pavlov pairs, and remove the answer-sheet / fact-card feature.

**Architecture:** Two pure Rust modules carry the logic (`entity.rs` resolves response strings into entities; `hooks.rs` clusters an entity's clues into hooks, picks the hook to drill, caps intervals for coverage, and builds/validates label prompts). One async module (`objects.rs`) runs the two admin jobs (resolve, hooks) against Postgres and OpenAI. The drill routes serve one hook cue per review and return the hook map on reveal; a shared `HookMap.svelte` renders it in the drill, the practice pause, and the list page.

**Tech Stack:** Rust / Axum 0.8 / sqlx 0.8 runtime queries (SQL is not compile-checked — every query is exercised by the verify script or by hand) / chrono / reqwest → OpenAI JSON mode (`openai::chat_json`) / PostgreSQL 15 / SvelteKit adapter-static + Svelte 5 runes / Tailwind 4.

**Spec:** `docs/superpowers/specs/2026-09-16-jeopardy-objects-design.md`

**Spec amendment recorded by this plan (approved via the plan):** the seed-gram distinctiveness caps are `HOOK_UNIGRAM_MAX_DF = 3000` and `HOOK_BIGRAM_MAX_DF = 300` (spec §2 said 500 / 100). A dry run of the clustering on the live Visigoths grams showed "spain" has corpus frequency 1,254 and would have been excluded, losing the angle the mock clue used. Clue assignment weights grams by inverse document frequency, so common-but-relevant grams like "spain" contribute less than "711" without being dropped.

## Global Constraints

- Migration file is `backend/migrations/0016_jeopardy_objects.sql`; migrations are applied by hand with `scripts/apply-migration.sh` and NEVER by the container.
- `pavlov_answers` remains the entity table; `answer_norm` is the entity key; `forms TEXT[]` holds merged response strings; `vetted BOOLEAN` marks JBoard-named entities.
- `pavlov_hooks` identity is `UNIQUE (answer_id, key_gram)`; `cue IS NULL` means unlabeled (never drilled, never shown on the map); `status IN ('active','dropped')`; `source IN ('mined','vetted','cue','model','both')`.
- `pavlov_reviews.hook_id` is the only exposure record; per-hook "seen" and "last wrong" are derived from it.
- Hook selection order: fewest exposures → last rating was wrong → rank. Coverage cap: while any labeled active hook is unseen by the user, the entity's interval is capped at `HOOK_COVERAGE_CAP_DAYS = 7.0`.
- Clustering constants: `HOOK_MIN_SUPPORT = 2`, `HOOK_MAX_PER_ENTITY = 8`, `HOOK_UNIGRAM_MAX_DF = 3000`, `HOOK_BIGRAM_MAX_DF = 300`, `HOOK_MERGE_JACCARD = 0.5`.
- Label sources in priority order: vetted (JBoard cue verbatim, `source='both'`), existing active standard v2 cue whose `cue_stem` is among the cluster grams (`source='cue'`), model (`HOOK_LABEL_MODEL = "gpt-4o-mini"`, batches of `HOOK_LABEL_BATCH = 20`, `source='model'`).
- Label gates: non-empty, ≤ 8 words, `pavlov::phrase_leaks_answer` false, `hooks::label_grounded` true, no hedge words (`possibly`, `perhaps`, `often`, `maybe`, `sometimes`).
- Vetted data file: `backend/data/pavlovs-jboard.tsv`, header `cue\tresponse\tdomain\tsource_url`, compiled in with `include_str!`.
- New-card order within a sampled category: `vetted DESC, answer_freq DESC, score DESC, id`.
- Deck progress, pace, and allowance count entities (rows of `pavlov_answers`), nothing else.
- The drill falls back to legacy `phrases` for any entity with no labeled active hook; the fallback review has `hook_id = NULL`.
- Answer sheets, fact cards, `users.pavlov_auto_facts`, the `e`/`d` bindings, and the paused-on-Wrong state are removed entirely.
- Frontend verification: `cd frontend && npm run check` (0 errors; one pre-existing CountdownTimer warning is acceptable) and `npm run build`; `frontend/build/` is a stale tracked directory that the build dirties — run `git checkout -- frontend/build` before committing.
- Backend verification: `cd backend && cargo test` (all green) and `cargo build`.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## File structure

| File | Responsibility |
|---|---|
| `backend/migrations/0016_jeopardy_objects.sql` | new columns/tables, removals |
| `backend/data/pavlovs-jboard.tsv` | vetted pairs (already generated, untracked — Task 1 commits it) |
| `backend/src/entity.rs` (new, pure) | response normalization, parenthetical license, `resolve()` |
| `backend/src/hooks.rs` (new, pure) | `cluster_hooks`, `pick_hook`, `cap_for_coverage`, `vetted_matches`, label prompt/parse/gates |
| `backend/src/objects.rs` (new, async) | `run_resolve`, `run_hooks` (vetted import inside), `refresh_entity_rows` |
| `backend/src/pavlov.rs` | pipeline keys on `entity_norm`; `norm_tokens` becomes `pub(crate)` |
| `backend/src/routes/pavlov.rs` | drill next/check/grade with hooks; hook drop/restore; answers list with hooks; entity-by-question; admin resolve/hooks |
| `backend/src/routes/pavlov_stats.rs`, `backend/src/pavlov_stats.rs` | hook coverage in progress |
| `backend/src/routes/preferences.rs`, `backend/src/main.rs`, `backend/src/routes/mod.rs` | removals + new routes |
| deleted: `backend/src/sheets.rs`, `backend/src/routes/sheet.rs`, `backend/src/routes/pavlov_facts.rs`, `scripts/verify-sheets.sql`, `frontend/src/lib/components/AnswerSheet.svelte` | |
| `scripts/verify-objects.sql` (new) | read-only checks for rollout |
| `frontend/src/lib/components/HookMap.svelte` (new) | shared hook map |
| `frontend/src/routes/pavlov/+page.svelte`, `frontend/src/routes/pavlov/list/+page.svelte`, `frontend/src/routes/practice/+page.svelte`, `frontend/src/routes/settings/+page.svelte`, `frontend/src/lib/components/PavlovProgressCard.svelte`, `frontend/src/lib/stats.ts`, `scripts/dev-mock-api.mjs` | UI |

Task order matters: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12. Tasks 2, 3, 4 are pure and could run concurrently with each other after Task 1; Tasks 10, 11, 12 are frontend-only and could run concurrently with each other after Task 9.

---

### Task 1: Migration 0016, vetted data file, and backend removal of sheets / fact cards

**Files:**
- Create: `backend/migrations/0016_jeopardy_objects.sql`
- Commit (already exists, untracked): `backend/data/pavlovs-jboard.tsv`
- Delete: `backend/src/sheets.rs`, `backend/src/routes/sheet.rs`, `backend/src/routes/pavlov_facts.rs`, `scripts/verify-sheets.sql`
- Modify: `backend/src/main.rs`, `backend/src/routes/mod.rs`, `backend/src/routes/pavlov.rs`, `backend/src/routes/pavlov_stats.rs`, `backend/src/routes/preferences.rs`, `backend/src/pavlov.rs:684-694`

**Interfaces:**
- Consumes: nothing.
- Produces: the schema every later task assumes; `routes/pavlov.rs` with `DrillAnswerRow { id, answer_norm, phrases, phrase_tiers, meta_category }` and no sheet/fact code; `AppState` without `sheet_inflight` / `sheet_failed`.

- [ ] **Step 1: Write the migration**

```sql
-- backend/migrations/0016_jeopardy_objects.sql
-- Jeopardy objects: entities, hooks, vetted import; removes answer sheets / fact cards.
-- Spec: docs/superpowers/specs/2026-09-16-jeopardy-objects-design.md

ALTER TABLE jeopardy_questions ADD COLUMN IF NOT EXISTS entity_norm TEXT;
CREATE INDEX IF NOT EXISTS idx_jq_entity_norm ON jeopardy_questions (entity_norm);

ALTER TABLE pavlov_answers
  ADD COLUMN IF NOT EXISTS forms  TEXT[]  NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS vetted BOOLEAN NOT NULL DEFAULT false,
  ADD COLUMN IF NOT EXISTS hooks_built_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS pavlov_hooks (
  id         SERIAL PRIMARY KEY,
  answer_id  INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
  key_gram   TEXT    NOT NULL,
  rank       INTEGER NOT NULL DEFAULT 1,
  cue        TEXT,
  grams      TEXT[]    NOT NULL DEFAULT '{}',
  clue_ids   INTEGER[] NOT NULL DEFAULT '{}',
  support    INTEGER NOT NULL DEFAULT 0,
  source     TEXT NOT NULL DEFAULT 'mined' CHECK (source IN ('mined','vetted','cue','model','both')),
  status     TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','dropped')),
  model      TEXT NOT NULL DEFAULT '',
  label_attempted_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (answer_id, key_gram)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_hooks_answer ON pavlov_hooks (answer_id, status);

ALTER TABLE pavlov_reviews
  ADD COLUMN IF NOT EXISTS hook_id INTEGER REFERENCES pavlov_hooks(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_pavlov_reviews_hook ON pavlov_reviews (user_id, answer_id, hook_id);

-- Removals: answer sheets / fact cards (spec §5). Fact rows cascade to cards + reviews.
DELETE FROM pavlov_answers WHERE kind = 'fact';
DROP TABLE IF EXISTS answer_sheets;
DROP INDEX IF EXISTS idx_pavlov_answers_kind_parent;
ALTER TABLE pavlov_answers DROP COLUMN IF EXISTS kind, DROP COLUMN IF EXISTS parent_norm;
ALTER TABLE users DROP COLUMN IF EXISTS pavlov_auto_facts;
```

- [ ] **Step 2: Delete the sheet / fact modules and script**

```bash
git rm backend/src/sheets.rs backend/src/routes/sheet.rs backend/src/routes/pavlov_facts.rs scripts/verify-sheets.sql
```

- [ ] **Step 3: Remove the module and route registrations**

In `backend/src/main.rs`: delete the line `mod sheets;`; delete the two `.route("/api/sheet/...` lines and the `.route("/api/pavlov/facts", ...)` line; delete the `sheet_inflight` and `sheet_failed` fields from `AppState` (both the struct and the `Arc::new(AppState { ... })` initializer, including the doc comment above `sheet_failed`).

In `backend/src/routes/mod.rs`: delete `pub mod pavlov_facts;` and `pub mod sheet;`.

- [ ] **Step 4: Strip sheet / fact code from `backend/src/routes/pavlov.rs`**

Delete the `PARENT_ANSWER_SQL` const and its doc comment. Replace `AnswerListRow`, the `answers` SELECT, and the per-row JSON so no `kind` / `parent` remain:

```rust
#[derive(sqlx::FromRow)]
struct AnswerListRow {
    id: i32,
    answer: String,
    answer_norm: String,
    meta_category: String,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    score: f32,
    suspended: bool,
}
```

```rust
    let mut rows: Vec<AnswerListRow> = sqlx::query_as(
        "SELECT pa.id, pa.answer, pa.answer_norm, pa.meta_category, pa.phrases,
                pa.phrase_tiers, pa.score,
                COALESCE(ca.suspended, false) AS suspended
         FROM pavlov_answers pa
         LEFT JOIN pavlov_cards ca ON ca.answer_id = pa.id AND ca.user_id = $1",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;
```

and in the `json!` for each answer drop `"kind": r.kind, "parent": r.parent,`.

Replace `DrillAnswerRow`, `drill_card_json`, and `drill_cols`:

```rust
#[derive(sqlx::FromRow)]
struct DrillAnswerRow {
    id: i32,
    answer_norm: String,
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
        "answerId": r.id, "answerNorm": r.answer_norm,
        "phrases": phrases, "category": r.meta_category,
    })
}

/// The SELECT list every drill query shares.
fn drill_cols() -> &'static str {
    "pa.id, pa.answer_norm, pa.phrases, pa.phrase_tiers, pa.meta_category"
}
```

In `pick_new_card` remove `WHERE kind = 'answer'` / `AND pa.kind = 'answer'` from all three queries (keep the `id NOT IN (...)` predicates). In `drill_next` remove the three `if row.kind == "answer" { crate::sheets::pregenerate_sheet(...) }` blocks, remove `AND pa.kind = 'answer'` from `new_today` and from `more_new_available`. In `drill_check` replace the query and tuple:

```rust
    let row: Option<(String, Vec<i32>, String)> = sqlx::query_as(
        "SELECT pa.answer, pa.example_clue_ids, pa.answer_norm FROM pavlov_answers pa WHERE pa.id = $1",
    )
    .bind(body.answer_id)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, example_ids, answer_norm) = row.ok_or_else(|| AppError::NotFound("No such cue".into()))?;
```

and the response becomes `json!({ "correct": correct, "answer": answer, "answerNorm": answer_norm, "examples": examples })`. Delete `auto_add_facts` entirely; in `drill_grade` delete the `facts_added` block after `tx.commit()` and the `"factsAdded"` field.

- [ ] **Step 5: Strip `kind` from stats and preferences**

`backend/src/routes/pavlov_stats.rs`: remove `AND pa.kind = 'answer'` from `new_today`, `touched`, `created_14d`; change `deck_total` to `"SELECT COUNT(*) FROM pavlov_answers"`. Remove the comment line "Deck coverage counts real answers only; fact cards are depth, not progress."

`backend/src/routes/preferences.rs`: drop `pavlov_auto_facts` from the SELECT (tuple becomes 6 elements), drop `"pavlovAutoFacts": row.6,`, drop the `pavlov_auto_facts` field from `UpdatePreferencesBody`, and delete the `if let Some(b) = body.pavlov_auto_facts { ... }` block.

`backend/src/pavlov.rs` `assemble_stage`: change the stale-delete to `"DELETE FROM pavlov_answers WHERE answer_norm NOT IN (SELECT DISTINCT answer_norm FROM pavlov_cues WHERE status = 'active' AND tier = 'standard')"` (Task 5 adds the `vetted` guard).

- [ ] **Step 6: Build and test**

Run: `cd backend && cargo build && cargo test`
Expected: builds; all tests pass (the deleted sheets tests are gone; `is_first_grade` tests remain).

- [ ] **Step 7: Commit**

```bash
git add backend/migrations/0016_jeopardy_objects.sql backend/data/pavlovs-jboard.tsv backend/src scripts
git commit -m "feat(objects): migration 0016, vetted JBoard data file; remove answer sheets and fact cards from the backend

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: `entity.rs` — response normalization and entity resolution (pure)

**Files:**
- Create: `backend/src/entity.rs`
- Modify: `backend/src/main.rs` (add `mod entity;`)

**Interfaces:**
- Produces:
  - `pub fn norm_response(s: &str) -> String` — Rust twin of the SQL `lower(trim(regexp_replace(x, '^(the|a|an) ', '', 'i')))`.
  - `pub fn strip_parens(s: &str) -> String` — removes `(...)` and `"..."` segments, collapses whitespace.
  - `pub fn strip_honorific(s: &str) -> String` — removes one leading `Sir ` / `Dame ` (case-insensitive).
  - `pub fn entity_key(raw: &str) -> String` — `norm_response(strip_honorific(strip_parens(raw)))`.
  - `pub fn parenthetical_license(raw: &str) -> Option<(String, String)>` — `"(Edvard) Grieg"` → `Some(("edvard", "grieg"))`.
  - `pub struct Form { pub raw: String, pub count: i64 }`, `pub struct Entity { pub key: String, pub display: String, pub forms: Vec<String>, pub freq: i64 }`.
  - `pub fn resolve(forms: &[Form]) -> Vec<Entity>` — sorted by key.

- [ ] **Step 1: Write the failing tests**

```rust
// bottom of backend/src/entity.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn f(raw: &str, count: i64) -> Form { Form { raw: raw.to_string(), count } }
    fn by_key<'a>(ents: &'a [Entity], key: &str) -> &'a Entity {
        ents.iter().find(|e| e.key == key).unwrap_or_else(|| panic!("no entity {key}"))
    }

    #[test]
    fn norm_response_strips_one_article_and_lowercases() {
        assert_eq!(norm_response("The Visigoths"), "visigoths");
        assert_eq!(norm_response("a cage"), "cage");
        assert_eq!(norm_response("  Grieg "), "grieg");
        assert_eq!(norm_response("Theodore Roosevelt"), "theodore roosevelt"); // 'the' only as a word
    }

    #[test]
    fn strip_parens_removes_parentheticals_and_quoted_nicknames() {
        assert_eq!(strip_parens("(Edvard) Grieg"), "Grieg");
        assert_eq!(strip_parens("Edvard Munch (1863-1944)"), "Edvard Munch");
        assert_eq!(strip_parens("jean \"finlandia\" sibelius"), "jean sibelius");
        assert_eq!(strip_parens("Grieg"), "Grieg");
    }

    #[test]
    fn strip_honorific_removes_sir_and_dame_only() {
        assert_eq!(strip_honorific("Sir Edward Elgar"), "Edward Elgar");
        assert_eq!(strip_honorific("dame Judi Dench"), "Judi Dench");
        assert_eq!(strip_honorific("Saint Peter"), "Saint Peter");
        assert_eq!(strip_honorific("Sirius"), "Sirius");
    }

    #[test]
    fn entity_key_composes_the_three_steps() {
        assert_eq!(entity_key("(Sir Edward) Elgar"), "elgar");
        assert_eq!(entity_key("Sir Edward Elgar"), "edward elgar");
        assert_eq!(entity_key("the Visigoths"), "visigoths");
        assert_eq!(entity_key("(the) Visigoths"), "visigoths");
    }

    #[test]
    fn parenthetical_license_reads_first_and_last() {
        assert_eq!(parenthetical_license("(Edvard) Grieg"), Some(("edvard".into(), "grieg".into())));
        assert_eq!(parenthetical_license("(Ralph Waldo) Emerson"), Some(("ralph waldo".into(), "emerson".into())));
        assert_eq!(parenthetical_license("(Sir Edward) Elgar"), Some(("edward".into(), "elgar".into())));
        assert_eq!(parenthetical_license("Edvard Grieg"), None);
        assert_eq!(parenthetical_license("Edvard Munch (1863-1944)"), None);
        assert_eq!(parenthetical_license("(1 of) Balakirev, Borodin"), None); // last part has a comma: not a name
    }

    #[test]
    fn grieg_forms_merge_into_one_entity() {
        let ents = resolve(&[f("(Edvard) Grieg", 19), f("Edvard Grieg", 19), f("Grieg", 11), f("Edward Grieg", 2)]);
        let g = by_key(&ents, "grieg");
        assert_eq!(g.display, "Edvard Grieg");
        assert_eq!(g.freq, 49);
        assert_eq!(g.forms, vec!["(Edvard) Grieg", "Edvard Grieg", "Grieg"]);
        // misspelled first name is NOT licensed
        assert_eq!(by_key(&ents, "edward grieg").freq, 2);
        assert_eq!(ents.len(), 2);
    }

    #[test]
    fn honorific_forms_merge_under_the_licensed_surname() {
        let ents = resolve(&[f("(Sir Edward) Elgar", 10), f("Sir Edward Elgar", 5), f("Edward Elgar", 8), f("Elgar", 5)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "elgar");
        assert_eq!(ents[0].freq, 28);
        assert_eq!(ents[0].display, "Sir Edward Elgar");
    }

    #[test]
    fn multiword_first_names_are_licensed_as_a_unit() {
        let ents = resolve(&[f("(Ralph Waldo) Emerson", 30), f("Ralph Waldo Emerson", 40), f("Emerson", 12)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].display, "Ralph Waldo Emerson");
        assert_eq!(ents[0].freq, 82);
    }

    #[test]
    fn unlicensed_compounds_stay_separate() {
        let ents = resolve(&[
            f("Mexico", 387), f("New Mexico", 179),
            f("London", 296), f("Jack London", 70),
            f("Washington", 223), f("Denzel Washington", 32),
        ]);
        assert_eq!(ents.len(), 6);
        assert_eq!(by_key(&ents, "new mexico").freq, 179);
        assert_eq!(by_key(&ents, "jack london").freq, 70);
        assert_eq!(by_key(&ents, "denzel washington").freq, 32);
    }

    #[test]
    fn different_first_name_variants_do_not_merge() {
        let ents = resolve(&[f("(Claude) Debussy", 20), f("Claude Debussy", 20), f("Claude-Achille Debussy", 1)]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "debussy").freq, 40);
        assert_eq!(by_key(&ents, "claude-achille debussy").freq, 1);
    }

    #[test]
    fn display_prefers_the_fullest_then_most_frequent_form() {
        let ents = resolve(&[f("(Ludwig van) Beethoven", 60), f("Beethoven", 150), f("Ludwig van Beethoven", 20)]);
        assert_eq!(ents[0].display, "Ludwig van Beethoven");
        assert_eq!(ents[0].forms[0], "Beethoven"); // forms sorted by count desc
    }

    #[test]
    fn output_is_sorted_by_key() {
        let ents = resolve(&[f("Zola", 3), f("Austen", 4)]);
        assert_eq!(ents.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), vec!["austen", "zola"]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test entity::`
Expected: compile error — module `entity` does not exist.

- [ ] **Step 3: Implement**

```rust
//! Entity resolution: which response strings are the same Jeopardy! answer.
//! Spec: docs/superpowers/specs/2026-09-16-jeopardy-objects-design.md §1.
//! Pure — no DB. `objects::run_resolve` feeds it the corpus response forms.

use std::collections::{BTreeMap, HashMap, HashSet};

/// Rust twin of the SQL `lower(trim(regexp_replace(x, '^(the|a|an) ', '', 'i')))`:
/// strips ONE leading article (followed by a space), lowercases, trims.
pub fn norm_response(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    let stripped = ["the ", "an ", "a "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    stripped.trim().to_string()
}

/// Remove `(...)` and `"..."` segments, collapse runs of whitespace.
pub fn strip_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '(' if !in_quote => depth += 1,
            ')' if !in_quote && depth > 0 => depth -= 1,
            '"' if depth == 0 => in_quote = !in_quote,
            _ if depth == 0 && !in_quote => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip one leading honorific `Sir ` / `Dame ` (case-insensitive).
pub fn strip_honorific(s: &str) -> String {
    let t = s.trim();
    for h in ["sir ", "dame "] {
        if let Some(prefix) = t.get(..h.len()) {
            if t.len() > h.len() && prefix.eq_ignore_ascii_case(h) {
                return t[h.len()..].trim().to_string();
            }
        }
    }
    t.to_string()
}

/// The entity key of a raw response string.
pub fn entity_key(raw: &str) -> String {
    norm_response(&strip_honorific(&strip_parens(raw)))
}

/// `"(First) Last"` → `Some((first_lower, last_key))`. The parenthetical must
/// open the string; the remainder must be a single-word name (no commas, no
/// digits, no spaces after normalization). Honorifics inside the parens are
/// dropped so `"(Sir Edward) Elgar"` licenses `edward`.
pub fn parenthetical_license(raw: &str) -> Option<(String, String)> {
    let t = raw.trim();
    if !t.starts_with('(') {
        return None;
    }
    let close = t.find(')')?;
    let first = strip_honorific(t[1..close].trim()).to_lowercase();
    let last = norm_response(t[close + 1..].trim());
    if first.is_empty() || last.is_empty() || last.contains(' ') || last.contains(',') {
        return None;
    }
    if first.chars().any(|c| c.is_ascii_digit()) || last.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !first.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '.' || c == '\'') {
        return None;
    }
    Some((first, last))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub raw: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub key: String,
    pub display: String,
    pub forms: Vec<String>,
    pub freq: i64,
}

/// Group response forms into entities (spec §1 rules):
/// 1. key = entity_key(raw);
/// 2. a multi-token key `first last` collapses to `last` only when some form
///    `"(first) last"` exists in the input (the parenthetical license);
/// 3. display = the form with the most tokens (parentheticals stripped), ties
///    by count; forms = raw strings by count desc; freq = sum of counts.
pub fn resolve(forms: &[Form]) -> Vec<Entity> {
    let mut licenses: HashMap<String, HashSet<String>> = HashMap::new();
    for f in forms {
        if let Some((first, last)) = parenthetical_license(&f.raw) {
            licenses.entry(last).or_default().insert(first);
        }
    }

    let mut groups: BTreeMap<String, Vec<&Form>> = BTreeMap::new();
    for f in forms {
        let base = entity_key(&f.raw);
        if base.is_empty() {
            continue;
        }
        let key = match base.rsplit_once(' ') {
            Some((first, last)) if licenses.get(last).is_some_and(|s| s.contains(first)) => {
                last.to_string()
            }
            _ => base,
        };
        groups.entry(key).or_default().push(f);
    }

    groups
        .into_iter()
        .map(|(key, mut members)| {
            members.sort_by(|a, b| b.count.cmp(&a.count).then(a.raw.cmp(&b.raw)));
            let display = members
                .iter()
                .map(|m| (strip_parens(&m.raw), m.count))
                .filter(|(d, _)| !d.is_empty())
                .max_by(|(da, ca), (db, cb)| {
                    da.split_whitespace().count()
                        .cmp(&db.split_whitespace().count())
                        .then(ca.cmp(cb))
                        .then(db.cmp(da))
                })
                .map(|(d, _)| d)
                .unwrap_or_else(|| key.clone());
            Entity {
                freq: members.iter().map(|m| m.count).sum(),
                forms: members.iter().map(|m| m.raw.clone()).collect(),
                display,
                key,
            }
        })
        .collect()
}
```

Add `mod entity;` to `backend/src/main.rs` next to `mod pavlov;`.

- [ ] **Step 4: Run the tests**

Run: `cd backend && cargo test entity::`
Expected: 12 passed. If `display_prefers_the_fullest_then_most_frequent_form` fails on `forms[0]`, check the sort in `members.sort_by` (count desc) — `Beethoven` (150) must come first.

- [ ] **Step 5: Commit**

```bash
git add backend/src/entity.rs backend/src/main.rs
git commit -m "feat(objects): pure entity resolution with the parenthetical-license merge rule

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 3: `hooks.rs` — clustering an entity's clues into hooks (pure)

**Files:**
- Create: `backend/src/hooks.rs`
- Modify: `backend/src/main.rs` (add `mod hooks;`)

**Interfaces:**
- Produces:
  - constants `HOOK_MIN_SUPPORT: i64 = 2`, `HOOK_MAX_PER_ENTITY: usize = 8`, `HOOK_UNIGRAM_MAX_DF: i64 = 3000`, `HOOK_BIGRAM_MAX_DF: i64 = 300`, `HOOK_MERGE_JACCARD: f64 = 0.5`.
  - `pub struct GramStat { pub gram: String, pub n: i16, pub support: i64, pub corpus_df: i64, pub clue_ids: Vec<i32> }`
  - `pub struct Cluster { pub key_gram: String, pub grams: Vec<String>, pub clue_ids: Vec<i32>, pub support: usize }`
  - `pub fn idf(corpus_clues: i64, df: i64) -> f64`, `pub fn is_seed(g: &GramStat) -> bool`
  - `pub fn cluster_hooks(grams: &[GramStat], corpus_clues: i64) -> Vec<Cluster>` — ranked (index 0 = rank 1), at most 8.

- [ ] **Step 1: Write the failing tests**

The fixture is the real Visigoths gram table from the live n-gram corpus (clue ids are real; `peopl` and `king` are the stop-like grams that must be excluded).

```rust
// bottom of backend/src/hooks.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn g(gram: &str, n: i16, corpus_df: i64, clues: &[i32]) -> GramStat {
        GramStat { gram: gram.into(), n, support: clues.len() as i64, corpus_df, clue_ids: clues.to_vec() }
    }

    fn visigoths() -> Vec<GramStat> {
        vec![
            g("alar", 1, 18, &[192216, 201593, 246946, 252909, 296899, 421463, 503952, 528821]),
            g("sack", 1, 209, &[201593, 241725, 252909, 296899, 421463, 503952, 528821]),
            g("rome", 1, 1070, &[201593, 241725, 252909, 296899, 421463, 503952, 528821]),
            g("sack rome", 2, 23, &[201593, 241725, 252909, 503952]),
            g("410", 1, 15, &[252909, 421463, 503952]),
            g("ostrogoth", 1, 9, &[31327, 44239, 105561, 186092, 411040, 479520, 507846]),
            g("goth split", 2, 5, &[31327, 44239, 105561]),
            g("western goth", 2, 2, &[105561, 186092]),
            g("spain", 1, 1254, &[186092, 241725, 249894, 490093, 496405]),
            g("711", 1, 15, &[186092, 249894, 496405]),
            g("peopl", 1, 6484, &[31327, 201593, 241725, 249894, 411040, 490093, 496405, 507846]),
            g("king", 1, 7762, &[192216, 246946, 249894, 490093, 496405]),
        ]
    }

    #[test]
    fn idf_is_log_ratio_floored_at_zero() {
        assert!((idf(530_000, 18) - 10.29).abs() < 0.01);
        assert_eq!(idf(100, 1000), 0.0);
        assert_eq!(idf(0, 0), 0.0);
    }

    #[test]
    fn seeds_need_support_and_distinctiveness() {
        assert!(is_seed(&g("spain", 1, 1254, &[1, 2])));
        assert!(!is_seed(&g("peopl", 1, 6484, &[1, 2, 3])));
        assert!(!is_seed(&g("4th centuri", 2, 301, &[1, 2])));
        assert!(is_seed(&g("4th centuri", 2, 300, &[1, 2])));
        assert!(!is_seed(&g("toledo", 1, 40, &[1])));
    }

    #[test]
    fn visigoths_cluster_into_three_angles_in_support_order() {
        let out = cluster_hooks(&visigoths(), 530_000);
        let keys: Vec<&str> = out.iter().map(|c| c.key_gram.as_str()).collect();
        assert_eq!(keys, vec!["alar", "ostrogoth", "711"]);
        assert_eq!(out.iter().map(|c| c.support).collect::<Vec<_>>(), vec![9, 7, 3]);

        assert_eq!(out[0].grams, vec!["410", "alar", "rome", "sack", "sack rome"]);
        assert!(out[0].clue_ids.contains(&241725), "sack-of-Rome clue that also mentions Spain goes to Alaric");
        assert!(out[0].clue_ids.contains(&192216), "Alaric II clue goes to Alaric");

        assert_eq!(out[1].grams, vec!["goth split", "ostrogoth", "western goth"]);
        assert!(out[1].clue_ids.contains(&186092), "western-Goths-ruled-Spain clue weighs Ostrogoth grams higher");

        assert_eq!(out[2].grams, vec!["711", "spain"]);
        assert_eq!(out[2].clue_ids, vec![249894, 490093, 496405]);
    }

    #[test]
    fn clue_lists_are_sorted_and_disjoint() {
        let out = cluster_hooks(&visigoths(), 530_000);
        let mut all: Vec<i32> = out.iter().flat_map(|c| c.clue_ids.iter().copied()).collect();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n);
        for c in &out {
            let mut s = c.clue_ids.clone();
            s.sort();
            assert_eq!(s, c.clue_ids);
        }
    }

    #[test]
    fn clusters_are_capped_at_eight() {
        let grams: Vec<GramStat> = (0..12)
            .map(|i| g(&format!("g{i:02}"), 1, 10, &[i * 10, i * 10 + 1]))
            .collect();
        let out = cluster_hooks(&grams, 530_000);
        assert_eq!(out.len(), HOOK_MAX_PER_ENTITY);
        assert_eq!(out[0].key_gram, "g00"); // equal support: key_gram ascending
    }

    #[test]
    fn a_cluster_whose_clues_are_claimed_elsewhere_is_dropped() {
        // B = {4,5} does not merge with A = {1,2,3,4} (Jaccard 1/5); clue 4 is
        // assigned to A (higher idf) so B keeps only clue 5 and falls below
        // HOOK_MIN_SUPPORT.
        let out = cluster_hooks(&[g("a", 1, 10, &[1, 2, 3, 4]), g("b", 1, 2000, &[4, 5])], 530_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key_gram, "a");
        assert_eq!(out[0].clue_ids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn identical_clue_sets_merge_and_containment_merges() {
        let out = cluster_hooks(
            &[g("x", 1, 100, &[1, 2, 3]), g("x y", 2, 50, &[1, 2, 3]), g("z", 1, 100, &[2, 3])],
            530_000,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].grams, vec!["x", "x y", "z"]);
        assert_eq!(out[0].key_gram, "x y"); // 3 × idf(50) beats 3 × idf(100)
    }

    #[test]
    fn empty_input_yields_no_hooks() {
        assert!(cluster_hooks(&[], 530_000).is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test hooks::`
Expected: compile error — module `hooks` does not exist.

- [ ] **Step 3: Implement**

```rust
//! Hooks: an entity's clue angles, mined from its own clues (spec §2), plus
//! the pure drill helpers (spec §3). No DB here — `objects.rs` feeds it.

use std::collections::{BTreeMap, BTreeSet};

pub const HOOK_MIN_SUPPORT: i64 = 2;
pub const HOOK_MAX_PER_ENTITY: usize = 8;
pub const HOOK_UNIGRAM_MAX_DF: i64 = 3000;
pub const HOOK_BIGRAM_MAX_DF: i64 = 300;
pub const HOOK_MERGE_JACCARD: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct GramStat {
    pub gram: String,
    pub n: i16,
    pub support: i64,
    pub corpus_df: i64,
    pub clue_ids: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// The cluster's most distinctive gram (max support × idf); stable identity.
    pub key_gram: String,
    pub grams: Vec<String>,
    pub clue_ids: Vec<i32>,
    pub support: usize,
}

/// ln(corpus / df), floored at 0. Zero-safe.
pub fn idf(corpus_clues: i64, df: i64) -> f64 {
    (corpus_clues.max(1) as f64 / df.max(1) as f64).ln().max(0.0)
}

/// A gram seeds a cluster when it recurs within the entity and is not a
/// corpus-wide stop-like term.
pub fn is_seed(g: &GramStat) -> bool {
    let cap = if g.n == 2 { HOOK_BIGRAM_MAX_DF } else { HOOK_UNIGRAM_MAX_DF };
    g.support >= HOOK_MIN_SUPPORT && g.corpus_df <= cap
}

/// Containment counts as 1.0; otherwise Jaccard when it clears the bar.
fn merge_score(a: &BTreeSet<i32>, b: &BTreeSet<i32>) -> Option<f64> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    if a.is_subset(b) || b.is_subset(a) {
        return Some(1.0);
    }
    let inter = a.intersection(b).count();
    if inter == 0 {
        return None;
    }
    let j = inter as f64 / (a.len() + b.len() - inter) as f64;
    (j >= HOOK_MERGE_JACCARD).then_some(j)
}

struct Work {
    grams: Vec<usize>, // indices into `seeds`
    clues: BTreeSet<i32>,
}

/// Cluster an entity's grams into hooks:
/// 1. seeds = grams passing `is_seed`, ordered by support desc, gram asc;
/// 2. greedy agglomeration — repeatedly merge the pair with the highest
///    `merge_score` (first pair wins ties) until none qualifies;
/// 3. every clue is assigned to the one cluster whose grams present in the
///    clue carry the most idf weight (ties: the larger cluster, then earlier);
/// 4. clusters below HOOK_MIN_SUPPORT are dropped; the rest are ranked by
///    support desc, key_gram asc, and capped at HOOK_MAX_PER_ENTITY.
pub fn cluster_hooks(grams: &[GramStat], corpus_clues: i64) -> Vec<Cluster> {
    let mut seeds: Vec<&GramStat> = grams.iter().filter(|g| is_seed(g)).collect();
    seeds.sort_by(|a, b| b.support.cmp(&a.support).then(a.gram.cmp(&b.gram)));
    let weight: Vec<f64> = seeds.iter().map(|g| idf(corpus_clues, g.corpus_df)).collect();

    let mut work: Vec<Work> = seeds
        .iter()
        .enumerate()
        .map(|(i, g)| Work { grams: vec![i], clues: g.clue_ids.iter().copied().collect() })
        .collect();

    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for i in 0..work.len() {
            for j in (i + 1)..work.len() {
                if let Some(s) = merge_score(&work[i].clues, &work[j].clues) {
                    if best.map_or(true, |(_, _, bs)| s > bs) {
                        best = Some((i, j, s));
                    }
                }
            }
        }
        let Some((i, j, _)) = best else { break };
        let Work { grams: gj, clues: cj } = work.remove(j);
        work[i].grams.extend(gj);
        work[i].clues.extend(cj);
    }

    // clue -> seed indices whose clue set contains it
    let mut clue_grams: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for (gi, g) in seeds.iter().enumerate() {
        for c in &g.clue_ids {
            clue_grams.entry(*c).or_default().push(gi);
        }
    }
    let mut assigned: Vec<BTreeSet<i32>> = vec![BTreeSet::new(); work.len()];
    for (clue, gis) in &clue_grams {
        let mut best: Option<(usize, f64, usize)> = None;
        for (wi, w) in work.iter().enumerate() {
            let score: f64 = gis.iter().filter(|gi| w.grams.contains(gi)).map(|gi| weight[*gi]).sum();
            if score <= 0.0 {
                continue;
            }
            let size = w.clues.len();
            let better = match best {
                None => true,
                Some((_, bs, bsize)) => score > bs || (score == bs && size > bsize),
            };
            if better {
                best = Some((wi, score, size));
            }
        }
        if let Some((wi, _, _)) = best {
            assigned[wi].insert(*clue);
        }
    }

    let mut out: Vec<Cluster> = work
        .iter()
        .zip(assigned)
        .filter_map(|(w, clues)| {
            if (clues.len() as i64) < HOOK_MIN_SUPPORT {
                return None;
            }
            let key = w.grams.iter().copied().max_by(|&a, &b| {
                let sa = seeds[a].support as f64 * weight[a];
                let sb = seeds[b].support as f64 * weight[b];
                sa.partial_cmp(&sb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(seeds[b].gram.cmp(&seeds[a].gram)) // tie: alphabetically first
            })?;
            let mut gs: Vec<String> = w.grams.iter().map(|&g| seeds[g].gram.clone()).collect();
            gs.sort();
            Some(Cluster {
                key_gram: seeds[key].gram.clone(),
                grams: gs,
                support: clues.len(),
                clue_ids: clues.into_iter().collect(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.support.cmp(&a.support).then(a.key_gram.cmp(&b.key_gram)));
    out.truncate(HOOK_MAX_PER_ENTITY);
    out
}
```

Add `mod hooks;` to `backend/src/main.rs`.

- [ ] **Step 4: Run the tests**

Run: `cd backend && cargo test hooks::`
Expected: 8 passed. If `visigoths_cluster_into_three_angles_in_support_order` fails on the third key gram, recheck the arithmetic: `5 × ln(530000/1254) = 30.2` for spain versus `3 × ln(530000/15) = 31.4` for 711 — 711 must win.

- [ ] **Step 5: Commit**

```bash
git add backend/src/hooks.rs backend/src/main.rs
git commit -m "feat(objects): cluster an entity's clue grams into ranked hooks

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `hooks.rs` — hook selection, coverage cap, vetted matching, label prompts and gates (pure)

**Files:**
- Modify: `backend/src/hooks.rs` (append), `backend/src/pavlov.rs:39` (`fn norm_tokens` → `pub(crate) fn norm_tokens`)

**Interfaces:**
- Consumes: `pavlov::phrase_leaks_answer(answer, phrase) -> bool`, `pavlov::trim_scaffolding(&str) -> String`.
- Produces:
  - `pub struct HookExposure { pub id: i32, pub rank: i32, pub seen: i64, pub last_wrong: bool }`, `pub fn pick_hook(&[HookExposure]) -> Option<i32>`
  - `pub const HOOK_COVERAGE_CAP_DAYS: f64 = 7.0`, `pub fn cap_for_coverage(interval_days: f64, interval_secs: i64, has_unseen: bool) -> (f64, i64)`
  - `pub fn vetted_matches(cue_lexemes: &[String], grams: &[String]) -> bool`
  - `pub const HOOK_LABEL_MODEL: &str = "gpt-4o-mini"`, `pub const HOOK_LABEL_BATCH: i64 = 20`
  - `pub struct HookLabelInput { pub answer: String, pub key_gram: String, pub grams: Vec<String>, pub sample_clues: Vec<String> }`
  - `pub struct HookLabelOutcome { pub answer: String, pub key_gram: String, pub cue: Option<String> }`
  - `pub fn hook_label_prompts(&[HookLabelInput]) -> (String, String)`, `pub fn label_grounded(cue: &str, clues: &[String]) -> bool`, `pub fn parse_hook_labels(v: &serde_json::Value, inputs: &[HookLabelInput]) -> Vec<HookLabelOutcome>`
  - `pub struct VettedPair { pub cue: String, pub response: String }`, `pub fn parse_vetted_tsv(s: &str) -> Vec<VettedPair>`

- [ ] **Step 1: Write the failing tests** (append inside the existing `mod tests`)

```rust
    fn h(id: i32, rank: i32, seen: i64, last_wrong: bool) -> HookExposure {
        HookExposure { id, rank, seen, last_wrong }
    }

    #[test]
    fn pick_hook_prefers_unseen_then_last_wrong_then_rank() {
        assert_eq!(pick_hook(&[h(1, 1, 3, false), h(2, 2, 0, false), h(3, 3, 0, false)]), Some(2));
        assert_eq!(pick_hook(&[h(1, 1, 2, false), h(2, 2, 2, true), h(3, 3, 2, false)]), Some(2));
        assert_eq!(pick_hook(&[h(1, 2, 1, false), h(2, 1, 1, false)]), Some(2));
        assert_eq!(pick_hook(&[]), None);
    }

    #[test]
    fn coverage_cap_only_bites_with_unseen_hooks_and_long_intervals() {
        assert_eq!(cap_for_coverage(30.0, 30 * 86_400, true), (7.0, 7 * 86_400));
        assert_eq!(cap_for_coverage(30.0, 30 * 86_400, false), (30.0, 30 * 86_400));
        assert_eq!(cap_for_coverage(3.0, 3 * 86_400, true), (3.0, 3 * 86_400));
        assert_eq!(cap_for_coverage(0.0, 600, true), (0.0, 600));
    }

    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn vetted_matches_when_every_token_of_some_gram_is_a_lexeme() {
        assert!(vetted_matches(&s(&["finnish", "compos"]), &s(&["finnish compos", "finlandia"])));
        assert!(vetted_matches(&s(&["lullabi"]), &s(&["lullabi", "requiem"])));
        assert!(!vetted_matches(&s(&["german", "requiem"]), &s(&["lullabi"])));
        assert!(!vetted_matches(&s(&["sack"]), &s(&["sack rome"])));
        assert!(!vetted_matches(&[], &s(&["x"])));
    }

    fn input(answer: &str, key: &str, grams: &[&str], clues: &[&str]) -> HookLabelInput {
        HookLabelInput { answer: answer.into(), key_gram: key.into(), grams: s(grams), sample_clues: s(clues) }
    }

    #[test]
    fn label_prompts_carry_answer_grams_and_clues_and_demand_json() {
        let (system, user) = hook_label_prompts(&[input(
            "the Visigoths", "711", &["711", "spain"],
            &["In 711 a Muslim army defeated Roderick, the last king of these people in Spain"],
        )]);
        assert!(system.contains("JSON"));
        assert!(system.to_lowercase().contains("never"));
        assert!(user.contains("Visigoths"));
        assert!(user.contains("\"711\""));
        assert!(user.contains("Roderick"));
    }

    #[test]
    fn label_grounded_requires_content_words_to_appear_in_a_clue() {
        let clues = s(&["In 711 a Muslim army defeated Roderick, the last king of these people in Spain"]);
        assert!(label_grounded("last king of these people in Spain", &clues));
        assert!(label_grounded("Roderick's people", &clues));
        assert!(label_grounded("Spain in 711", &clues)); // "711" is short: not checked
        assert!(!label_grounded("kings of Toledo", &clues));
        assert!(!label_grounded("ruled Spain until 711", &clues)); // "ruled"/"until" absent
        assert!(!label_grounded("", &clues));
    }

    #[test]
    fn parse_labels_applies_every_gate() {
        let inputs = vec![
            input("the Visigoths", "711", &["711", "spain"], &["These western Goths (as opposed to the eastern Ostrogoths) ruled Spain until 711"]),
            input("the Visigoths", "alar", &["alar", "sack rome"], &["410 A.D.: Under Alaric, these \"Westerners\" sack Rome"]),
            input("Brahms", "lullabi", &["lullabi"], &["This composer of a famous Lullaby"]),
            input("Solomon", "wise", &["wise"], &["This wise king judged between two mothers"]),
            input("Solomon", "templ", &["templ"], &["He built the first temple in Jerusalem"]),
            input("Solomon", "mother", &["mother"], &["This wise king judged between two mothers"]),
        ];
        let v = serde_json::json!({ "results": [
            { "answer": "the Visigoths", "key_gram": "711", "keep": true, "cue": "\"ruled Spain until 711\"" },
            { "answer": "the Visigoths", "key_gram": "alar", "keep": true, "cue": "the Visigoths' Alaric sacks Rome" },  // leaks the answer
            { "answer": "Brahms", "key_gram": "lullabi", "keep": true, "cue": "Lullaby composer of Hamburg" },          // Hamburg not in a clue
            { "answer": "Solomon", "key_gram": "wise", "keep": false, "cue": "wise king" },                              // keep=false
            { "answer": "Solomon", "key_gram": "templ", "keep": true, "cue": "possibly built the first temple" },        // hedge
            { "answer": "Solomon", "key_gram": "mother", "keep": true, "cue": "this wise king who judged between two mothers and more" }, // > 8 words after trim
            { "answer": "Nobody", "key_gram": "x", "keep": true, "cue": "orphan" },                                      // no input: skipped
        ]});
        let out = parse_hook_labels(&v, &inputs);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0].cue.as_deref(), Some("ruled Spain until 711"));
        assert!(out[1].cue.is_none(), "answer leak");
        assert!(out[2].cue.is_none(), "ungrounded word");
        assert!(out[3].cue.is_none(), "keep=false");
        assert!(out[4].cue.is_none(), "hedge");
        assert!(out[5].cue.is_none(), "too long");
    }

    #[test]
    fn parse_labels_of_garbage_is_empty() {
        assert!(parse_hook_labels(&serde_json::json!({"nope": 1}), &[]).is_empty());
    }

    #[test]
    fn vetted_tsv_parses_rows_and_skips_header_and_blanks() {
        let tsv = "cue\tresponse\tdomain\tsource_url\nFINLAND\tjean \"finlandia\" sibelius\trussian\thttps://x\n\nbad line\n\"lullaby\"\tjohannes brahms\tgerman\thttps://x\n";
        let rows = parse_vetted_tsv(tsv);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cue, "FINLAND");
        assert_eq!(rows[0].response, "jean \"finlandia\" sibelius");
        assert_eq!(rows[1].response, "johannes brahms");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test hooks::`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement** (append to `backend/src/hooks.rs` above `mod tests`)

```rust
use serde_json::Value;

use crate::pavlov::{phrase_leaks_answer, trim_scaffolding};

// ---------------------------------------------------------------- drill helpers (spec §3)

#[derive(Debug, Clone, PartialEq)]
pub struct HookExposure {
    pub id: i32,
    pub rank: i32,
    pub seen: i64,
    pub last_wrong: bool,
}

/// Fewest exposures → last rating wrong → rank → id.
pub fn pick_hook(hooks: &[HookExposure]) -> Option<i32> {
    hooks
        .iter()
        .min_by(|a, b| {
            a.seen
                .cmp(&b.seen)
                .then(b.last_wrong.cmp(&a.last_wrong)) // true (wrong) sorts first
                .then(a.rank.cmp(&b.rank))
                .then(a.id.cmp(&b.id))
        })
        .map(|h| h.id)
}

pub const HOOK_COVERAGE_CAP_DAYS: f64 = 7.0;
const DAY_SECS: i64 = 86_400;

/// While the entity still has a labeled hook this user has never seen, no
/// rating may schedule it further out than HOOK_COVERAGE_CAP_DAYS.
pub fn cap_for_coverage(interval_days: f64, interval_secs: i64, has_unseen: bool) -> (f64, i64) {
    if has_unseen && interval_days > HOOK_COVERAGE_CAP_DAYS {
        (HOOK_COVERAGE_CAP_DAYS, HOOK_COVERAGE_CAP_DAYS as i64 * DAY_SECS)
    } else {
        (interval_days, interval_secs)
    }
}

// ---------------------------------------------------------------- vetted import (spec §2)

#[derive(Debug, Clone, PartialEq)]
pub struct VettedPair {
    pub cue: String,
    pub response: String,
}

/// `cue\tresponse\tdomain\tsource_url` rows; header and malformed lines skipped.
pub fn parse_vetted_tsv(s: &str) -> Vec<VettedPair> {
    s.lines()
        .skip(1)
        .filter_map(|line| {
            let mut it = line.split('\t');
            let cue = it.next()?.trim();
            let response = it.next()?.trim();
            if cue.is_empty() || response.is_empty() {
                return None;
            }
            Some(VettedPair { cue: cue.to_string(), response: response.to_string() })
        })
        .collect()
}

/// A vetted cue (as Postgres `english` lexemes) labels a cluster when every
/// token of at least one cluster gram is among the lexemes.
pub fn vetted_matches(cue_lexemes: &[String], grams: &[String]) -> bool {
    grams.iter().any(|g| {
        let toks: Vec<&str> = g.split_whitespace().collect();
        !toks.is_empty() && toks.iter().all(|t| cue_lexemes.iter().any(|l| l == t))
    })
}

// ---------------------------------------------------------------- model labels (spec §2)

pub const HOOK_LABEL_MODEL: &str = "gpt-4o-mini";
pub const HOOK_LABEL_BATCH: i64 = 20;
const HEDGES: &[&str] = &["possibly", "perhaps", "often", "maybe", "sometimes"];
const MAX_LABEL_WORDS: usize = 8;

#[derive(Debug, Clone)]
pub struct HookLabelInput {
    pub answer: String,
    pub key_gram: String,
    pub grams: Vec<String>,
    pub sample_clues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookLabelOutcome {
    pub answer: String,
    pub key_gram: String,
    pub cue: Option<String>,
}

pub fn hook_label_prompts(batch: &[HookLabelInput]) -> (String, String) {
    let system = "You write short Jeopardy! cue labels. For each item you get an answer, the stemmed key \
terms of ONE angle the clue writers use for that answer, and real clues from that angle. Return a cue \
of 2 to 8 words naming that angle the way a Jeopardy! clue would, built only from words that appear \
in the supplied clues (names, dates, places, works). NEVER include the answer or any word of the \
answer. No hedges (possibly, perhaps, often). No leading 'this' or 'these'. Set keep=false when the \
clues do not share a real angle. Respond with JSON only: {\"results\": [{\"answer\": string (echoed \
verbatim), \"key_gram\": string (echoed verbatim), \"keep\": boolean, \"cue\": string}]}"
        .to_string();
    let items: Vec<Value> = batch
        .iter()
        .map(|b| {
            serde_json::json!({
                "answer": b.answer,
                "key_gram": b.key_gram,
                "terms": b.grams,
                "clues": b.sample_clues,
            })
        })
        .collect();
    let user = serde_json::to_string_pretty(&serde_json::json!({ "hooks": items })).expect("serializable");
    (system, user)
}

fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Every content token (≥ 4 chars) of `cue` appears as a token of some clue.
pub fn label_grounded(cue: &str, clues: &[String]) -> bool {
    let cue_toks = tokens(cue);
    if cue_toks.is_empty() {
        return false;
    }
    let clue_toks: std::collections::HashSet<String> = clues.iter().flat_map(|c| tokens(c)).collect();
    cue_toks.iter().filter(|t| t.len() >= 4).all(|t| clue_toks.contains(t))
}

/// Lenient parse; every gate from the spec applied. Items with no matching
/// input are skipped; an item that fails a gate yields `cue: None`.
pub fn parse_hook_labels(v: &Value, inputs: &[HookLabelInput]) -> Vec<HookLabelOutcome> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|item| {
            let answer = item.get("answer")?.as_str()?.trim().to_string();
            let key_gram = item.get("key_gram")?.as_str()?.trim().to_string();
            let input = inputs
                .iter()
                .find(|i| i.answer.eq_ignore_ascii_case(&answer) && i.key_gram == key_gram)?;
            let raw = item
                .get("cue")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'])
                .to_string();
            let cue = trim_scaffolding(&raw);
            let keep = item.get("keep").and_then(|k| k.as_bool()).unwrap_or(true);
            let words = cue.split_whitespace().count();
            let hedged = tokens(&cue).iter().any(|t| HEDGES.contains(&t.as_str()));
            let ok = keep
                && !cue.is_empty()
                && words <= MAX_LABEL_WORDS
                && !phrase_leaks_answer(&input.answer, &cue)
                && label_grounded(&cue, &input.sample_clues)
                && !hedged;
            Some(HookLabelOutcome {
                answer: input.answer.clone(),
                key_gram,
                cue: ok.then_some(cue),
            })
        })
        .collect()
}
```

In `backend/src/pavlov.rs` change `fn norm_tokens(` to `pub(crate) fn norm_tokens(` (Task 9's list route does not need it, but `objects.rs` Task 6 uses it for the vetted slug; keeping it crate-visible avoids a second copy).

- [ ] **Step 4: Run the tests**

Run: `cd backend && cargo test hooks::`
Expected: 16 passed. Watch `parse_labels_applies_every_gate` item 6: `trim_scaffolding` strips the leading "this", leaving 9 words — still > 8, so `cue` must be `None`.

- [ ] **Step 5: Commit**

```bash
git add backend/src/hooks.rs backend/src/pavlov.rs
git commit -m "feat(objects): hook selection, coverage cap, vetted matching, label prompts and gates

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 5: `objects.rs` — the resolve job, pipeline re-keying, admin endpoint

**Files:**
- Create: `backend/src/objects.rs`
- Modify: `backend/src/pavlov.rs` (`NORM_EXPR`, `candidate_rows` eligibility, `assemble_stage`), `backend/src/routes/pavlov.rs` (admin handlers), `backend/src/main.rs` (`mod objects;`, routes)

**Interfaces:**
- Consumes: `entity::{resolve, norm_response, Form, Entity}`.
- Produces:
  - `pub async fn resolved_entities(state: &Arc<AppState>) -> Result<Vec<entity::Entity>, AppError>`
  - `pub async fn run_resolve(state: &Arc<AppState>) -> Result<(), AppError>`
  - `pub async fn refresh_entity_rows(state: &Arc<AppState>, entities: &[entity::Entity]) -> Result<(), AppError>`
  - `POST /api/admin/pavlov/resolve` → `{ started: bool, running?: true }` (same contract as generate)

- [ ] **Step 1: Re-key the v2 pipeline on `entity_norm`** (`backend/src/pavlov.rs`)

Replace the `NORM_EXPR` const:

```rust
/// The entity key of a clue's response: `entity_norm` once the resolve job has
/// run, else 0008's string normalization (so generation keeps working on a
/// database that has never been resolved).
const NORM_EXPR: &str =
    "COALESCE(jq.entity_norm, lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))))";
```

In `candidate_rows`, eligibility becomes a count over the entity (the per-form `answer_freq` column undercounts merged entities):

```rust
        "WITH eligible AS (
           SELECT {NORM_EXPR} AS norm
           FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.question IS NOT NULL
           GROUP BY 1 HAVING count(*) >= $5
         ), sup AS (
```

and bind `MIN_ANSWER_FREQ as i64` (the parameter is now compared to `count(*)`, a bigint).

In `assemble_stage`: the stale delete keeps vetted entities —

```rust
    sqlx::query(
        "DELETE FROM pavlov_answers WHERE vetted = false AND answer_norm NOT IN
           (SELECT DISTINCT answer_norm FROM pavlov_cues
            WHERE status = 'active' AND tier = 'standard')",
    )
```

and the upsert stops overwriting the display name and computes frequency over the entity:

```rust
        let insert_sql = format!(
            "INSERT INTO pavlov_answers
               (answer_norm, answer, meta_category, phrases, phrase_tiers, score, example_clue_ids, answer_freq)
             VALUES ($1, $2, $3, $4, $5, $6, $7,
                     (SELECT count(*) FROM jeopardy_questions jq
                      WHERE jq.archived = false AND jq.question IS NOT NULL AND {NORM_EXPR} = $1))
             ON CONFLICT (answer_norm) DO UPDATE SET
               meta_category = EXCLUDED.meta_category,
               phrases = EXCLUDED.phrases,
               phrase_tiers = EXCLUDED.phrase_tiers,
               score = EXCLUDED.score,
               example_clue_ids = EXCLUDED.example_clue_ids,
               answer_freq = EXCLUDED.answer_freq"
        );
        sqlx::query(&insert_sql)
```

(bind list unchanged). Add a comment above it: `// answer/forms/vetted are owned by objects::refresh_entity_rows; a rerun of resolve or hooks refreshes them for rows created here.`

- [ ] **Step 2: Write `backend/src/objects.rs` (resolve half)**

```rust
//! Async jobs for Jeopardy objects (spec §5): entity resolution and hook
//! mining. All pure logic lives in `entity.rs` / `hooks.rs`; this module is
//! SQL and orchestration. Every job is idempotent and resumable.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::entity::{self, Entity, Form};
use crate::error::AppError;
use crate::AppState;

const UPDATE_CHUNK: usize = 5000;

/// Resolve the whole non-archived corpus into entities (pure, in memory).
pub async fn resolved_entities(state: &Arc<AppState>) -> Result<Vec<Entity>, AppError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT question, count(*) FROM jeopardy_questions
         WHERE archived = false AND question IS NOT NULL GROUP BY 1",
    )
    .fetch_all(&state.pool)
    .await?;
    let forms: Vec<Form> = rows.into_iter().map(|(raw, count)| Form { raw, count }).collect();
    let entities = entity::resolve(&forms);
    tracing::info!("objects: {} response forms -> {} entities", forms.len(), entities.len());
    Ok(entities)
}

/// Job 1 (spec §5): write `entity_norm` on every clue, merge deck rows whose
/// old string norms belong to one entity, re-key cues and n-grams, refresh
/// forms / display / frequency. A rerun finds nothing left to merge.
pub async fn run_resolve(state: &Arc<AppState>) -> Result<(), AppError> {
    let entities = resolved_entities(state).await?;

    // 1. entity_norm per clue (chunked; only rows whose key changes are written)
    let pairs: Vec<(String, String)> = entities
        .iter()
        .flat_map(|e| e.forms.iter().map(move |f| (f.clone(), e.key.clone())))
        .collect();
    for chunk in pairs.chunks(UPDATE_CHUNK) {
        let (fs, ks): (Vec<String>, Vec<String>) = chunk.iter().cloned().unzip();
        sqlx::query(
            "UPDATE jeopardy_questions jq SET entity_norm = m.key
             FROM unnest($1::text[], $2::text[]) AS m(form, key)
             WHERE jq.question = m.form AND jq.archived = false
               AND jq.entity_norm IS DISTINCT FROM m.key",
        )
        .bind(&fs)
        .bind(&ks)
        .execute(&state.pool)
        .await?;
    }
    tracing::info!("objects resolve: entity_norm written");

    // 2. merge deck rows: the old (0008) norms of an entity's forms may map to
    //    several pavlov_answers rows — keep one, move cards/reviews, re-key cues.
    let mut merged = 0usize;
    for e in &entities {
        let old_norms: Vec<String> = e
            .forms
            .iter()
            .map(|f| entity::norm_response(f))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let deck: Vec<(i32, String)> = sqlx::query_as(
            "SELECT id, answer_norm FROM pavlov_answers WHERE answer_norm = ANY($1)
             ORDER BY answer_freq DESC, id",
        )
        .bind(&old_norms)
        .fetch_all(&state.pool)
        .await?;
        if deck.is_empty() || (deck.len() == 1 && deck[0].1 == e.key) {
            continue;
        }
        let survivor = deck.iter().find(|(_, n)| *n == e.key).map(|(id, _)| *id).unwrap_or(deck[0].0);
        let losers: Vec<i32> = deck.iter().map(|(id, _)| *id).filter(|id| *id != survivor).collect();

        let mut tx = state.pool.begin().await?;
        for loser in &losers {
            merge_cards(&mut tx, survivor, *loser).await?;
            sqlx::query("UPDATE pavlov_reviews SET answer_id = $1 WHERE answer_id = $2")
                .bind(survivor)
                .bind(loser)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM pavlov_answers WHERE id = $1").bind(loser).execute(&mut *tx).await?;
        }
        // cues: a loser cue with the same stem as a survivor cue would collide on
        // UNIQUE (answer_norm, cue_stem) — drop it first, then re-key the rest.
        sqlx::query(
            "DELETE FROM pavlov_cues c USING pavlov_cues k
             WHERE c.answer_norm = ANY($1) AND c.answer_norm <> $2
               AND k.answer_norm = $2 AND k.cue_stem = c.cue_stem",
        )
        .bind(&old_norms)
        .bind(&e.key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE pavlov_cues SET answer_norm = $2, answer = $3
             WHERE answer_norm = ANY($1) AND answer_norm <> $2",
        )
        .bind(&old_norms)
        .bind(&e.key)
        .bind(&e.display)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE pavlov_answers SET answer_norm = $2 WHERE id = $1")
            .bind(survivor)
            .bind(&e.key)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        merged += 1;
    }
    tracing::info!("objects resolve: {merged} entities merged in the deck");

    // 3. n-grams: re-key every old norm that differs from its entity key
    let rekey: Vec<(String, String)> = entities
        .iter()
        .flat_map(|e| {
            e.forms
                .iter()
                .map(|f| (entity::norm_response(f), e.key.clone()))
                .filter(|(o, k)| o != k)
                .collect::<BTreeSet<_>>()
        })
        .collect();
    for chunk in rekey.chunks(UPDATE_CHUNK) {
        let (os, ks): (Vec<String>, Vec<String>) = chunk.iter().cloned().unzip();
        sqlx::query(
            "UPDATE pavlov_clue_ngrams g SET answer_norm = m.key
             FROM unnest($1::text[], $2::text[]) AS m(old, key)
             WHERE g.answer_norm = m.old",
        )
        .bind(&os)
        .bind(&ks)
        .execute(&state.pool)
        .await?;
    }
    tracing::info!("objects resolve: {} n-gram keys rewritten", rekey.len());

    refresh_entity_rows(state, &entities).await
}

/// For one user holding cards on both rows keep the one with more reps
/// (ties: the survivor's); re-point the loser's card for users who only
/// have that one.
async fn merge_cards(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    survivor: i32,
    loser: i32,
) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM pavlov_cards s USING pavlov_cards l
         WHERE s.answer_id = $1 AND l.answer_id = $2 AND s.user_id = l.user_id AND l.reps > s.reps",
    )
    .bind(survivor)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "DELETE FROM pavlov_cards l USING pavlov_cards s
         WHERE l.answer_id = $2 AND s.answer_id = $1 AND s.user_id = l.user_id",
    )
    .bind(survivor)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE pavlov_cards SET answer_id = $1 WHERE answer_id = $2")
        .bind(survivor)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Refresh forms / display / answer_freq on every deck row from the resolved
/// corpus. Rerunnable; the end of resolve and the start of hooks both call it
/// (assemble_stage creates rows with a mode() display and no forms).
pub async fn refresh_entity_rows(state: &Arc<AppState>, entities: &[Entity]) -> Result<(), AppError> {
    let keys: Vec<String> = sqlx::query_scalar("SELECT answer_norm FROM pavlov_answers")
        .fetch_all(&state.pool)
        .await?;
    let in_deck: std::collections::HashSet<&str> = keys.iter().map(|s| s.as_str()).collect();
    let mut n = 0usize;
    for e in entities.iter().filter(|e| in_deck.contains(e.key.as_str())) {
        sqlx::query(
            "UPDATE pavlov_answers SET forms = $2, answer = $3, answer_freq = $4 WHERE answer_norm = $1",
        )
        .bind(&e.key)
        .bind(&e.forms)
        .bind(&e.display)
        .bind(e.freq as i32)
        .execute(&state.pool)
        .await?;
        n += 1;
    }
    tracing::info!("objects: refreshed {n} entity rows");
    Ok(())
}
```

Add `mod objects;` to `backend/src/main.rs`.

- [ ] **Step 3: Admin handlers** (`backend/src/routes/pavlov.rs`, after `generate`)

Factor the shared guard/spawn so the three jobs read identically:

```rust
/// Admin-only, single-flight (shared `pavlov_inflight` flag for generate /
/// resolve / hooks), fire-and-forget. `needs_key` gates jobs that call OpenAI.
async fn spawn_admin_job<F, Fut>(
    state: Arc<AppState>,
    auth: &AuthUser,
    name: &'static str,
    needs_key: bool,
    job: F,
) -> Result<Json<Value>, AppError>
where
    F: FnOnce(Arc<AppState>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), AppError>> + Send + 'static,
{
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    if needs_key && state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest("OPENAI_API_KEY not configured".into()));
    }
    if state
        .pavlov_inflight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(Json(json!({ "started": false, "running": true })));
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = job(st.clone()).await {
            tracing::error!("pavlov {name} failed (resumable — rerun to continue): {e:?}");
        }
        st.pavlov_inflight.store(false, Ordering::SeqCst);
    });
    Ok(Json(json!({ "started": true })))
}

pub async fn generate(State(state): State<Arc<AppState>>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    spawn_admin_job(state, &auth, "generation", true, |st| async move {
        crate::pavlov::run_generation(&st).await
    })
    .await
}

pub async fn resolve(State(state): State<Arc<AppState>>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    spawn_admin_job(state, &auth, "resolve", false, |st| async move {
        crate::objects::run_resolve(&st).await
    })
    .await
}
```

(delete the old `generate` body). Register in `main.rs`: `.route("/api/admin/pavlov/resolve", post(routes::pavlov::resolve))`.

- [ ] **Step 4: Build and test**

Run: `cd backend && cargo build && cargo test`
Expected: builds; all tests pass (no new unit tests here — the job is SQL; Task 7's verify script covers it).

- [ ] **Step 5: Commit**

```bash
git add backend/src/objects.rs backend/src/pavlov.rs backend/src/routes/pavlov.rs backend/src/main.rs
git commit -m "feat(objects): resolve job — entity_norm, deck merge, cue/n-gram re-key; pipeline keys on entities

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: `objects.rs` — the hooks job (mining, vetted import, labels) and status

**Files:**
- Modify: `backend/src/objects.rs` (append), `backend/src/routes/pavlov.rs` (`hooks` handler, `status`), `backend/src/main.rs` (route), `backend/migrations/0016_jeopardy_objects.sql` (add the `pavlov_gram_df` table — the migration has not been applied anywhere yet, so editing it is safe)

**Interfaces:**
- Consumes: `hooks::{cluster_hooks, GramStat, vetted_matches, parse_vetted_tsv, hook_label_prompts, parse_hook_labels, HookLabelInput, HOOK_MIN_SUPPORT, HOOK_LABEL_MODEL, HOOK_LABEL_BATCH}`, `entity::entity_key`, `pavlov::norm_tokens`, `openai::chat_json`, `objects::{resolved_entities, refresh_entity_rows}`.
- Produces: `pub async fn run_hooks(state) -> Result<(), AppError>`; `POST /api/admin/pavlov/hooks`; `GET /api/admin/pavlov/status` gains `hooks: { total, labeled, unlabeled, vetted, entitiesPending }`.

- [ ] **Step 1: Add the gram-frequency table to the migration**

Append to `backend/migrations/0016_jeopardy_objects.sql`:

```sql
-- Corpus document frequency per gram, filled by the hooks job on first run
-- (one GROUP BY over pavlov_clue_ngrams, minutes); read per entity after that.
CREATE TABLE IF NOT EXISTS pavlov_gram_df (
  gram TEXT PRIMARY KEY,
  df   INTEGER NOT NULL
);
```

- [ ] **Step 2: Append the hooks job to `backend/src/objects.rs`**

```rust
use crate::hooks::{
    self, cluster_hooks, hook_label_prompts, parse_hook_labels, parse_vetted_tsv, vetted_matches,
    GramStat, HookLabelInput, HOOK_LABEL_BATCH, HOOK_LABEL_MODEL, HOOK_MIN_SUPPORT,
};

const VETTED_TSV: &str = include_str!("../data/pavlovs-jboard.tsv");
const HOOK_SAMPLE_CLUES: i64 = 5;
const MINE_BATCH: i64 = 200;

/// Job 2 (spec §5): refresh entity rows, import the vetted pairs, mine hooks
/// for every entity, label them (vetted → cue → model), rerank.
pub async fn run_hooks(state: &Arc<AppState>) -> Result<(), AppError> {
    let entities = resolved_entities(state).await?;
    let vetted = import_vetted(state).await?;
    refresh_entity_rows(state, &entities).await?;
    ensure_gram_df(state).await?;

    // A completed previous run leaves no entity pending → this is a
    // regeneration: re-mine everything. Otherwise resume where it stopped.
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pavlov_answers WHERE hooks_built_at IS NULL")
        .fetch_one(&state.pool)
        .await?;
    if pending == 0 {
        sqlx::query("UPDATE pavlov_answers SET hooks_built_at = NULL").execute(&state.pool).await?;
    }
    // Unlabeled hooks from earlier runs get another try at a label.
    sqlx::query("UPDATE pavlov_hooks SET label_attempted_at = NULL WHERE cue IS NULL AND status = 'active'")
        .execute(&state.pool)
        .await?;

    let corpus_clues: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jeopardy_questions WHERE archived = false AND question IS NOT NULL")
            .fetch_one(&state.pool)
            .await?;
    mine_hooks(state, corpus_clues).await?;
    label_vetted(state, &vetted).await?;
    label_from_cues(state).await?;
    label_with_model(state).await?;
    rerank(state).await
}

async fn ensure_gram_df(state: &Arc<AppState>) -> Result<(), AppError> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM pavlov_gram_df").fetch_one(&state.pool).await?;
    if n > 0 {
        return Ok(());
    }
    tracing::info!("objects hooks: building pavlov_gram_df (one-time)");
    sqlx::query(
        "INSERT INTO pavlov_gram_df (gram, df)
         SELECT gram, count(DISTINCT clue_id) FROM pavlov_clue_ngrams GROUP BY 1",
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct GramRow {
    gram: String,
    n: i16,
    support: i64,
    df: i32,
    clue_ids: Vec<i32>,
}

/// Cluster every pending entity's grams and upsert its hooks.
async fn mine_hooks(state: &Arc<AppState>, corpus_clues: i64) -> Result<(), AppError> {
    loop {
        let batch: Vec<(i32, String)> = sqlx::query_as(
            "SELECT id, answer_norm FROM pavlov_answers WHERE hooks_built_at IS NULL ORDER BY id LIMIT $1",
        )
        .bind(MINE_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }
        for (answer_id, norm) in batch {
            let rows: Vec<GramRow> = sqlx::query_as(
                "SELECT g.gram, g.n, count(DISTINCT g.clue_id) AS support,
                        COALESCE(d.df, 0) AS df,
                        array_agg(DISTINCT g.clue_id) AS clue_ids
                 FROM pavlov_clue_ngrams g LEFT JOIN pavlov_gram_df d ON d.gram = g.gram
                 WHERE g.answer_norm = $1
                 GROUP BY g.gram, g.n, d.df
                 HAVING count(DISTINCT g.clue_id) >= $2",
            )
            .bind(&norm)
            .bind(HOOK_MIN_SUPPORT)
            .fetch_all(&state.pool)
            .await?;
            let grams: Vec<GramStat> = rows
                .into_iter()
                .map(|r| GramStat { gram: r.gram, n: r.n, support: r.support, corpus_df: r.df as i64, clue_ids: r.clue_ids })
                .collect();
            let clusters = cluster_hooks(&grams, corpus_clues);

            let mut tx = state.pool.begin().await?;
            let mut keys: Vec<String> = Vec::with_capacity(clusters.len());
            for (i, c) in clusters.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO pavlov_hooks (answer_id, key_gram, rank, grams, clue_ids, support)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (answer_id, key_gram) DO UPDATE SET
                       rank = EXCLUDED.rank, grams = EXCLUDED.grams,
                       clue_ids = EXCLUDED.clue_ids, support = EXCLUDED.support",
                )
                .bind(answer_id)
                .bind(&c.key_gram)
                .bind((i + 1) as i32)
                .bind(&c.grams)
                .bind(&c.clue_ids)
                .bind(c.support as i32)
                .execute(&mut *tx)
                .await?;
                keys.push(c.key_gram.clone());
            }
            // Mined hooks that no longer come out of clustering and never got a
            // label are noise from a previous run; labeled ones keep their history.
            sqlx::query(
                "DELETE FROM pavlov_hooks WHERE answer_id = $1 AND source = 'mined' AND cue IS NULL
                   AND NOT (key_gram = ANY($2))",
            )
            .bind(answer_id)
            .bind(&keys)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE pavlov_answers SET hooks_built_at = now() WHERE id = $1")
                .bind(answer_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
    }
}

struct VettedMatch {
    answer_id: i32,
    cue: String,
    lexemes: Vec<String>,
}

/// Resolve every vetted response to a deck entity, inserting vetted entities
/// the deck lacks. Unmatched responses are logged for hand review.
async fn import_vetted(state: &Arc<AppState>) -> Result<Vec<VettedMatch>, AppError> {
    let pairs = parse_vetted_tsv(VETTED_TSV);
    let mut out = Vec::with_capacity(pairs.len());
    let mut unmatched = 0usize;
    for p in &pairs {
        let key = entity::entity_key(&p.response);
        if key.is_empty() {
            continue;
        }
        // The response may be a full name whose entity key is the licensed
        // surname: look the old string norm up in the resolved corpus.
        let entity_norm: Option<String> = sqlx::query_scalar(
            "SELECT entity_norm FROM jeopardy_questions
             WHERE archived = false AND entity_norm IS NOT NULL
               AND (entity_norm = $1
                    OR lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) = $1)
             ORDER BY (entity_norm = $1) DESC LIMIT 1",
        )
        .bind(&key)
        .fetch_optional(&state.pool)
        .await?;
        let Some(norm) = entity_norm else {
            unmatched += 1;
            tracing::warn!("vetted pair unmatched in corpus: {:?} = {:?}", p.cue, p.response);
            continue;
        };
        let answer_id: i32 = sqlx::query_scalar(
            "INSERT INTO pavlov_answers
               (answer_norm, answer, meta_category, phrases, phrase_tiers, score, example_clue_ids, answer_freq, vetted)
             SELECT $1,
                    mode() WITHIN GROUP (ORDER BY question),
                    COALESCE(mode() WITHIN GROUP (ORDER BY classifier_category), 'Miscellaneous'),
                    '{}', '{}', 0,
                    (SELECT COALESCE(array_agg(id), '{}') FROM
                       (SELECT id FROM jeopardy_questions WHERE entity_norm = $1 AND archived = false
                        ORDER BY air_date DESC NULLS LAST LIMIT 3) x),
                    count(*), true
             FROM jeopardy_questions WHERE entity_norm = $1 AND archived = false
             HAVING count(*) > 0
             ON CONFLICT (answer_norm) DO UPDATE SET vetted = true
             RETURNING id",
        )
        .bind(&norm)
        .fetch_one(&state.pool)
        .await?;
        let lexemes: Vec<String> =
            sqlx::query_scalar("SELECT tsvector_to_array(to_tsvector('english', $1))")
                .bind(&p.cue)
                .fetch_one(&state.pool)
                .await?;
        out.push(VettedMatch { answer_id, cue: p.cue.clone(), lexemes });
    }
    tracing::info!("objects hooks: {} vetted pairs matched, {unmatched} unmatched", out.len());
    Ok(out)
}

fn vetted_key_gram(cue: &str) -> String {
    let mut toks: Vec<String> = crate::pavlov::norm_tokens(cue).into_iter().collect();
    toks.sort();
    let slug: String = toks.join("-").chars().take(40).collect();
    format!("vetted:{slug}")
}

/// A vetted cue labels the cluster whose grams it hits; otherwise it becomes
/// its own hook, with member clues found by full-text search.
async fn label_vetted(state: &Arc<AppState>, vetted: &[VettedMatch]) -> Result<(), AppError> {
    for v in vetted {
        let hooks: Vec<(i32, Vec<String>)> = sqlx::query_as(
            "SELECT id, grams FROM pavlov_hooks WHERE answer_id = $1 AND status = 'active' ORDER BY rank",
        )
        .bind(v.answer_id)
        .fetch_all(&state.pool)
        .await?;
        if let Some((id, _)) = hooks.iter().find(|(_, grams)| vetted_matches(&v.lexemes, grams)) {
            sqlx::query("UPDATE pavlov_hooks SET cue = $2, source = 'both' WHERE id = $1")
                .bind(id)
                .bind(&v.cue)
                .execute(&state.pool)
                .await?;
            continue;
        }
        let clue_ids: Vec<i32> = sqlx::query_scalar(
            "SELECT jq.id FROM jeopardy_questions jq JOIN pavlov_answers pa ON pa.answer_norm = jq.entity_norm
             WHERE pa.id = $1 AND jq.archived = false
               AND to_tsvector('english', coalesce(jq.answer, '')) @@ plainto_tsquery('english', $2)
             ORDER BY jq.air_date DESC NULLS LAST LIMIT 50",
        )
        .bind(v.answer_id)
        .bind(&v.cue)
        .fetch_all(&state.pool)
        .await?;
        sqlx::query(
            "INSERT INTO pavlov_hooks (answer_id, key_gram, rank, cue, grams, clue_ids, support, source)
             VALUES ($1, $2, 99, $3, '{}', $4, $5, 'vetted')
             ON CONFLICT (answer_id, key_gram) DO UPDATE SET
               cue = EXCLUDED.cue, clue_ids = EXCLUDED.clue_ids, support = EXCLUDED.support",
        )
        .bind(v.answer_id)
        .bind(vetted_key_gram(&v.cue))
        .bind(&v.cue)
        .bind(&clue_ids)
        .bind(clue_ids.len() as i32)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

/// An active standard v2 cue whose stem is one of the cluster's grams labels
/// it for free.
async fn label_from_cues(state: &Arc<AppState>) -> Result<(), AppError> {
    let n = sqlx::query(
        "UPDATE pavlov_hooks h SET cue = c.cue_display, source = 'cue'
         FROM pavlov_answers pa, pavlov_cues c
         WHERE h.answer_id = pa.id AND c.answer_norm = pa.answer_norm
           AND c.status = 'active' AND c.tier = 'standard' AND c.cue_display <> ''
           AND h.cue IS NULL AND h.status = 'active' AND c.cue_stem = ANY(h.grams)",
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    tracing::info!("objects hooks: {n} hooks labeled from existing cues");
    Ok(())
}

#[derive(sqlx::FromRow)]
struct PendingHook {
    id: i32,
    answer: String,
    key_gram: String,
    grams: Vec<String>,
    clue_ids: Vec<i32>,
}

/// Batched model labels for whatever is still unlabeled. Resumable: each
/// batch is marked attempted before the call, and the loop ends when nothing
/// unattempted remains.
async fn label_with_model(state: &Arc<AppState>) -> Result<(), AppError> {
    if state.config.openai_api_key.is_empty() {
        tracing::warn!("objects hooks: no OPENAI_API_KEY — skipping model labels");
        return Ok(());
    }
    loop {
        let batch: Vec<PendingHook> = sqlx::query_as(
            "SELECT h.id, pa.answer, h.key_gram, h.grams, h.clue_ids
             FROM pavlov_hooks h JOIN pavlov_answers pa ON pa.id = h.answer_id
             WHERE h.cue IS NULL AND h.status = 'active' AND h.label_attempted_at IS NULL
             ORDER BY h.id LIMIT $1",
        )
        .bind(HOOK_LABEL_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }
        let ids: Vec<i32> = batch.iter().map(|b| b.id).collect();
        sqlx::query("UPDATE pavlov_hooks SET label_attempted_at = now() WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&state.pool)
            .await?;

        let mut inputs = Vec::with_capacity(batch.len());
        for b in &batch {
            let clues: Vec<String> = sqlx::query_scalar(
                "SELECT coalesce(answer, '') FROM jeopardy_questions WHERE id = ANY($1)
                 ORDER BY air_date DESC NULLS LAST LIMIT $2",
            )
            .bind(&b.clue_ids)
            .bind(HOOK_SAMPLE_CLUES)
            .fetch_all(&state.pool)
            .await?;
            inputs.push(HookLabelInput {
                answer: b.answer.clone(),
                key_gram: b.key_gram.clone(),
                grams: b.grams.clone(),
                sample_clues: clues,
            });
        }
        let (system, user) = hook_label_prompts(&inputs);
        let response =
            crate::openai::chat_json(&state.config.openai_api_key, HOOK_LABEL_MODEL, &system, &user, 0.3).await?;
        let mut labeled = 0usize;
        for out in parse_hook_labels(&response, &inputs) {
            let Some(cue) = out.cue else { continue };
            let Some(b) = batch
                .iter()
                .find(|b| b.key_gram == out.key_gram && b.answer.eq_ignore_ascii_case(&out.answer))
            else {
                continue;
            };
            sqlx::query("UPDATE pavlov_hooks SET cue = $2, source = 'model', model = $3 WHERE id = $1 AND cue IS NULL")
                .bind(b.id)
                .bind(&cue)
                .bind(HOOK_LABEL_MODEL)
                .execute(&state.pool)
                .await?;
            labeled += 1;
        }
        tracing::info!("objects hooks: model batch of {} → {labeled} labeled", batch.len());
    }
}

/// Mined hooks by support, vetted-only hooks after them.
async fn rerank(state: &Arc<AppState>) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE pavlov_hooks h SET rank = r.rn
         FROM (SELECT id, row_number() OVER (PARTITION BY answer_id
                 ORDER BY (source = 'vetted') ASC, support DESC, id) AS rn
               FROM pavlov_hooks WHERE status = 'active') r
         WHERE r.id = h.id",
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}
```

The `hooks::` glob import keeps `hooks::HookExposure` etc. reachable for Task 8 without a second `use`.

- [ ] **Step 3: Handler and status** (`backend/src/routes/pavlov.rs`)

```rust
pub async fn hooks(State(state): State<Arc<AppState>>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    spawn_admin_job(state, &auth, "hooks", false, |st| async move {
        crate::objects::run_hooks(&st).await
    })
    .await
}
```

(`needs_key = false`: the job mines and imports without a key and only skips model labels.) In `status`, after the cue counts:

```rust
    let (total, labeled, vetted): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'active'),
                count(*) FILTER (WHERE status = 'active' AND cue IS NOT NULL),
                count(*) FILTER (WHERE status = 'active' AND source IN ('vetted', 'both'))
         FROM pavlov_hooks",
    )
    .fetch_one(&state.pool)
    .await?;
    let entities_pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pavlov_answers WHERE hooks_built_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
```

and add to the JSON: `"hooks": { "total": total, "labeled": labeled, "unlabeled": total - labeled, "vetted": vetted, "entitiesPending": entities_pending }`. Register `.route("/api/admin/pavlov/hooks", post(routes::pavlov::hooks))` in `main.rs`.

- [ ] **Step 4: Build and test**

Run: `cd backend && cargo build && cargo test`
Expected: builds (the `include_str!` path is relative to `backend/src/`, so `../data/pavlovs-jboard.tsv` resolves to `backend/data/`); all tests pass.

- [ ] **Step 5: Commit**

```bash
git add backend/migrations/0016_jeopardy_objects.sql backend/src/objects.rs backend/src/routes/pavlov.rs backend/src/main.rs
git commit -m "feat(objects): hooks job — mine clusters per entity, import vetted pairs, label from vetted/cues/model, rerank

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: `scripts/verify-objects.sql`

**Files:**
- Create: `scripts/verify-objects.sql`

**Interfaces:** none (read-only SQL run by hand at rollout).

- [ ] **Step 1: Write the script**

```sql
-- Read-only checks for the Jeopardy objects rollout (PG15-compatible).
-- Run: docker run --rm -i -v "$PWD/backend/data:/data" postgres:16 psql "$DB_URL" -f - < scripts/verify-objects.sql
-- "expect 0" checks are failures when nonzero. Uses user_id = 1.

-- A. Resolution coverage: every non-archived clue has an entity key (expect 0 after resolve).
SELECT 'clues_without_entity_norm' AS check, count(*) AS fail_rows
FROM jeopardy_questions WHERE archived = false AND question IS NOT NULL AND entity_norm IS NULL;

-- B. JBoard coverage before/after (informational). "in_deck" is the number to report.
CREATE TEMP TABLE vetted_pairs (cue TEXT, response TEXT, domain TEXT, source_url TEXT);
\copy vetted_pairs FROM '/data/pavlovs-jboard.tsv' WITH (FORMAT text, HEADER)
WITH keyed AS (
  SELECT response,
         lower(trim(regexp_replace(regexp_replace(regexp_replace(regexp_replace(response,
           '\([^)]*\)', '', 'g'), '"[^"]*"', '', 'g'), '\s+', ' ', 'g'), '^(the|a|an) ', '', 'i'))) AS key
  FROM vetted_pairs
), resolved AS (
  SELECT k.response, k.key,
         (SELECT jq.entity_norm FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.entity_norm IS NOT NULL
             AND (jq.entity_norm = k.key
                  OR lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = k.key)
           LIMIT 1) AS entity_norm
  FROM keyed k
)
SELECT count(*) AS pairs,
       count(*) FILTER (WHERE entity_norm IS NOT NULL) AS in_corpus,
       count(*) FILTER (WHERE EXISTS (SELECT 1 FROM pavlov_answers pa WHERE pa.answer_norm = resolved.entity_norm)) AS in_deck
FROM resolved;
-- Unmatched vetted responses, for hand review.
SELECT response, key FROM (
  SELECT k.response, k.key, (SELECT 1 FROM jeopardy_questions jq WHERE jq.archived = false
     AND (jq.entity_norm = k.key OR lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i'))) = k.key) LIMIT 1) AS hit
  FROM (SELECT response, lower(trim(regexp_replace(regexp_replace(regexp_replace(regexp_replace(response,
           '\([^)]*\)', '', 'g'), '"[^"]*"', '', 'g'), '\s+', ' ', 'g'), '^(the|a|an) ', '', 'i'))) AS key FROM vetted_pairs) k
) u WHERE hit IS NULL ORDER BY response;

-- C. The 50 highest-frequency merges, for hand review of false positives.
SELECT answer, answer_norm, answer_freq, forms
FROM pavlov_answers WHERE cardinality(forms) > 1 ORDER BY answer_freq DESC LIMIT 50;

-- D. Deck integrity (expect 0 each).
SELECT 'orphan_cards' AS check, count(*) AS fail_rows
FROM pavlov_cards ca LEFT JOIN pavlov_answers pa ON pa.id = ca.answer_id WHERE pa.id IS NULL
UNION ALL
SELECT 'orphan_reviews', count(*)
FROM pavlov_reviews pr LEFT JOIN pavlov_answers pa ON pa.id = pr.answer_id WHERE pa.id IS NULL
UNION ALL
SELECT 'deck_rows_without_forms', count(*) FROM pavlov_answers WHERE forms = '{}'
UNION ALL
SELECT 'review_hook_on_other_entity', count(*)
FROM pavlov_reviews pr JOIN pavlov_hooks h ON h.id = pr.hook_id WHERE h.answer_id <> pr.answer_id
UNION ALL
SELECT 'undrillable_entities', count(*)
FROM pavlov_answers pa
WHERE cardinality(pa.phrases) = 0
  AND NOT EXISTS (SELECT 1 FROM pavlov_hooks h WHERE h.answer_id = pa.id AND h.status = 'active' AND h.cue IS NOT NULL);

-- E. Hooks: per-entity distribution, label sources, unlabeled count (informational).
SELECT n_hooks, count(*) AS entities FROM (
  SELECT pa.id, count(h.id) FILTER (WHERE h.status = 'active' AND h.cue IS NOT NULL) AS n_hooks
  FROM pavlov_answers pa LEFT JOIN pavlov_hooks h ON h.answer_id = pa.id GROUP BY pa.id
) d GROUP BY n_hooks ORDER BY n_hooks;
SELECT source, status, count(*) FILTER (WHERE cue IS NOT NULL) AS labeled, count(*) FILTER (WHERE cue IS NULL) AS unlabeled
FROM pavlov_hooks GROUP BY 1, 2 ORDER BY 1, 2;
SELECT 'entities_pending_hooks' AS check, count(*) FROM pavlov_answers WHERE hooks_built_at IS NULL;

-- F. Spot check: the Visigoths entity and its hooks.
SELECT pa.answer, pa.forms, pa.answer_freq, h.rank, h.key_gram, h.cue, h.support, h.source, h.status
FROM pavlov_answers pa LEFT JOIN pavlov_hooks h ON h.answer_id = pa.id
WHERE pa.answer_norm = 'visigoths' ORDER BY h.rank;

-- G. Hook coverage for user 1 (mirrors routes/pavlov_stats.rs).
SELECT
  (SELECT count(DISTINCT pr.hook_id) FROM pavlov_reviews pr JOIN pavlov_hooks h ON h.id = pr.hook_id
    WHERE pr.user_id = 1 AND h.status = 'active' AND h.cue IS NOT NULL) AS hooks_seen,
  (SELECT count(*) FROM pavlov_hooks h JOIN pavlov_cards ca ON ca.answer_id = h.answer_id
    WHERE ca.user_id = 1 AND ca.last_review IS NOT NULL AND h.status = 'active' AND h.cue IS NOT NULL) AS hooks_total;
```

- [ ] **Step 2: Syntax-check against an empty database is not possible without the schema; instead run the script's section A only against the live database** (read-only):

Run: `DB_URL=$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g'); sed -n '/-- A\./,/-- B\./p' scripts/verify-objects.sql | docker run --rm -i postgres:16 psql "$DB_URL" -f -`
Expected before migration 0016: `ERROR: column "entity_norm" does not exist` (proves the query reaches the server); after 0016 + resolve: one row with `fail_rows 0`.

- [ ] **Step 3: Commit**

```bash
git add scripts/verify-objects.sql
git commit -m "chore(objects): verify script for resolution, vetted coverage, merges, hooks

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 8: Drill API — one hook per review, hook map on reveal, coverage cap on grade

**Files:**
- Modify: `backend/src/routes/pavlov.rs` (`pick_new_card`, `drill_next`, `drill_check`, `drill_grade`, new helpers)

**Interfaces:**
- Consumes: `hooks::{pick_hook, HookExposure, cap_for_coverage}`.
- Produces:
  - `pub(crate) const DRILLABLE_SQL: &str` — predicate on alias `pa`.
  - `pub(crate) async fn hook_map(state, user_id, answer_id) -> Result<(String, Vec<String>, Vec<Value>), AppError>` → `(answer, forms, hooks_json)`; hook JSON `{ id, rank, cue, support, source, seen, lastWrongAt }`.
  - `GET /api/pavlov/drill/next` card: `{ answerId, answerNorm, category, phrases, hookId: number|null, hookRank: number|null, cue: string|null }`.
  - `POST /api/pavlov/drill/check` body `{ answerId, hookId?, typed? }` → `{ correct, answer, answerNorm, forms, hooks, servedHookId, exampleClue: {clue, category, airDate}|null, examples }`.
  - `POST /api/pavlov/drill/grade` body `{ answerId, rating, hookId? }` → `{ state, due, intervalDays, requeueInSession }`.

- [ ] **Step 1: Write the failing test** (append to `mod tests` in `routes/pavlov.rs`)

```rust
    #[test]
    fn card_json_carries_hook_or_falls_back_to_phrases() {
        let row = DrillAnswerRow {
            id: 7, answer_norm: "visigoths".into(),
            phrases: vec!["the Goths split".into()], phrase_tiers: vec!["standard".into()],
            meta_category: "History & Politics".into(),
        };
        let with = drill_card_json(&row, Some(ServedHook { id: 3, rank: 2, cue: "ruled Spain until 711".into() }));
        assert_eq!(with["hookId"], 3);
        assert_eq!(with["cue"], "ruled Spain until 711");
        let without = drill_card_json(&row, None);
        assert!(without["hookId"].is_null());
        assert_eq!(without["phrases"][0]["text"], "the Goths split");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd backend && cargo test routes::pavlov::`
Expected: compile error — `ServedHook` / two-argument `drill_card_json` do not exist.

- [ ] **Step 3: Implement**

Add near the top of the drill section:

```rust
use crate::hooks::{cap_for_coverage, pick_hook, HookExposure};

/// An entity can be served when it has a labeled active hook or (transitional
/// fallback, spec §5) legacy cue phrases. Alias `pa` = pavlov_answers.
pub(crate) const DRILLABLE_SQL: &str = "(cardinality(pa.phrases) > 0 OR EXISTS (
    SELECT 1 FROM pavlov_hooks h WHERE h.answer_id = pa.id AND h.status = 'active' AND h.cue IS NOT NULL))";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServedHook {
    pub id: i32,
    pub rank: i32,
    pub cue: String,
}

fn drill_card_json(r: &DrillAnswerRow, hook: Option<ServedHook>) -> Value {
    let phrases: Vec<Value> = r
        .phrases
        .iter()
        .zip(r.phrase_tiers.iter())
        .map(|(text, tier)| json!({ "text": text, "tier": tier }))
        .collect();
    json!({
        "answerId": r.id, "answerNorm": r.answer_norm, "category": r.meta_category,
        "phrases": phrases,
        "hookId": hook.as_ref().map(|h| h.id),
        "hookRank": hook.as_ref().map(|h| h.rank),
        "cue": hook.as_ref().map(|h| h.cue.clone()),
    })
}

#[derive(sqlx::FromRow)]
struct HookPickRow {
    id: i32,
    rank: i32,
    cue: String,
    seen: i64,
    last_wrong: bool,
}

/// Spec §3 hook selection: fewest exposures → last wrong → rank.
async fn served_hook(state: &Arc<AppState>, user_id: i32, answer_id: i32) -> Result<Option<ServedHook>, AppError> {
    let rows: Vec<HookPickRow> = sqlx::query_as(
        "SELECT h.id, h.rank, h.cue, COALESCE(e.n, 0) AS seen,
                COALESCE(e.last_rating = 'wrong', false) AS last_wrong
         FROM pavlov_hooks h
         LEFT JOIN (
           SELECT hook_id, count(*) AS n,
                  (array_agg(rating ORDER BY reviewed_at DESC))[1] AS last_rating
           FROM pavlov_reviews
           WHERE user_id = $1 AND answer_id = $2 AND hook_id IS NOT NULL
           GROUP BY hook_id
         ) e ON e.hook_id = h.id
         WHERE h.answer_id = $2 AND h.status = 'active' AND h.cue IS NOT NULL",
    )
    .bind(user_id)
    .bind(answer_id)
    .fetch_all(&state.pool)
    .await?;
    let exposures: Vec<HookExposure> = rows
        .iter()
        .map(|r| HookExposure { id: r.id, rank: r.rank, seen: r.seen, last_wrong: r.last_wrong })
        .collect();
    Ok(pick_hook(&exposures)
        .and_then(|id| rows.into_iter().find(|r| r.id == id))
        .map(|r| ServedHook { id: r.id, rank: r.rank, cue: r.cue }))
}

/// Serve one card: pick its hook, shape the response.
async fn serve_card(
    state: &Arc<AppState>,
    user_id: i32,
    row: DrillAnswerRow,
    is_new: bool,
    due_count: i64,
    new_remaining: i64,
) -> Result<Json<Value>, AppError> {
    let hook = served_hook(state, user_id, row.id).await?;
    Ok(Json(json!({
        "done": false, "isNew": is_new, "card": drill_card_json(&row, hook),
        "dueCount": due_count, "newRemaining": new_remaining,
    })))
}

#[derive(sqlx::FromRow)]
struct HookMapRow {
    id: i32,
    rank: i32,
    cue: String,
    support: i32,
    source: String,
    seen: i64,
    last_wrong_at: Option<DateTime<Utc>>,
}

/// The entity's display name, merged forms, and labeled active hooks with
/// this user's exposure marks. Shared by drill_check and entity_by_question.
pub(crate) async fn hook_map(
    state: &Arc<AppState>,
    user_id: i32,
    answer_id: i32,
) -> Result<(String, Vec<String>, Vec<Value>), AppError> {
    let (answer, forms): (String, Vec<String>) =
        sqlx::query_as("SELECT answer, forms FROM pavlov_answers WHERE id = $1")
            .bind(answer_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("No such card".into()))?;
    let rows: Vec<HookMapRow> = sqlx::query_as(
        "SELECT h.id, h.rank, h.cue, h.support, h.source,
                COALESCE(e.n, 0) AS seen, e.last_wrong_at
         FROM pavlov_hooks h
         LEFT JOIN (
           SELECT hook_id, count(*) AS n,
                  max(reviewed_at) FILTER (WHERE rating = 'wrong') AS last_wrong_at
           FROM pavlov_reviews
           WHERE user_id = $1 AND answer_id = $2 AND hook_id IS NOT NULL
           GROUP BY hook_id
         ) e ON e.hook_id = h.id
         WHERE h.answer_id = $2 AND h.status = 'active' AND h.cue IS NOT NULL
         ORDER BY h.rank, h.id",
    )
    .bind(user_id)
    .bind(answer_id)
    .fetch_all(&state.pool)
    .await?;
    let hooks = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id, "rank": r.rank, "cue": r.cue, "support": r.support, "source": r.source,
                "seen": r.seen, "lastWrongAt": r.last_wrong_at,
            })
        })
        .collect();
    Ok((answer, forms, hooks))
}
```

`pick_new_card`: add the vetted-first order and the drillable predicate to all three queries —

```rust
    let available: Vec<(String,)> = sqlx::query_as(&format!(
        "SELECT DISTINCT pa.meta_category FROM pavlov_answers pa
         WHERE {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)"
    ))
```

```rust
    let pick_in_cat = format!(
        "SELECT {cols} FROM pavlov_answers pa
         WHERE pa.meta_category = $2 AND {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY pa.vetted DESC, pa.answer_freq DESC, pa.score DESC, pa.id LIMIT 1"
    );
    let pick_any = format!(
        "SELECT {cols} FROM pavlov_answers pa
         WHERE {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY pa.vetted DESC, pa.answer_freq DESC, pa.score DESC, pa.id LIMIT 1"
    );
```

Update the comment above them: "Within the sampled category: vetted (JBoard-named) entities first, then the most frequently recurring unseen entity …". `drill_next`: replace the three `return Ok(Json(json!({ "done": false, ... })))` blocks with `return serve_card(&state, user_id, row, true, due_count, new_remaining).await;` (new) / `false` (due); `more_new_available` becomes:

```rust
    let more_new_available: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM pavlov_answers pa
         WHERE {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1))"
    ))
```

`drill_check`:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckBody {
    pub answer_id: i32,
    pub hook_id: Option<i32>,
    /// Optional: honesty-mode reveal sends no typed answer.
    pub typed: Option<String>,
}

pub async fn drill_check(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CheckBody>,
) -> Result<Json<Value>, AppError> {
    let row: Option<(String, Vec<i32>, String)> = sqlx::query_as(
        "SELECT pa.answer, pa.example_clue_ids, pa.answer_norm FROM pavlov_answers pa WHERE pa.id = $1",
    )
    .bind(body.answer_id)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, example_ids, answer_norm) = row.ok_or_else(|| AppError::NotFound("No such cue".into()))?;
    let correct = body.typed.as_deref().map(|t| answer_match::is_correct(t, &answer));

    let (_, forms, hooks) = hook_map(&state, auth.user_id, body.answer_id).await?;
    let example_clue: Option<(String, Option<String>, Option<chrono::NaiveDate>)> = match body.hook_id {
        Some(hid) => {
            sqlx::query_as(
                "SELECT coalesce(jq.answer, ''), jq.category, jq.air_date
                 FROM pavlov_hooks h JOIN jeopardy_questions jq ON jq.id = ANY(h.clue_ids)
                 WHERE h.id = $1 AND h.answer_id = $2
                 ORDER BY jq.air_date DESC NULLS LAST LIMIT 1",
            )
            .bind(hid)
            .bind(body.answer_id)
            .fetch_optional(&state.pool)
            .await?
        }
        None => None,
    };

    let examples: Vec<(String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT coalesce(answer, ''), category, air_date FROM jeopardy_questions
         WHERE id = ANY($1) ORDER BY air_date DESC",
    )
    .bind(&example_ids[..])
    .fetch_all(&state.pool)
    .await?;
    let ex_json = |(clue, category, air_date): (String, Option<String>, Option<chrono::NaiveDate>)| {
        json!({ "clue": clue, "category": category, "airDate": air_date })
    };
    Ok(Json(json!({
        "correct": correct, "answer": answer, "answerNorm": answer_norm, "forms": forms,
        "hooks": hooks, "servedHookId": body.hook_id,
        "exampleClue": example_clue.map(ex_json),
        "examples": examples.into_iter().map(ex_json).collect::<Vec<_>>(),
    })))
}
```

`drill_grade`: body gains `pub hook_id: Option<i32>`; validate and cap —

```rust
    // A served hook must belong to the graded entity.
    if let Some(hid) = body.hook_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pavlov_hooks WHERE id = $1 AND answer_id = $2)",
        )
        .bind(hid)
        .bind(body.answer_id)
        .fetch_one(&state.pool)
        .await?;
        if !ok {
            return Err(AppError::BadRequest("hookId does not belong to answerId".into()));
        }
    }
```

(before `let mut tx`), then after `let out = schedule(prev, rating);`:

```rust
    // Coverage guard (spec §3): while a labeled hook other than the one just
    // served is still unseen by this user, cap the interval.
    let has_unseen: bool = sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM pavlov_hooks h
           WHERE h.answer_id = $2 AND h.status = 'active' AND h.cue IS NOT NULL
             AND h.id <> COALESCE($3, -1)
             AND NOT EXISTS (SELECT 1 FROM pavlov_reviews r WHERE r.user_id = $1 AND r.hook_id = h.id))",
    )
    .bind(user_id)
    .bind(body.answer_id)
    .bind(body.hook_id)
    .fetch_one(&mut *tx)
    .await?;
    let (interval_days, interval_secs) = cap_for_coverage(out.interval_days, out.interval_secs, has_unseen);
    let now: DateTime<Utc> = Utc::now();
    let due = now + Duration::seconds(interval_secs);
```

Bind `interval_days` (not `out.interval_days`) in the card upsert, add `hook_id` to the review insert (`INSERT INTO pavlov_reviews (user_id, answer_id, rating, first_grade, reviewed_at, hook_id) VALUES ($1, $2, $3, $4, $5, $6)` binding `body.hook_id`), and return `"intervalDays": interval_days`.

- [ ] **Step 4: Build and test**

Run: `cd backend && cargo build && cargo test`
Expected: builds; `card_json_carries_hook_or_falls_back_to_phrases` and all others pass.

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/pavlov.rs
git commit -m "feat(objects): drill serves one hook per review, returns the hook map on reveal, caps intervals for coverage

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Curation, list, entity-by-question, and hook coverage in stats

**Files:**
- Modify: `backend/src/routes/pavlov.rs` (`answers`, new `hook_drop`, `hook_restore`, `hook_detail`, `entity_by_question`), `backend/src/main.rs` (routes), `backend/src/pavlov_stats.rs`, `backend/src/routes/pavlov_stats.rs`

**Interfaces:**
- Consumes: `hook_map` from Task 8.
- Produces:
  - `POST /api/pavlov/hooks/{id}/drop` → `{ status: "dropped" }`; `POST /api/pavlov/hooks/{id}/restore` → `{ status: "active" }`; 404 when no such hook.
  - `GET /api/pavlov/hooks/{id}` → `{ id, cue, keyGram, grams, examples: [{clue, category, airDate}] (≤ 3) }`.
  - `GET /api/pavlov/entity/question/{id}` → `{ answerId, answer, forms, hooks }` or 204.
  - `GET /api/pavlov/answers` rows gain `forms`, `vetted`, `hooks: [{ id, rank, cue: string|null, keyGram, support, source, status }]` (all hooks, including unlabeled and dropped, ordered by rank then id).
  - `Progress` gains `hooks_seen: i64`, `hooks_total: i64`; `compute_progress(deck_total, touched, created_last_14d, today, target, hooks_seen, hooks_total)`.

- [ ] **Step 1: Write the failing test** (`backend/src/pavlov_stats.rs` tests)

```rust
    #[test]
    fn progress_carries_hook_coverage_through() {
        let p = compute_progress(100, 10, 14, d(2026, 9, 16), d(2026, 12, 31), 12, 40);
        assert_eq!((p.hooks_seen, p.hooks_total), (12, 40));
    }
```

(`d(y, m, day)` is the test module's existing date helper.) Then append `, 0, 0` to every other `compute_progress(` call in the test module.

- [ ] **Step 2: Run it to verify it fails**

Run: `cd backend && cargo test pavlov_stats::`
Expected: compile error — wrong number of arguments.

- [ ] **Step 3: Implement stats**

`backend/src/pavlov_stats.rs`: add to `Progress`

```rust
    /// Labeled hooks this user has seen at least once / labeled hooks on the
    /// entities they have touched (spec §3, "hook coverage").
    pub hooks_seen: i64,
    pub hooks_total: i64,
```

extend the signature with `hooks_seen: i64, hooks_total: i64` and set both fields in the constructor. `backend/src/routes/pavlov_stats.rs`, before `compute_progress`:

```rust
    let (hooks_seen, hooks_total): (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(DISTINCT pr.hook_id) FROM pavlov_reviews pr
             JOIN pavlov_hooks h ON h.id = pr.hook_id
             WHERE pr.user_id = $1 AND h.status = 'active' AND h.cue IS NOT NULL),
           (SELECT count(*) FROM pavlov_hooks h
             JOIN pavlov_cards ca ON ca.answer_id = h.answer_id
             WHERE ca.user_id = $1 AND ca.last_review IS NOT NULL
               AND h.status = 'active' AND h.cue IS NOT NULL)",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let progress = compute_progress(deck_total, touched, created_14d, today, target_date, hooks_seen, hooks_total);
```

- [ ] **Step 4: Curation, detail, entity, list** (`backend/src/routes/pavlov.rs`)

```rust
async fn set_hook_status(state: &Arc<AppState>, id: i32, status: &str) -> Result<Json<Value>, AppError> {
    let n = sqlx::query("UPDATE pavlov_hooks SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound("No such hook".into()));
    }
    Ok(Json(json!({ "status": status })))
}

pub async fn hook_drop(State(state): State<Arc<AppState>>, _auth: AuthUser, Path(id): Path<i32>) -> Result<Json<Value>, AppError> {
    set_hook_status(&state, id, "dropped").await
}

pub async fn hook_restore(State(state): State<Arc<AppState>>, _auth: AuthUser, Path(id): Path<i32>) -> Result<Json<Value>, AppError> {
    set_hook_status(&state, id, "active").await
}

/// Up to three example clues for one hook (list page expansion).
pub async fn hook_detail(State(state): State<Arc<AppState>>, _auth: AuthUser, Path(id): Path<i32>) -> Result<Json<Value>, AppError> {
    let row: Option<(Option<String>, String, Vec<String>, Vec<i32>)> =
        sqlx::query_as("SELECT cue, key_gram, grams, clue_ids FROM pavlov_hooks WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let (cue, key_gram, grams, clue_ids) = row.ok_or_else(|| AppError::NotFound("No such hook".into()))?;
    let examples: Vec<(String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT coalesce(answer, ''), category, air_date FROM jeopardy_questions
         WHERE id = ANY($1) ORDER BY air_date DESC NULLS LAST LIMIT 3",
    )
    .bind(&clue_ids)
    .fetch_all(&state.pool)
    .await?;
    let examples: Vec<Value> = examples
        .into_iter()
        .map(|(clue, category, air_date)| json!({ "clue": clue, "category": category, "airDate": air_date }))
        .collect();
    Ok(Json(json!({ "id": id, "cue": cue, "keyGram": key_gram, "grams": grams, "examples": examples })))
}

/// The hook map for the deck entity a clue resolves to (practice pause).
pub async fn entity_by_question(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(question_id): Path<i32>,
) -> Result<axum::response::Response, AppError> {
    use axum::response::IntoResponse;
    let answer_id: Option<i32> = sqlx::query_scalar(
        "SELECT pa.id FROM jeopardy_questions jq JOIN pavlov_answers pa ON pa.answer_norm = jq.entity_norm
         WHERE jq.id = $1",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(answer_id) = answer_id else {
        return Ok(axum::http::StatusCode::NO_CONTENT.into_response());
    };
    let (answer, forms, hooks) = hook_map(&state, auth.user_id, answer_id).await?;
    Ok(Json(json!({ "answerId": answer_id, "answer": answer, "forms": forms, "hooks": hooks })).into_response())
}
```

In `answers`: extend `AnswerListRow` with `forms: Vec<String>` and `vetted: bool` (add `pa.forms, pa.vetted` to the SELECT), load all hooks once —

```rust
    #[derive(sqlx::FromRow)]
    struct HookListRow { id: i32, answer_id: i32, rank: i32, cue: Option<String>, key_gram: String, support: i32, source: String, status: String }
    let hook_rows: Vec<HookListRow> = sqlx::query_as(
        "SELECT id, answer_id, rank, cue, key_gram, support, source, status FROM pavlov_hooks ORDER BY answer_id, rank, id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut hooks_by_answer: HashMap<i32, Vec<Value>> = HashMap::new();
    for h in hook_rows {
        hooks_by_answer.entry(h.answer_id).or_default().push(json!({
            "id": h.id, "rank": h.rank, "cue": h.cue, "keyGram": h.key_gram,
            "support": h.support, "source": h.source, "status": h.status,
        }));
    }
```

and in the per-row `json!` add `"forms": r.forms, "vetted": r.vetted, "hooks": hooks_by_answer.remove(&r.id).unwrap_or_default(),`. Routes in `main.rs`:

```rust
        .route("/api/pavlov/hooks/{id}", get(routes::pavlov::hook_detail))
        .route("/api/pavlov/hooks/{id}/drop", post(routes::pavlov::hook_drop))
        .route("/api/pavlov/hooks/{id}/restore", post(routes::pavlov::hook_restore))
        .route("/api/pavlov/entity/question/{id}", get(routes::pavlov::entity_by_question))
```

- [ ] **Step 5: Build and test**

Run: `cd backend && cargo build && cargo test`
Expected: builds; all tests pass including `progress_carries_hook_coverage_through`.

- [ ] **Step 6: Commit**

```bash
git add backend/src
git commit -m "feat(objects): hook drop/restore/detail, entity-by-question, list hooks, hook coverage in progress

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: `HookMap.svelte` and the drill page

**Files:**
- Create: `frontend/src/lib/components/HookMap.svelte`
- Modify: `frontend/src/routes/pavlov/+page.svelte` (rewrite the script and the reveal markup)

**Interfaces:**
- Consumes: Task 8's drill payloads.
- Produces: `HookMap` with module-script exports `Hook`, `Entity`, `ExampleClue`; props `{ entity: Entity; servedHookId?: number | null; exampleClue?: ExampleClue | null; dark?: boolean; onDrop?: (id: number) => void }`.

- [ ] **Step 1: Write the component**

```svelte
<!-- frontend/src/lib/components/HookMap.svelte -->
<script lang="ts" module>
  export interface Hook {
    id: number;
    rank: number;
    cue: string;
    support: number;
    source: string;
    seen: number;
    lastWrongAt: string | null;
  }
  export interface Entity {
    answer: string;
    forms: string[];
    hooks: Hook[];
  }
  export interface ExampleClue {
    clue: string;
    category: string | null;
    airDate: string | null;
  }
</script>

<script lang="ts">
  let {
    entity,
    servedHookId = null,
    exampleClue = null,
    dark = true,
    onDrop,
  }: {
    entity: Entity;
    servedHookId?: number | null;
    exampleClue?: ExampleClue | null;
    dark?: boolean;
    onDrop?: (id: number) => void;
  } = $props();

  const muted = $derived(dark ? 'text-white/50' : 'text-gray-400');
  const served = $derived(dark ? 'text-jeopardy-gold font-semibold' : 'text-jeopardy-blue font-semibold');
  const fmt = (iso: string) => new Date(iso).toLocaleDateString([], { month: 'numeric', day: 'numeric' });
</script>

<!-- The entity's hooks: filled dot = seen, hollow = not yet, arrow = served now. -->
<div class="text-left {dark ? 'text-white' : 'text-gray-900'}">
  {#if entity.forms.length > 1}
    <p class="text-xs {muted} mb-1">{entity.forms.join(' · ')}</p>
  {/if}
  {#if entity.hooks.length === 0}
    <p class="text-sm {muted}">No hooks yet for this answer.</p>
  {:else}
    <ul class="space-y-1 text-sm">
      {#each entity.hooks as h (h.id)}
        <li class="flex items-baseline gap-2 {h.id === servedHookId ? served : ''}">
          <span class="w-4 shrink-0 text-center">{h.id === servedHookId ? '▶' : h.seen > 0 ? '●' : '○'}</span>
          <span class="flex-1 min-w-0">{h.cue}</span>
          <span class="text-xs {muted} tabular-nums">{h.support}</span>
          {#if h.lastWrongAt}
            <span class="text-xs {dark ? 'text-red-300' : 'text-red-500'}">✗ {fmt(h.lastWrongAt)}</span>
          {/if}
          {#if onDrop && h.id === servedHookId}
            <button
              onclick={() => onDrop(h.id)}
              title="Drop this hook from the deck (restore on the list page)"
              class="text-xs {muted} hover:text-red-300 border {dark ? 'border-white/20' : 'border-gray-300'} rounded px-1.5"
            >drop <span class="opacity-60">(x)</span></button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if exampleClue}
    <p class="mt-2 text-sm {dark ? 'text-white/80' : 'text-gray-700'}">
      e.g. “{exampleClue.clue}”
      <span class={muted}>({exampleClue.category}{exampleClue.airDate ? `, ${exampleClue.airDate}` : ''})</span>
    </p>
  {/if}
</div>
```

- [ ] **Step 2: Rewrite the drill page script** (`frontend/src/routes/pavlov/+page.svelte`, replace the whole `<script>` block)

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import CountdownTimer from '$lib/components/CountdownTimer.svelte';
  import HookMap, { type Hook, type ExampleClue } from '$lib/components/HookMap.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let card = $state<{
    answerId: number;
    answerNorm: string;
    category: string;
    phrases: Array<{ text: string; tier: string }>;
    hookId: number | null;
    hookRank: number | null;
    cue: string | null;
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
    forms: string[];
    hooks: Hook[];
    servedHookId: number | null;
    exampleClue: ExampleClue | null;
    examples: Array<{ clue: string; category: string | null; airDate: string | null }>;
  } | null>(null);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state('');
  let session = $state({ total: 0, correct: 0 });

  async function fetchNext() {
    loading = true;
    error = '';
    result = null;
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
      result = await api.post('/api/pavlov/drill/check', { answerId: card.answerId, hookId: card.hookId });
    } catch (e: any) {
      error = e.message || 'Reveal failed';
    } finally {
      submitting = false;
    }
  }

  async function grade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post('/api/pavlov/drill/grade', { answerId: card.answerId, rating, hookId: card.hookId });
      session = {
        total: session.total + 1,
        correct: session.correct + (rating === 'wrong' ? 0 : 1),
      };
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Grade failed';
    } finally {
      submitting = false;
    }
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

  // Drop the served hook (deck-level, like banish) and move on without grading.
  async function dropHook(id: number) {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post(`/api/pavlov/hooks/${id}/drop`);
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Drop failed';
    } finally {
      submitting = false;
    }
  }

  // Space/Enter reveals; 1/2/3 self-grade after reveal (honesty mode);
  // b banishes anytime; x drops the served hook after reveal.
  function onKeydown(e: KeyboardEvent) {
    if (!card || submitting || loading) return;
    if (e.key === 'b' || e.key === 'B') {
      e.preventDefault();
      banish();
    } else if (!result && (e.key === ' ' || e.key === 'Enter')) {
      e.preventDefault();
      reveal();
    } else if (result && (e.key === 'x' || e.key === 'X')) {
      e.preventDefault();
      if (card.hookId !== null) dropHook(card.hookId);
    } else if (result) {
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
</script>
```

- [ ] **Step 3: Rewrite the prompt and reveal markup**

Replace the intro paragraph text with:

```svelte
    <p class="text-sm text-gray-500 mb-6">
      One cue → answer. Each review shows a different angle of the same answer; the reveal shows all of them.
      Banish (b) removes a card, x drops the shown angle — undo both on the list page.
      <a href="/pavlov/list" class="text-jeopardy-blue hover:underline">Browse the list →</a>
    </p>
```

Replace the cue block (`<!-- Cue phrases ... -->` through its closing `</div>`) with:

```svelte
        <!-- The cue: one hook's label, or the legacy phrases while an entity has no labeled hook -->
        <div class="flex flex-col items-center justify-center px-6 py-8 gap-3">
          {#if card.cue}
            <span class="px-4 py-2 rounded-full border border-white/25 text-jeopardy-gold text-xl sm:text-2xl font-bold inline-block text-center">{card.cue}</span>
          {:else}
            <div class="flex flex-wrap gap-2 justify-center">
              {#each card.phrases as phrase}
                <span class="px-4 py-2 rounded-full border text-xl sm:text-2xl font-bold inline-block
                  {phrase.tier === 'hint'
                    ? 'border-white/10 text-white/50'
                    : 'border-white/25 text-jeopardy-gold'}">{phrase.text}</span>
              {/each}
            </div>
          {/if}
        </div>
```

Replace everything from `<div class="bg-white rounded-xl px-5 py-4 mb-4 text-center">` through the end of the `{#if result.examples.length > 0} ... {/if}` block with:

```svelte
            <div class="bg-white rounded-xl px-5 py-4 mb-4 text-center">
              <p class="text-gray-900 font-bold text-xl">{result.answer}</p>
            </div>
            <div class="grid grid-cols-3 gap-2">
              <button onclick={() => grade('wrong')} disabled={submitting}
                class="py-3 rounded-xl bg-red-500 hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Wrong</button>
              <button onclick={() => grade('got_it')} disabled={submitting}
                class="py-3 rounded-xl bg-green-500 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Got it</button>
              <button onclick={() => grade('too_easy')} disabled={submitting}
                class="py-3 rounded-xl bg-blue-500 hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Too easy</button>
            </div>
            <p class="mt-2 text-center text-xs text-white/40">1 / 2 / 3</p>
            <div class="mt-4 pt-4 border-t border-white/10">
              {#if result.hooks.length > 0}
                <HookMap
                  entity={{ answer: result.answer, forms: result.forms, hooks: result.hooks }}
                  servedHookId={result.servedHookId}
                  exampleClue={result.exampleClue}
                  onDrop={dropHook}
                />
              {:else if result.examples.length > 0}
                <div class="text-sm text-white/80 space-y-2">
                  {#each result.examples as ex}
                    <p>"{ex.clue}" <span class="text-white/50">({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</span></p>
                  {/each}
                </div>
              {/if}
            </div>
```

- [ ] **Step 4: Check with the mock API** (Task 12 updates the mock routes; until then run the check only)

Run: `cd frontend && npm run check`
Expected: 0 errors (the pre-existing CountdownTimer warning may remain).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/components/HookMap.svelte frontend/src/routes/pavlov/+page.svelte
git commit -m "feat(objects): HookMap component; drill shows one hook cue and the map on reveal, x drops a hook

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: List page — entities, forms, hooks, drop/restore, vetted filter

**Files:**
- Modify: `frontend/src/routes/pavlov/list/+page.svelte` (rewrite)

**Interfaces:**
- Consumes: Task 9's `/api/pavlov/answers`, `/api/pavlov/hooks/{id}`, drop/restore, and the admin status/resolve/hooks endpoints.

- [ ] **Step 1: Rewrite the page**

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });
  let isAdmin = $derived(auth.user?.role === 'admin');

  type Phrase = { text: string; tier: string; support?: number; total?: number; precision?: number };
  type HookRow = {
    id: number; rank: number; cue: string | null; keyGram: string;
    support: number; source: string; status: 'active' | 'dropped';
  };
  type Card = {
    id: number; answer: string; category: string; forms: string[]; vetted: boolean;
    phrases: Phrase[]; suspended: boolean; hooks: HookRow[];
  };
  type Example = { clue: string; category: string | null; airDate: string | null };

  let cards = $state<Card[]>([]);
  let search = $state('');
  let vettedOnly = $state(false);
  let loading = $state(true);
  let error = $state('');
  let expanded = $state<Set<number>>(new Set());
  let examples = $state<Record<number, Example[]>>({});
  let genStatus = $state<{
    running: boolean; pending: number; active: number; dropped: number;
    hooks?: { total: number; labeled: number; unlabeled: number; vetted: number; entitiesPending: number };
  } | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  let filtered = $derived(
    cards.filter((c) => {
      if (vettedOnly && !c.vetted) return false;
      const q = search.trim().toLowerCase();
      if (!q) return true;
      return (
        c.answer.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        c.forms.some((f) => f.toLowerCase().includes(q)) ||
        c.hooks.some((h) => (h.cue ?? h.keyGram).toLowerCase().includes(q)) ||
        c.phrases.some((p) => p.text.toLowerCase().includes(q))
      );
    })
  );
  let grouped = $derived.by(() => {
    const groups: Array<{ category: string; items: Card[] }> = [];
    for (const c of filtered) {
      const last = groups[groups.length - 1];
      if (last && last.category === c.category) last.items.push(c);
      else groups.push({ category: c.category, items: [c] });
    }
    return groups;
  });

  async function load() {
    loading = true;
    try {
      const res = await api.get('/api/pavlov/answers');
      cards = res.answers ?? [];
    } catch (e: any) {
      error = e.message || 'Failed to load';
    } finally {
      loading = false;
    }
  }

  async function toggleSuspend(card: Card) {
    const next = !card.suspended;
    try {
      await api.post(`/api/pavlov/answers/${card.id}/suspend`, { suspended: next });
      card.suspended = next;
    } catch (e: any) {
      error = e.message || 'Suspend failed';
    }
  }

  async function toggleHook(h: HookRow) {
    const action = h.status === 'active' ? 'drop' : 'restore';
    try {
      const res = await api.post(`/api/pavlov/hooks/${h.id}/${action}`);
      h.status = res.status;
    } catch (e: any) {
      error = e.message || 'Hook update failed';
    }
  }

  async function toggleExpand(card: Card) {
    const next = new Set(expanded);
    if (next.has(card.id)) {
      next.delete(card.id);
      expanded = next;
      return;
    }
    next.add(card.id);
    expanded = next;
    for (const h of card.hooks) {
      if (examples[h.id]) continue;
      try {
        const res = await api.get(`/api/pavlov/hooks/${h.id}`);
        examples = { ...examples, [h.id]: res.examples ?? [] };
      } catch {
        examples = { ...examples, [h.id]: [] };
      }
    }
  }

  async function refreshStatus() {
    try {
      genStatus = await api.get('/api/admin/pavlov/status');
      if (genStatus && !genStatus.running && pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
        await load();
      }
    } catch {
      /* non-admin or transient; ignore */
    }
  }

  async function runJob(job: 'generate' | 'resolve' | 'hooks') {
    error = '';
    try {
      await api.post(`/api/admin/pavlov/${job}`);
      await refreshStatus();
      if (!pollTimer) pollTimer = setInterval(refreshStatus, 5000);
    } catch (e: any) {
      error = e.message || `${job} failed`;
    }
  }

  onMount(async () => {
    await load();
    if (isAdmin) await refreshStatus();
  });
  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });
</script>

<svelte:head><title>Pavlov Entities</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-6 sm:py-8 px-4">
  <div class="max-w-4xl mx-auto">
    <div class="flex items-center justify-between mb-2">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Pavlov Entities</h1>
      <a href="/pavlov" class="text-jeopardy-blue hover:underline text-sm">Drill →</a>
    </div>
    <p class="text-sm text-gray-500 mb-6">
      One row per answer, with the angles (hooks) Jeopardy! writers use for it, mined from its own clues.
      ★ marks answers named on the community Pavlov lists. Expand a row to see each hook's example clues; drop hooks or suspend whole answers you don't want in your drill.
    </p>

    {#if isAdmin}
      <div class="mb-4 p-3 rounded-xl border border-gray-200 bg-white shadow-sm flex flex-wrap items-center gap-3 text-sm">
        <button onclick={() => runJob('resolve')} disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors">Resolve entities</button>
        <button onclick={() => runJob('generate')} disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors">Generate cues</button>
        <button onclick={() => runJob('hooks')} disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors">Build hooks</button>
        {#if genStatus}
          <span class="text-gray-500">
            {genStatus.running ? 'running · ' : ''}cues {genStatus.active} active · {genStatus.pending} pending
            {#if genStatus.hooks}
              · hooks {genStatus.hooks.labeled}/{genStatus.hooks.total} labeled · {genStatus.hooks.vetted} vetted · {genStatus.hooks.entitiesPending} entities pending
            {/if}
          </span>
        {/if}
      </div>
    {/if}

    {#if error}
      <div class="mb-4 px-4 py-3 rounded-lg bg-red-50 border border-red-200 text-red-700 text-sm">{error}</div>
    {/if}

    <div class="flex flex-wrap items-center gap-3 mb-6">
      <input type="text" bind:value={search} placeholder="Search answers, forms, hooks, categories…"
        class="flex-1 min-w-[200px] px-3 py-2 rounded-lg bg-white border border-gray-300 text-gray-900 focus:border-jeopardy-blue focus:outline-none focus:ring-1 focus:ring-jeopardy-blue" />
      <label class="flex items-center gap-2 text-sm text-gray-700 cursor-pointer">
        <input type="checkbox" bind:checked={vettedOnly} /> vetted only
      </label>
    </div>

    {#if loading}
      <p class="text-gray-500">Loading…</p>
    {:else if cards.length === 0}
      <p class="text-gray-500">No entities yet{isAdmin ? ' — run Resolve, Generate, then Build hooks above.' : '.'}</p>
    {:else}
      {#each grouped as group}
        <h2 class="text-lg font-semibold mt-6 mb-2 text-jeopardy-blue">
          {group.category} <span class="text-gray-500 text-sm font-normal">({group.items.length})</span>
        </h2>
        <div class="divide-y divide-gray-200 border border-gray-200 rounded-xl bg-white shadow-sm overflow-hidden">
          {#each group.items as card (card.id)}
            <div class="p-3 {card.suspended ? 'opacity-40' : ''}">
              <div class="flex items-start gap-3">
                <button onclick={() => toggleExpand(card)} class="text-gray-400 hover:text-jeopardy-blue w-5 shrink-0 text-left" title="Show example clues">
                  {expanded.has(card.id) ? '▾' : '▸'}
                </button>
                <div class="flex-1 min-w-0">
                  <div class="text-gray-900">
                    {#if card.vetted}<span class="text-jeopardy-gold mr-1" title="On the community Pavlov lists">★</span>{/if}
                    <span class="font-semibold">{card.answer}</span>
                    {#if card.forms.length > 1}
                      <span class="text-xs text-gray-400 ml-2">{card.forms.filter((f) => f !== card.answer).join(' · ')}</span>
                    {/if}
                  </div>
                  <ul class="mt-1 space-y-0.5 text-sm">
                    {#each card.hooks as h (h.id)}
                      <li class="flex items-baseline gap-2 {h.status === 'dropped' ? 'opacity-40 line-through' : ''}">
                        <span class="text-gray-400 w-4 text-right tabular-nums">{h.rank}</span>
                        {#if h.cue}
                          <span class="flex-1 min-w-0">{h.cue}</span>
                        {:else}
                          <span class="flex-1 min-w-0 text-gray-400 italic">unlabeled · {h.keyGram}</span>
                        {/if}
                        <span class="text-xs text-gray-400">{h.support} · {h.source}</span>
                        <button onclick={() => toggleHook(h)} class="text-xs text-gray-500 hover:text-jeopardy-blue">
                          {h.status === 'active' ? 'drop' : 'restore'}
                        </button>
                      </li>
                      {#if expanded.has(card.id) && examples[h.id]}
                        {#each examples[h.id] as ex}
                          <li class="pl-6 text-xs text-gray-500">“{ex.clue}” ({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</li>
                        {/each}
                      {/if}
                    {/each}
                    {#if card.hooks.length === 0 && card.phrases.length > 0}
                      <li class="text-gray-500">
                        {#each card.phrases as phrase, i}
                          {#if i > 0}<span class="text-gray-400 mx-1">·</span>{/if}
                          <span class={phrase.tier === 'hint' ? 'text-gray-400' : ''}>{phrase.text}</span>
                        {/each}
                        <span class="text-xs text-gray-400 ml-1">(legacy cues — no hooks yet)</span>
                      </li>
                    {/if}
                  </ul>
                </div>
                <button onclick={() => toggleSuspend(card)}
                  class="text-xs px-2 py-1 rounded-lg border border-gray-300 hover:border-jeopardy-blue shrink-0 text-gray-700 transition-colors">
                  {card.suspended ? 'Unsuspend' : 'Suspend'}
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
```

- [ ] **Step 2: Check**

Run: `cd frontend && npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/routes/pavlov/list/+page.svelte
git commit -m "feat(objects): list page shows entities, forms, hooks with drop/restore, vetted filter, admin jobs

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: Practice pause, settings, progress card, types, mock API; delete `AnswerSheet`

**Files:**
- Delete: `frontend/src/lib/components/AnswerSheet.svelte`
- Modify: `frontend/src/routes/practice/+page.svelte`, `frontend/src/routes/settings/+page.svelte`, `frontend/src/lib/components/PavlovProgressCard.svelte`, `frontend/src/lib/stats.ts`, `scripts/dev-mock-api.mjs`

**Interfaces:**
- Consumes: `GET /api/pavlov/entity/question/{id}` (204 or entity), `progress.hooksSeen` / `progress.hooksTotal`.

- [ ] **Step 1: Practice page — hook map in the teaching pause**

Replace the `AnswerSheet` import with `import HookMap, { type Entity } from '$lib/components/HookMap.svelte';`. Replace the four sheet state lines with:

```ts
  let entity = $state<Entity | null>(null);
  let entityLoading = $state(false);
  let entityGen = 0; // guards against a stale fetch resolving after a newer question/pause
```

Everywhere the file resets `sheet = null; sheetAddedNow = 0; sheetGen++;` (in `fetchQuestion` twice and `advanceFromPause`) write `entity = null; entityGen++;` instead. In `handleGrade` call `fetchEntity(question.id)` instead of `fetchSheet(question.id)`. Replace `fetchSheet` and delete `addSheetFacts`:

```ts
  async function fetchEntity(questionId: number) {
    const gen = ++entityGen;
    entity = null;
    entityLoading = true;
    try {
      const res = await api.get(`/api/pavlov/entity/question/${questionId}`);
      if (gen !== entityGen) return; // superseded
      entity = res; // null on 204 (api.get returns null for empty bodies)
    } catch {
      if (gen !== entityGen) return;
      entity = null;
    } finally {
      if (gen === entityGen) entityLoading = false;
    }
  }
```

In the `pausePanel` snippet replace the `{#if sheetLoading || sheet} <AnswerSheet .../> {/if}` block with:

```svelte
              {#if entity}
                <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3">
                  <p class="text-xs uppercase tracking-wide text-white/50 mb-1">In your Pavlov deck · {entity.answer}</p>
                  <HookMap {entity} />
                </div>
              {/if}
```

(`entityLoading` is intentionally not rendered: the pause does not wait for it.)

- [ ] **Step 2: Settings — remove the auto-facts checkbox**

Delete `let pavlovAutoFacts = $state(false);`, the `pavlovAutoFacts = prefs?.pavlovAutoFacts ?? false;` line, the `pavlovAutoFacts,` entry in the save payload, and the whole `<label class="flex items-center gap-2 ..."> ... Auto-add fact cards ... </label>` block.

- [ ] **Step 3: Progress card and types**

`frontend/src/lib/stats.ts` — add to `PavlovProgress`:

```ts
  hooksSeen: number;
  hooksTotal: number;
```

`frontend/src/lib/components/PavlovProgressCard.svelte` — after the pace paragraph's closing `{/if}` (just before the final `</div>`), add:

```svelte
  {#if progress.hooksTotal > 0}
    <p class="text-xs text-gray-400 mt-1">
      Hook coverage <span class="font-semibold text-gray-600">{progress.hooksSeen.toLocaleString()} / {progress.hooksTotal.toLocaleString()}</span>
      · {Math.round((progress.hooksSeen / progress.hooksTotal) * 100)}% of the angles on cards you've touched
    </p>
  {/if}
```

- [ ] **Step 4: Mock API**

In `scripts/dev-mock-api.mjs` replace the five Pavlov/sheet routes (`/api/pavlov/drill/next` … `/api/sheet/answer/...`) with:

```js
  '/api/pavlov/drill/next': { done: false, isNew: false, dueCount: 124, newRemaining: 40, card: { answerId: 1, answerNorm: 'visigoths', category: 'History & Politics', phrases: [], hookId: 3, hookRank: 3, cue: 'ruled Spain from Toledo until 711' } },
  '/api/pavlov/drill/check': { correct: null, answer: 'the Visigoths', answerNorm: 'visigoths', forms: ['the Visigoths', 'Visigoths'], servedHookId: 3,
    hooks: [
      { id: 1, rank: 1, cue: 'split from the Ostrogoths, "western" Goths', support: 9, source: 'cue', seen: 2, lastWrongAt: null },
      { id: 2, rank: 2, cue: 'Alaric sacks Rome, 410', support: 5, source: 'cue', seen: 1, lastWrongAt: isoDay(-1) + 'T18:00:00Z' },
      { id: 3, rank: 3, cue: 'ruled Spain from Toledo until 711', support: 6, source: 'model', seen: 0, lastWrongAt: null },
      { id: 4, rank: 4, cue: 'Alaric II · Vouillé · Franks, 507', support: 2, source: 'model', seen: 0, lastWrongAt: null },
    ],
    exampleClue: { clue: 'In 711 a Muslim army defeated Roderick, the last king of these people in Spain', category: 'VICTORY IS OURS', airDate: '2004-09-24' },
    examples: [] },
  '/api/pavlov/drill/grade': { state: 'learning', due: new Date().toISOString(), intervalDays: 0, requeueInSession: true },
  '/api/pavlov/hooks/3/drop': { status: 'dropped' },
  '/api/pavlov/entity/question/1': { answerId: 1, answer: 'the Visigoths', forms: ['the Visigoths', 'Visigoths'], hooks: [
      { id: 1, rank: 1, cue: 'split from the Ostrogoths, "western" Goths', support: 9, source: 'cue', seen: 2, lastWrongAt: null },
      { id: 3, rank: 3, cue: 'ruled Spain from Toledo until 711', support: 6, source: 'model', seen: 0, lastWrongAt: null } ] },
  '/api/pavlov/answers': { answers: [
    { id: 1, answer: 'the Visigoths', category: 'History & Politics', forms: ['the Visigoths', 'Visigoths'], vetted: false, suspended: false, phrases: [],
      hooks: [ { id: 1, rank: 1, cue: 'split from the Ostrogoths, "western" Goths', keyGram: 'ostrogoth', support: 9, source: 'cue', status: 'active' },
               { id: 3, rank: 3, cue: 'ruled Spain from Toledo until 711', keyGram: '711', support: 6, source: 'model', status: 'active' },
               { id: 5, rank: 5, cue: null, keyGram: 'roman', support: 2, source: 'mined', status: 'active' } ] },
    { id: 2, answer: 'Jean Sibelius', category: 'Music & Performing Arts', forms: ['(Jean) Sibelius', 'Jean Sibelius', 'Sibelius'], vetted: true, suspended: false, phrases: [],
      hooks: [ { id: 7, rank: 1, cue: 'Finnish composer of "Finlandia"', keyGram: 'finlandia', support: 21, source: 'both', status: 'active' } ] },
  ] },
  '/api/pavlov/hooks/1': { id: 1, cue: 'split from the Ostrogoths, "western" Goths', keyGram: 'ostrogoth', grams: ['ostrogoth', 'goth split'], examples: [ { clue: 'Circa 370 A.D., the Goths split into 2 tribes, the Ostrogoths & these people', category: 'ANCIENT TIMES', airDate: '1987-10-14' } ] },
  '/api/pavlov/hooks/3': { id: 3, cue: 'ruled Spain from Toledo until 711', keyGram: '711', grams: ['711', 'spain'], examples: [ { clue: 'In 711 a Muslim army defeated Roderick, the last king of these people in Spain', category: 'VICTORY IS OURS', airDate: '2004-09-24' } ] },
  '/api/pavlov/hooks/5': { id: 5, cue: null, keyGram: 'roman', grams: ['roman'], examples: [] },
  '/api/pavlov/hooks/7': { id: 7, cue: 'Finnish composer of "Finlandia"', keyGram: 'finlandia', grams: ['finlandia', 'finnish compos'], examples: [] },
  '/api/admin/pavlov/status': { running: false, pending: 0, active: 13062, dropped: 900, hooks: { total: 14200, labeled: 13100, unlabeled: 1100, vetted: 610, entitiesPending: 0 } },
```

and add `hooksSeen: 0, hooksTotal: 0` to the `MOCK_EMPTY` progress object and `hooksSeen: 1830, hooksTotal: 3400` to the other one. The mock server matches on exact path and ignores the HTTP method, so the `POST` routes above need nothing extra.

- [ ] **Step 5: Delete the sheet component, check, build, screenshots**

```bash
git rm frontend/src/lib/components/AnswerSheet.svelte
cd frontend && npm run check && npm run build && cd .. && git checkout -- frontend/build
```

Expected: 0 errors; build succeeds. Then start `node scripts/dev-mock-api.mjs` and `cd frontend && VITE_API_PROXY=http://127.0.0.1:3999 npm run dev`, and take Playwright screenshots (viewport, not fullPage) of: `/pavlov` after pressing Space (hook map with ▶ on hook 3 and ✗ on hook 2), `/pavlov/list` with row 1 expanded, `/dashboard` progress card showing "Hook coverage 1,830 / 3,400", at 1280 and 390 wide. Save them under `.playwright-mcp/` (untracked).

- [ ] **Step 6: Commit**

```bash
git add frontend/src scripts/dev-mock-api.mjs
git commit -m "feat(objects): practice pause shows the hook map; remove auto-facts setting and AnswerSheet; hook coverage on the progress card; mock routes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Rollout (by hand, after the branch is merged to main and pushed over SSH)

1. `scripts/apply-migration.sh backend/migrations/0016_jeopardy_objects.sql`
2. Wait for Watchtower (≤ 5 min); `curl -s https://<host>/api/health`.
3. As admin on `/pavlov/list`: **Resolve entities** → wait for `running: false`.
4. Run `scripts/verify-objects.sql`; section A must be 0; read sections B (report `in_deck`) and C (scan the 50 merges for false positives).
5. **Generate cues** (mines new entities such as Grieg; renders only new pending cues with gpt-4o).
6. **Build hooks** (mines, imports vetted, labels; cost: a few dollars of gpt-4o-mini).
7. Re-run the verify script; section D must be all zeros — `undrillable_entities` in particular; sections E and F for a sanity read.
8. Drill one card on `/pavlov`, confirm the cue and the reveal map, press `x` on a hook, restore it on the list page.

## Self-review notes

- Spec coverage: §1 → Tasks 1, 2, 5; §2 → Tasks 3, 4, 6; §3 → Tasks 8, 9; §4 → Tasks 10, 11, 12; §5 → Tasks 1, 7, rollout above. The practice hook map needs an endpoint the spec's API table did not list; Task 9 adds `GET /api/pavlov/entity/question/{id}`.
- Type consistency: `ServedHook` (Task 8) is the only new struct crossing task lines inside `routes/pavlov.rs`; `HookExposure`, `cap_for_coverage`, `pick_hook` names match between Tasks 4 and 8; `Hook` / `Entity` / `ExampleClue` match between Tasks 10, 11, 12; `hooksSeen` / `hooksTotal` match between Tasks 9 and 12.
- The `hooks_built_at` and `label_attempted_at` columns and the `pavlov_gram_df` table are not in the spec's DDL; they exist for resumability (spec §5 requires resumable jobs).
