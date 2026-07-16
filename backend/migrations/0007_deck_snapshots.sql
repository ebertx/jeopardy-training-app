-- 0007: daily deck-composition snapshots so the dashboard can show
-- week-over-week deltas (mastered +18, struggling -2). One row per user per
-- local day, upserted on each /api/practice/status call.
CREATE TABLE srs_deck_snapshots (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    snap_date DATE NOT NULL,
    learning INTEGER NOT NULL,
    maturing INTEGER NOT NULL,
    mastered INTEGER NOT NULL,
    struggling INTEGER NOT NULL,
    PRIMARY KEY (user_id, snap_date)
);
