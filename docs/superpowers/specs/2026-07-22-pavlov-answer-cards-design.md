# Pavlov v2.1 — Per-Answer Cards with Hint Tier

**Date:** 2026-07-22
**Status:** Approved
**Amends:** `2026-07-22-pavlov-cues-v2-design.md` (mining/evidence model retained;
card unit and display change).

## Why

First live use of v2's one-phrase-per-card drill: a single cue is too vague
for many cards ("current, borne" → The Great Gatsby is unanswerable alone;
"current, borne · Nick Carraway · green light" is triangulable). 59% of
answers (2,904 of 4,916) have only one default-tier cue, so merging siblings
alone cannot fix it. Measured: at the loose tier (bigram support ≥ 3 /
prec ≥ 0.4; unigram ≥ 5 / ≥ 0.5), 1,554 of those 2,904 single-cue answers
gain at least one extra phrase. Decision (user-approved): revert the card
unit to **one card per answer** showing up to 3 phrases, topped up from a
display-only **hint tier**.

## 1. Schema (migration 0011)

- `pavlov_cues` gains `tier TEXT NOT NULL DEFAULT 'standard'
  CHECK (tier IN ('standard', 'hint'))`. Existing rows stay `standard`;
  their renders are reused — nothing already rendered is re-rendered.
- New `pavlov_answers` (the card table, denormalized at generation):
  `id`, `answer_norm UNIQUE`, `answer` (display), `meta_category`,
  `phrases TEXT[]` (1–3 display forms, standard before hint, by score),
  `score REAL` (max support × prec over its standard cues),
  `example_clue_ids INT[]`, `created_at`.
- `pavlov_cards`: `cue_id` column replaced by
  `answer_id → pavlov_answers(id) ON DELETE CASCADE`,
  `UNIQUE (user_id, answer_id)`; all rows deleted (drill-state reset —
  approved; ~1 day of v2 state).

## 2. Hint-tier mining (incremental)

- Hint thresholds: bigrams `support >= 3 AND prec >= 0.4`; unigrams
  `support >= 5 AND prec >= 0.5` — but a candidate already qualifying at the
  default tier is `standard`, and hint mining only runs for answers with
  **fewer than 3 standard cues**, keeping at most `3 − standard_count` best
  hint candidates per answer (score = support × prec).
- Hint candidates pass the same pipeline as standard: `phrase_leaks_answer`
  filter, `prune_redundant` (pruned against the answer's standard cues AND
  other hint candidates), insert as `pending` with `tier = 'hint'`,
  LLM-rendered by the existing render stage (~4–5k extra renders expected).
- Hint cues never become cards on their own; they exist only to fill
  `pavlov_answers.phrases`.

## 3. Card assembly (new generation stage C)

After render completes, rebuild `pavlov_answers` from active cues:

- One row per `answer_norm` having ≥ 1 active `standard` cue (hint-only
  answers get no card).
- `phrases` = up to 3 active cue displays, ordered standard-first then
  score desc.
- `score` = max(support × prec) over the answer's active standard cues.
- `example_clue_ids` = union of the chosen phrases' example ids, 3 most
  recent kept.
- Rebuild is idempotent (delete + reinsert per answer, or full truncate +
  rebuild — it is derived data). Runs inside `run_generation` after the
  render stage.

## 4. Drill + browse

- Drill card front: 2–3 phrase chips (hint chips visually dimmed) +
  meta-category chip. API card shape: `{answerId, phrases: [{text, tier}],
  category}`.
- Reveal (`drill_check`): unchanged contract, keyed by `answerId`; returns
  the answer + example clues from `pavlov_answers.example_clue_ids`.
  Honesty mode (`typed` optional), `drill_grade` SM-2, allowance rules —
  unchanged, all keyed by `answerId`.
- New-card introduction order: `-ln(random()) / ln(1 + score)`.
- List page: one row per answer — phrases with per-phrase evidence inline
  ("Welsh poet (19/24) · go gentle (11/22) → Dylan Thomas"), hint phrases
  dimmed, suspend per answer. Search across phrases/answer/category.
- Admin generate/status unchanged (status counts still over `pavlov_cues`).

## 5. User review gate (pre-merge)

After local generation on the feature branch, present sample cards
**in-session** for approval before any merge/push/deploy: ~20 random cards
plus ~10 drawn from previously single-cue answers (the vague class this
feature fixes). If rejected, tune (e.g., hint thresholds) and re-preview.
Nothing lands on main until the user approves the samples.

## 6. Verification

- Unit tests: hint-tier qualification + top-up-count logic; card-assembly
  phrase ordering (standard before hint, score desc, cap 3); render parse
  unchanged.
- `verify-pavlov.sql` additions: every `pavlov_answers.phrases` non-empty
  and ≤ 3; every card answer has ≥ 1 standard active cue; hint cues below
  default thresholds allowed but above hint thresholds; leak check G
  extended over `pavlov_answers.phrases`; welsh-poet canary now checks the
  Dylan Thomas *card* contains "Welsh poet".
- Live generation + browser QA; no `question_attempts`/`quiz_sessions`
  writes from drilling.

## Out of scope

Re-rendering existing standard cues; category-weighted scheduling; per-phrase
SRS analytics (the per-pair experiment is over — SRS tracks answers).
