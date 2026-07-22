# Pavlov Cues v2 — Recurrence + Precision Mining

**Date:** 2026-07-22
**Status:** Approved
**Supersedes:** the mining pipeline of `2026-07-21-pavlov-cues-design.md`
(drill/browse/SRS machinery from v1 is retained).

## Why v2

The v1 deck (frequency-ranked answers + TF-IDF terms + LLM paraphrase) failed
in first use: it selected extremely broad answers (Africa, red, 6, triangle)
and produced "random facts" as cue phrases. The archetypal Pavlov cue —
**"Welsh poet" → Dylan Thomas** — has a different statistical signature,
verified against the corpus:

- **Recurrence:** the stemmed phrase appears in many *distinct clues* for the
  same answer ("welsh poet" in 19 Dylan Thomas clues).
- **Precision:** when the phrase appears anywhere in the corpus, it almost
  always means that answer (19/25 = 0.76; "wise king" → Solomon 15/17 = 0.88;
  versus "primary color" → red 0.09, "southernmost point" → Africa 0.06).

v2 mines (phrase, answer) pairs by these two tests. Broad answers drop out
automatically because none of their phrases pass both bars.

## Unit of drilling

One card per **(phrase → answer) association** (Davy-style), not per answer.
"welsh poet" → Dylan Thomas and "fern hill" → Dylan Thomas are separate cards
with separate SRS state.

## 1. Mining corpus (migration 0010)

- `pavlov_clue_ngrams`: one-time table of position-ordered stemmed grams per
  clue — unigrams and adjacent bigrams — built via
  `to_tsvector('english', answer)` (the **clue text only**; v1's use of
  `search_tsv` leaked category names and the answer text into the signal).
  - Columns: `clue_id`, `answer_norm` (0008 normalization), `gram`,
    `n` (1|2).
  - Scope: `archived = false`, `question IS NOT NULL`,
    `answer_freq >= 4` (an answer below 4 clues cannot reach support 4).
  - Indexes on `(gram)` and `(answer_norm)`.
  - NOTE: heavy one-time build (~10–20M rows, minutes). Apply during low use.
- `pavlov_cues` rebuilt (v1 table dropped — approved):
  - `id`, `answer` (display), `answer_norm`, `meta_category`,
    `cue_stem` (the mined gram), `cue_display` (natural surface form),
    `support INT`, `total INT`, `prec REAL`, `example_clue_ids INT[]`,
    `status` (`pending` | `active` | `dropped`), `model`, `created_at`.
  - `UNIQUE (answer_norm, cue_stem)`.
- `pavlov_cards`: schema unchanged; all rows deleted (drill state reset —
  approved).

## 2. Candidate selection

From `pavlov_clue_ngrams`, per (answer_norm, gram):

- `support` = distinct clues of that answer containing the gram.
- `total` = distinct clues corpus-wide containing the gram (within the
  freq ≥ 4 scope).
- `prec` = support / total.
- Qualify when: bigrams `support >= 4 AND prec >= 0.5`; unigrams
  `support >= 6 AND prec >= 0.6` (stricter — single stems are noisier).
  These are the **default** thresholds, subject to the preview checkpoint.
- Answer-leak filter (`phrase_leaks_answer`) applied to the gram vs. answer.
- Redundancy pruning: when a qualifying bigram contains a qualifying unigram
  of the same answer (or two qualifying bigrams of the same answer overlap in
  a shared clue-set), keep the higher-scoring gram (score = support × prec)
  and drop the subsumed one — avoids "milk wood" + "wood" duplicates.
- **No quotas, no category seats.** Every qualifying cue enters the deck;
  deck size and category mix fall out of the data. `blend::TARGET_WEIGHTS`
  is no longer used for selection (the category mix is *reported* in the
  preview, not enforced).

## 3. Threshold preview checkpoint (user gate)

After the ngram table builds, present to the user before any generation:
counts, per-category mix, and ~15 sample cues at three settings (strict /
default / loose around the defaults above). The user picks; the chosen
thresholds are then recorded in this spec.

## 4. Surface-form polish (LLM, cosmetic only)

- Batches through `openai.rs` as in v1; resumable pending → active/dropped.
- Input per cue: the stemmed gram, the answer, up to 3 real example clues.
- Output: `cue_display` — the natural rendering of the recurring phrase as it
  appears in the clues ("welsh poet" → "Welsh poet", "go gentl" → "go
  gentle"). Instruction: render the given phrase only; never add information,
  never include the answer. `keep=false` only when no natural rendering
  exists.
- `phrase_leaks_answer` re-applied to the rendered form; leaking or empty
  renders → `dropped`.

## 5. Drill + browse changes

- Drill card front: **one** cue phrase + meta-category chip. API card shape:
  `{cueId, cue, category}` (replaces `cuePhrases[]`).
- Reveal: unchanged (answer + real example clues; check/grade endpoints,
  honesty mode, SM-2, allowance rules all as in v1 + follow-ups).
- Browser: rows are "phrase → answer" with support, total, and precision
  visible (the deck's evidence is auditable). Grouped by meta-category,
  searchable, per-row suspend as before. Admin generate/status unchanged.
- The `answer_freq`-weighted new-cue introduction order is replaced by
  score order: `-ln(random()) / ln(1 + support * prec)`.

## 6. Verification

- Unit tests: candidate-qualification predicate (support/prec/leak),
  redundancy pruning, polish parse (render-only contract).
- `scripts/verify-pavlov.sql` v2: every active cue has support/prec above
  the chosen thresholds; no duplicate (answer_norm, cue_stem); check G
  (leaks) retained against `cue_display`; spot-check that "welsh poet" →
  Dylan Thomas–grade cues exist.
- Live generation + browser QA as in v1.

## Out of scope

Category-weighted drill scheduling (can be added later if the natural mix
feels lopsided); trigrams (bigrams capture the archetypes; revisit only if
the preview shows obvious missing phrases).
