# Hook breadth judge — design (2026-10-01)

## Problem
Christian, after drilling the Jeopardy-objects hooks: "way too many hints that are much too broad to be
practical", e.g. "Tennessee Williams play". Hooks are clustered on a gram that recurs within an answer's
clues and ranked by support; nothing checks that the gram (or its label) points to *only* that answer.
"tennessee william" is the top cluster for every Williams play.

## Evidence (live, 20,582 labeled hooks)
- Corpus label precision (FTS of the label vs. entity_norm of matching clues): ~3,900 (19%) below 0.3 with
  ≥5 hits; 42% of entities have one. But it misses paraphrased labels ("Tennessee Williams' family drama"
  scored 1.0 on one clue) and flags fine ones ("Tennessee capital" → Nashville, 0.14). Rejected as the gate.
- Blind recall (gpt-4o sees hint + category, names answer + equal rivals), 120-hook pilot: catches both the
  broad hints and a second class — hints pointing at a different answer ("Bela Bartok's Dance Suite" →
  Hungary, "Leon Uris" → Exodus). One repair from the hook's own clues fixed ~2/3 of failures
  ("Tennessee Williams play, Pulitzer Prize" → "play features character named Big Daddy").

## Design
Per active labeled hook (`judge_hooks`, objects.rs; pure logic in hooks.rs "breadth judge"):
1. Blind guess (gpt-4o): hint + meta category → best answer + rivals that fit as well.
2. Sighted ruling (gpt-4o-mini): guess_matches (aliases count: Quran = the Koran), answer_among_rivals,
   others (genuinely different answers). Verdict: pass = match & no others; broad = reaches the answer but
   others fit; miss = doesn't reach it.
3. Non-pass → one rewrite (gpt-4o) adding the detail only this answer has, gated: ≤10 words, no answer/form
   leak, no hedges, differs from the old hint, every capitalized word and number grounded in the clues or
   old hint (connectives may be new). Re-judge; pass → `judge='rewritten'`, old hint in `cue_before_judge`.
   Fail → `status='dropped'`, `judge` = broad|miss, `judge_rivals`.
4. `restore_last_hooks`: an entity left with no active labeled hook gets its best failed hook back (broad
   before miss, then support). Rerank puts failed-but-kept hints last.
5. Then `merge_duplicate_hooks` (rewrites can converge on another hook's angle) and `rerank`.

Applies to all hooks, including ones already drilled (Christian's choice); review history is untouched.

State: migration 0017 — `judged_at`, `judge`, `judge_rivals`, `cue_before_judge`; a trigger re-arms judging
(clears all four) whenever `cue` changes in an UPDATE that does not also change `judged_at` (the judge's own
writes use clock_timestamp()). Vetted relabeling skips judged 'both' hooks and judged vetted-only rows so a
repair is not undone each Build hooks run. A user drop of a broad/miss hook clears its verdict so the
fallback does not revive it.

Jobs: "Judge hooks" admin button (`POST /api/admin/pavlov/judge`, judge → merge → restore → rerank,
resumable via claims re-armed at start); Build hooks runs the judge after tidy. List page shows
"too broad — fits X, Y" / "points elsewhere" on failed hooks and "was …" on repaired ones; admin status line
shows judged / to go / repaired / dropped / kept.

Revert if needed: `UPDATE pavlov_hooks SET cue = cue_before_judge WHERE judge='rewritten'` and
`UPDATE pavlov_hooks SET status='active' WHERE judge IN ('broad','miss')`.
