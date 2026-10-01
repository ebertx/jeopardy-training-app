-- Breadth judge for hooks (docs/superpowers/specs/2026-10-01-hook-breadth-judge-design.md).
-- judged_at NULL = not yet judged (any relabel resets it). judge records the
-- verdict of the hint as it stands: pass / rewritten (failed, repaired, passed
-- on re-judge) / broad / miss (failed after one repair → dropped, unless it is
-- the entity's last hook). judge_rivals are the answers the hint fit instead;
-- cue_before_judge keeps the replaced hint for the list page.
ALTER TABLE pavlov_hooks
  ADD COLUMN IF NOT EXISTS judged_at        TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS judge            TEXT CHECK (judge IN ('pass', 'rewritten', 'broad', 'miss')),
  ADD COLUMN IF NOT EXISTS judge_rivals     TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS cue_before_judge TEXT;
CREATE INDEX IF NOT EXISTS idx_pavlov_hooks_unjudged ON pavlov_hooks (id)
  WHERE judged_at IS NULL AND status = 'active' AND cue IS NOT NULL;

-- Any cue change the judge did not make (a relabel, a tidy) re-arms judging.
-- The judge's own writes set judged_at in the same UPDATE and pass through.
CREATE OR REPLACE FUNCTION pavlov_hooks_rearm_judge() RETURNS trigger AS $$
BEGIN
  IF NEW.cue IS DISTINCT FROM OLD.cue AND NEW.judged_at IS NOT DISTINCT FROM OLD.judged_at THEN
    NEW.judged_at := NULL;
    NEW.judge := NULL;
    NEW.judge_rivals := '{}';
    NEW.cue_before_judge := NULL;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS pavlov_hooks_rearm_judge ON pavlov_hooks;
CREATE TRIGGER pavlov_hooks_rearm_judge BEFORE UPDATE OF cue ON pavlov_hooks
  FOR EACH ROW EXECUTE FUNCTION pavlov_hooks_rearm_judge();
