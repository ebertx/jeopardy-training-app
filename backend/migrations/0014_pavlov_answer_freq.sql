-- 0014: answer_freq on pavlov_answers — corpus-wide count of the answer
-- (copied from jeopardy_questions.answer_freq, migration 0008) so new-card
-- introduction can favour frequently-recurring answers. Additive; backfills
-- from each answer's first example clue (every example clue of an answer
-- shares the same normalized response, so any one carries the same count).
ALTER TABLE pavlov_answers
  ADD COLUMN IF NOT EXISTS answer_freq INTEGER NOT NULL DEFAULT 1;

UPDATE pavlov_answers pa
SET answer_freq = jq.answer_freq
FROM jeopardy_questions jq
WHERE jq.id = pa.example_clue_ids[1];
