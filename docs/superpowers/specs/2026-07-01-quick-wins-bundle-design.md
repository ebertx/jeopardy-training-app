# Quick-Wins Bundle — Design Spec

**Date:** 2026-07-01
**Status:** Approved inline by user

Five small items closing deferred findings from the SRS/drill reviews. No schema
migration; rides on existing data.

1. **Dashboard due-forecast chart** — 14-day bar chart from `GET /api/practice/status`
   `forecast[]`. Backend fix folded in: overdue cards bucket into *today*
   (`GREATEST(due::date, today)`), never past dates.
2. **Dashboard 30-day accuracy trend** — daily correct/total from `question_attempts`.
   Labeled **accuracy** (not "retention" — true due-review retention is not derivable
   from recorded history).
3. **Honest "All caught up"** — `practice/next` `done:true` response gains
   `nextDueAt` (soonest future due) and `dueSoonCount` (due within 60 min); Practice
   shows "All caught up — N more due at H:MM" plus a "Check again" button.
4. **NEW badge** — Practice and Drill render a small "NEW" chip (QuestionCard `badge`
   snippet) when `isNew` is true.
5. **Small closes** — `mastery::random_mastered` excludes `suspended` cards;
   `mastery::reset` also clears `lapses`/`suspended`; Practice filter changes hide the
   revealed answer *before* refetching; Settings "Saved" indicator auto-resets.

Also in this pass: a **navigation/presentation audit** of the frontend (read-only
survey); quick fixes fold into this bundle, larger findings reported for later.

Verification: `cargo test`/`clippy` clean; `npm run check` + `build` clean; deploy via
standard push → GHCR → container swap (no migration).
