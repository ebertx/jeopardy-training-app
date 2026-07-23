# Training Optimization Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Six features optimizing practice for expected Anytime Test points: test-value-weighted adaptive targeting, Pavlov throughput controls + category-weighted introduction, a display-only 8s countdown, a projected-mock-score tile, mock miss classification, and vocab drill presets.

**Architecture:** Pure math lands in `adaptive.rs`/`blend.rs` with TDD; migration 0012 adds `users.pavlov_new_per_day` and `question_attempts.miss_kind`; route changes are localized (pavlov drill pick, stats payload, one new mock endpoint, preferences passthrough); frontend adds one `CountdownTimer` component reused by two pages, plus tile/tags/presets/settings edits.

**Tech Stack:** Rust (axum, sqlx), Postgres 15+, SvelteKit (Svelte 5 runes, Tailwind).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-23-training-optimization-bundle-design.md`.
- The countdown is DISPLAY-ONLY: 8 seconds, ticks to 0, sits at 0; no auto-reveal/grade, no logging, no sound. Stops on reveal; resets per card.
- Adaptive weights = smoothed weakness × normalized test share (`blend::TARGET_WEIGHTS`); unknown categories get share floor 0.02; weights still sum to 1.
- Projected mock: per-meta-category cold accuracy × share × 50, no-data categories at 0.5 flagged estimated; response also carries per-category `headroom = share × (1 − acc) × 50`.
- Pavlov introduction: sample category by TARGET_WEIGHTS restricted to categories with unseen cards (renormalized), then the existing evidence race within it; fall back to unfiltered on empty. Allowance source becomes `users.pavlov_new_per_day` (default 20) in `drill_next` AND `status_user`; extra-mode semantics unchanged.
- `miss_kind` CHECK ('unknown','slow','wording'), NULL default; tagging endpoint is idempotent re-tag; add-misses-to-SRS unchanged.
- Migrations manual: `scripts/apply-migration.sh backend/migrations/0012_training_bundle.sql`. Tests `cd backend && cargo test`; frontend `cd frontend && npm run build` then discard `build/` churn.
- Commit per task with the repo's Claude Code trailer.

---

### Task 1: Migration 0012

**Files:**
- Create: `backend/migrations/0012_training_bundle.sql`

- [ ] **Step 1: Write it**

```sql
-- 0012: training optimization bundle — pavlov throughput setting + mock miss tags
-- (docs/superpowers/specs/2026-07-23-training-optimization-bundle-design.md). Idempotent.

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS pavlov_new_per_day INTEGER NOT NULL DEFAULT 20;

ALTER TABLE question_attempts
  ADD COLUMN IF NOT EXISTS miss_kind TEXT
    CHECK (miss_kind IN ('unknown', 'slow', 'wording'));
```

- [ ] **Step 2: Apply + verify**

Run: `scripts/apply-migration.sh backend/migrations/0012_training_bundle.sql` then
`docker run --rm -i postgres:16 psql "$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')" -c "SELECT pavlov_new_per_day FROM users LIMIT 1" -c "\d question_attempts" | grep miss_kind`
Expected: default 20 visible; `miss_kind` column present.

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/0012_training_bundle.sql
git commit -m "feat(training): migration 0012 — pavlov_new_per_day, mock miss_kind"
```

---

### Task 2: Test-value-weighted adaptive targeting (TDD)

**Files:**
- Modify: `backend/src/adaptive.rs`
- Modify: `backend/src/blend.rs` (one helper)

**Interfaces:**
- Produces: `blend::test_share(category: &str) -> f64` (normalized fraction of TARGET_WEIGHTS; 0.02 floor for unknown categories); `compute_weights` output now test-share-scaled.

- [ ] **Step 1: Failing tests**

In `backend/src/blend.rs` tests:

```rust
    #[test]
    fn test_share_normalizes_and_floors() {
        assert!((test_share("Literature & Language") - 0.20).abs() < 1e-9);
        assert!((test_share("Sports & Games") - 0.02).abs() < 1e-9);
        assert!((test_share("No Such Category") - 0.02).abs() < 1e-9);
    }
```

In `backend/src/adaptive.rs` tests (reusing its existing test helpers/style — read the module's current tests first):

```rust
    #[test]
    fn equal_weakness_higher_test_share_wins() {
        // Same attempts/correct in both categories; Literature (20%) must
        // out-weight Sports (2%).
        let stats = vec![
            CategoryStat { category: "Literature & Language".into(), attempts: 50, correct: 25 },
            CategoryStat { category: "Sports & Games".into(), attempts: 50, correct: 25 },
        ];
        let w = compute_weights(&stats);
        let lit = w.iter().find(|x| x.category.starts_with("Lit")).unwrap().weight;
        let spo = w.iter().find(|x| x.category.starts_with("Spo")).unwrap().weight;
        assert!(lit > spo * 5.0, "lit {lit} vs sports {spo}");
        let sum: f64 = w.iter().map(|x| x.weight).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
```

- [ ] **Step 2: See them fail** — `cd backend && cargo test blend:: adaptive::` (compile error / assertion).

- [ ] **Step 3: Implement**

In `blend.rs`:

```rust
/// Normalized Anytime Test share of a meta-category (TARGET_WEIGHTS sums to
/// 100). Unknown categories get a 0.02 floor so nothing zeroes out.
pub fn test_share(category: &str) -> f64 {
    TARGET_WEIGHTS
        .iter()
        .find(|(c, _)| *c == category)
        .map(|(_, w)| *w as f64 / 100.0)
        .unwrap_or(0.02)
        .max(0.02)
}
```

In `adaptive.rs` `compute_weights`, scale the raw weakness before normalizing:

```rust
    let raw: Vec<f64> = stats
        .iter()
        .map(|s| {
            let smoothed = (s.correct as f64 + PRIOR_PSEUDO_COUNT * global_acc)
                / (s.attempts as f64 + PRIOR_PSEUDO_COUNT);
            // Weakness × Anytime Test share: attention follows expected test
            // points, not raw weakness (spec 2026-07-23 §1).
            (1.0 - smoothed).max(0.0) * crate::blend::test_share(&s.category)
        })
        .collect();
```

- [ ] **Step 4: All green** — `cd backend && cargo test` (existing adaptive tests may assert old proportions — update any that break, preserving their intent and noting it).

- [ ] **Step 5: Commit** — `feat(training): adaptive targeting weighted by Anytime Test share`

---

### Task 3: Projected mock score (TDD + stats wiring)

**Files:**
- Modify: `backend/src/blend.rs` (pure fn + tests)
- Modify: `backend/src/routes/stats.rs`

**Interfaces:**
- Produces: `blend::projected_mock(cold: &[(String, i64, i64)]) -> serde_json::Value` — input (meta_category, cold_total, cold_correct); output `{score, passLine: 35, categories: [{category, share, coldAccuracy, contribution, headroom, estimated}]}` sorted by headroom desc. `/api/stats` gains `projectedMock`.

- [ ] **Step 1: Failing tests** (blend.rs)

```rust
    #[test]
    fn projected_mock_math_and_neutral_fallback() {
        let cold = vec![
            ("Literature & Language".to_string(), 100_i64, 43_i64),
            ("Geography & Exploration".to_string(), 0, 0), // no data -> 0.5 estimated
        ];
        let v = projected_mock(&cold);
        // Only categories in TARGET_WEIGHTS contribute; absent ones count at 0.5.
        let score = v["score"].as_f64().unwrap();
        // Lit: .20*.43*50 = 4.3; Geo: .14*.5*50 = 3.5 (estimated); all other
        // 11 categories: share*.5*50. Total shares = 1.0 → others = (1-.34)*.5*50 = 16.5.
        assert!((score - (4.3 + 3.5 + 16.5)).abs() < 0.01, "score {score}");
        assert_eq!(v["passLine"], 35);
        let cats = v["categories"].as_array().unwrap();
        assert_eq!(cats.len(), 13);
        let geo = cats.iter().find(|c| c["category"] == "Geography & Exploration").unwrap();
        assert_eq!(geo["estimated"], true);
        // Sorted by headroom desc: Literature (.20*.57*50=5.7) must be first.
        assert_eq!(cats[0]["category"], "Literature & Language");
    }
```

- [ ] **Step 2: Fail** — `cargo test blend::`.

- [ ] **Step 3: Implement** (blend.rs)

```rust
/// Projected Anytime Test score from per-category cold accuracy × test share.
/// Categories without cold data count at a neutral 0.5 and are flagged.
pub fn projected_mock(cold: &[(String, i64, i64)]) -> serde_json::Value {
    let mut cats: Vec<serde_json::Value> = Vec::with_capacity(TARGET_WEIGHTS.len());
    let mut score = 0.0f64;
    for (name, w) in TARGET_WEIGHTS.iter() {
        let share = *w as f64 / 100.0;
        let row = cold.iter().find(|(c, _, _)| c == name);
        let (acc, estimated) = match row {
            Some((_, total, correct)) if *total > 0 => (*correct as f64 / *total as f64, false),
            _ => (0.5, true),
        };
        let contribution = share * acc * 50.0;
        let headroom = share * (1.0 - acc) * 50.0;
        score += contribution;
        cats.push(serde_json::json!({
            "category": name, "share": share, "coldAccuracy": acc,
            "contribution": contribution, "headroom": headroom, "estimated": estimated,
        }));
    }
    cats.sort_by(|a, b| {
        b["headroom"].as_f64().unwrap_or(0.0)
            .partial_cmp(&a["headroom"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    serde_json::json!({ "score": score, "passLine": 35, "categories": cats })
}
```

- [ ] **Step 4: Wire into stats** — in `routes/stats.rs`, the handler already aggregates per-category cold totals for `categoryBreakdown` (read it first). Feed `(classifier_category, coldTotal, coldCorrect)` triples into `blend::projected_mock` and add `"projectedMock": ...` to the response JSON.

- [ ] **Step 5: Green + commit** — `cargo test`; `feat(training): projected mock score in /api/stats`

---

### Task 4: Pavlov throughput + category-weighted introduction

**Files:**
- Modify: `backend/src/routes/pavlov.rs`
- Modify: `backend/src/routes/preferences.rs` (passthrough — read it first, mirror `new_cards_per_day` handling for `pavlov_new_per_day` in both GET and PUT)

**Interfaces:**
- `drill_next` + `status_user` read `pavlov_new_per_day` instead of `new_cards_per_day`.
- New-card pick: category sampled by TARGET_WEIGHTS over categories with unseen cards, then evidence race within; unfiltered fallback.

- [ ] **Step 1: Implement the two-step pick**

In `drill_next`, replace the `pick_new` usage with a helper (same file):

```rust
/// Category-weighted new-card pick: sample a meta-category by Anytime Test
/// share (restricted to categories that still have unseen cards for this
/// user, renormalized by the race), then the evidence race within it. Falls
/// back to the unfiltered race when the sampled category comes up empty.
async fn pick_new_card(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<Option<DrillAnswerRow>, AppError> {
    let available: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT meta_category FROM pavlov_answers
         WHERE id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let avail: Vec<String> = available.into_iter().map(|(c,)| c).collect();
    let weights = crate::blend::target_weights(&avail);
    let total: i64 = weights.iter().map(|(_, w)| w).sum();
    let picked_cat = if total > 0 {
        use rand::Rng;
        let mut roll = rand::rng().random_range(0..total);
        weights.iter().find(|(_, w)| { if roll < *w { true } else { roll -= w; false } })
            .map(|(c, _)| c.clone())
    } else {
        None
    };

    const PICK_IN_CAT: &str = "SELECT id, phrases, phrase_tiers, meta_category FROM pavlov_answers
         WHERE meta_category = $2
           AND id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY -ln(random()) / ln(1 + score) LIMIT 1";
    const PICK_ANY: &str = "SELECT id, phrases, phrase_tiers, meta_category FROM pavlov_answers
         WHERE id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY -ln(random()) / ln(1 + score) LIMIT 1";

    if let Some(cat) = picked_cat {
        if let Some(row) = sqlx::query_as::<_, DrillAnswerRow>(PICK_IN_CAT)
            .bind(user_id)
            .bind(&cat)
            .fetch_optional(&state.pool)
            .await?
        {
            return Ok(Some(row));
        }
    }
    Ok(sqlx::query_as::<_, DrillAnswerRow>(PICK_ANY)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?)
}
```

Replace both `pick_new` call sites in `drill_next` with `pick_new_card(&state, user_id).await?` (delete the old `pick_new` const). Change the prefs query in `drill_next` AND `status_user` to `SELECT pavlov_new_per_day, timezone FROM users WHERE id = $1`.

- [ ] **Step 2: Preferences passthrough** — mirror `new_cards_per_day` in `preferences.rs` GET/PUT for `pavlov_new_per_day` (clamp to the same bounds the existing field uses; read the file for its validation pattern).

- [ ] **Step 3: Green + commit** — `cargo test`; `feat(training): pavlov_new_per_day + test-share-weighted card introduction`

---

### Task 5: Mock miss classification endpoint

**Files:**
- Modify: `backend/src/routes/mock_test.rs`
- Modify: `backend/src/main.rs` (route)

- [ ] **Step 1: Handler** — read `override_verdict` first and mirror exactly how it locates the attempt row for (test, question); then:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissKindBody {
    pub question_id: i32,
    pub miss_kind: String,
}

/// Tag a mock miss: unknown | slow | wording. Idempotent re-tag. Informational
/// only — does not alter verdicts, stats, or add-misses behavior.
pub async fn set_miss_kind(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
    Json(body): Json<MissKindBody>,
) -> Result<Json<Value>, AppError> {
    if !matches!(body.miss_kind.as_str(), "unknown" | "slow" | "wording") {
        return Err(AppError::BadRequest("missKind must be unknown|slow|wording".into()));
    }
    // Ownership + membership check and attempt-row targeting: same pattern as
    // override_verdict (verify the test belongs to auth.user_id and the
    // question belongs to the test before updating).
    // ... (mirror override_verdict's row lookup; then:)
    // UPDATE question_attempts SET miss_kind = $ ... for that attempt row.
    Ok(Json(json!({ "questionId": body.question_id, "missKind": body.miss_kind })))
}
```

(The elided lookup MUST be copied from `override_verdict`'s working logic — same bind order, same 404/403 behavior; this is deliberate reuse of proven row targeting, not a placeholder for new design.)

- [ ] **Step 2: Route** — `.route("/api/mock-test/{id}/miss-kind", post(routes::mock_test::set_miss_kind))` next to the override route. Also extend the `results` handler's per-question rows with `missKind` (read from the same attempt rows it already joins).

- [ ] **Step 3: Green + commit** — `cargo test`; `feat(training): mock miss-kind tagging endpoint`

---

### Task 6: Frontend — timer component, settings, dashboard tile, mock tags, drill presets

**Files:**
- Create: `frontend/src/lib/components/CountdownTimer.svelte`
- Modify: `frontend/src/routes/pavlov/+page.svelte`, `frontend/src/routes/practice/+page.svelte`, `frontend/src/routes/settings/+page.svelte`, `frontend/src/routes/dashboard/+page.svelte`, `frontend/src/routes/mock/+page.svelte` (results view — find where results render; may be a subroute), `frontend/src/routes/drill/+page.svelte`

- [ ] **Step 1: Timer component**

```svelte
<script lang="ts">
  // Display-only countdown: ticks to 0 and sits there. No callbacks, no
  // logging — the user self-scores against it (spec 2026-07-23 §3).
  let { seconds = 8, running = true, resetKey = 0 }: { seconds?: number; running?: boolean; resetKey?: unknown } = $props();
  let remaining = $state(seconds);
  $effect(() => {
    void resetKey; // re-run on card change
    remaining = seconds;
    if (!running) return;
    const iv = setInterval(() => {
      remaining = Math.max(0, remaining - 1);
      if (remaining === 0) clearInterval(iv);
    }, 1000);
    return () => clearInterval(iv);
  });
</script>

<span class="text-xs tabular-nums {remaining === 0 ? 'text-red-300' : 'text-gray-400'}">{remaining}s</span>
```

- [ ] **Step 2: Mount it** — Pavlov card header row (next to Banish) with `resetKey={card.answerId}` and `running={!result}`; Practice card header with `resetKey={question.id}` and `running={!showAnswer}` (match that page's reveal flag — read it). Nothing else.
- [ ] **Step 3: Settings** — add "Pavlov new cards/day" numeric field mirroring the existing new-cards-per-day field (read settings page for the exact pattern, wire through `/api/preferences`).
- [ ] **Step 4: Dashboard tile** — from the already-fetched `/api/stats` `projectedMock`: card showing `score.toFixed(1)`/50 vs pass line 35 (green ≥35, amber 30–35, red <30), plus top-3 headroom categories as "+X.X pts available" rows.
- [ ] **Step 5: Mock results tags** — on each missed question row: three small buttons (Didn't know / Knew, too slow / Wording) posting `/api/mock-test/{id}/miss-kind`; selected state highlighted from `missKind` in results payload; small breakdown line in the results header when any tags set.
- [ ] **Step 6: Drill presets** — on the drill page near the search box: preset chips "Word origins", "Vocab" setting the search input to `from the greek OR from the latin OR word meaning` / `this word means OR is the word for` and triggering the existing search.
- [ ] **Step 7: Build + commit** — `cd frontend && npm run build` clean, discard `build/` churn; `feat(training): countdown timer, settings field, projected tile, miss tags, vocab presets`

---

### Task 7: E2E + deploy (controller)

- [ ] `cargo test` + frontend build clean.
- [ ] Local backend + temp user: verify adaptive weights shift (weights endpoint via practice/status), projectedMock in /api/stats, pavlov pick honors category weighting (sample ~20 draws, expect Lit-heavy vs Geo), pavlov_new_per_day round-trip via preferences, miss-kind tag round-trip.
- [ ] Browser QA: timer counts/stops/resets and does nothing at 0 (both pages); settings field; dashboard tiles; mock miss tags; drill presets.
- [ ] Final review (capable model) + fix wave; merge, push (ebertx), CI, Tower deploy, changelog, ledger; post-deploy content op: generate 3–5 vocab primers via /primers.
