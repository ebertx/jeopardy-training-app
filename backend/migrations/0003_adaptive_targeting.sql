-- Toggle for adaptive weakness targeting in Practice's new-clue picker.
ALTER TABLE users ADD COLUMN IF NOT EXISTS adaptive_targeting BOOLEAN NOT NULL DEFAULT true;
