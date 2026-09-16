//! Pure math for the Pavlov deck-progress card (spec §2, "Progress math").

use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub deck_total: i64,
    pub touched: i64,
    pub touched_pct: f64,
    pub trailing_per_day: f64,
    pub target_date: NaiveDate,
    /// Calendar days from today through the target, inclusive; 0 when passed.
    pub days_left: i64,
    /// Cards/day needed from today to finish by the target. When the target
    /// has passed this is simply the remaining count (see `past_target`).
    pub required_per_day: i64,
    pub past_target: bool,
    /// Date the deck finishes at the trailing pace; None when pace is zero.
    pub projected_finish: Option<NaiveDate>,
    /// days_left − days needed at trailing pace; positive = ahead. None when
    /// pace is zero.
    pub days_ahead: Option<i64>,
    /// Labeled hooks this user has seen at least once / labeled hooks on the
    /// entities they have touched (spec §3, "hook coverage").
    pub hooks_seen: i64,
    pub hooks_total: i64,
}

const TRAILING_WINDOW_DAYS: f64 = 14.0;

pub fn compute_progress(
    deck_total: i64,
    touched: i64,
    created_last_14d: i64,
    today: NaiveDate,
    target: NaiveDate,
    hooks_seen: i64,
    hooks_total: i64,
) -> Progress {
    let remaining = (deck_total - touched).max(0);
    let touched_pct = if deck_total > 0 {
        ((touched.min(deck_total)) as f64 / deck_total as f64) * 100.0
    } else {
        0.0
    };
    let trailing_per_day = created_last_14d as f64 / TRAILING_WINDOW_DAYS;

    let days_left = ((target - today).num_days() + 1).max(0);
    let past_target = target < today && remaining > 0;
    let required_per_day = if remaining == 0 {
        0
    } else if days_left == 0 {
        remaining
    } else {
        (remaining + days_left - 1) / days_left // ceil
    };

    let days_needed: Option<i64> = if remaining == 0 {
        Some(0)
    } else if trailing_per_day > 0.0 {
        Some((remaining as f64 / trailing_per_day).ceil() as i64)
    } else {
        None
    };
    let projected_finish = days_needed.map(|n| today + chrono::Duration::days(n));
    let days_ahead = days_needed.map(|n| days_left - n);

    Progress {
        deck_total,
        touched,
        touched_pct,
        trailing_per_day,
        target_date: target,
        days_left,
        required_per_day,
        past_target,
        projected_finish,
        days_ahead,
        hooks_seen,
        hooks_total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn progress_carries_hook_coverage_through() {
        let p = compute_progress(100, 10, 14, d(2026, 9, 16), d(2026, 12, 31), 12, 40);
        assert_eq!((p.hooks_seen, p.hooks_total), (12, 40));
    }

    #[test]
    fn typical_mid_september_snapshot() {
        // 4,765 deck, 1,097 touched, 154 created in last 14 days (11/day).
        let p = compute_progress(4765, 1097, 154, d(2026, 9, 15), d(2026, 12, 31), 0, 0);
        assert_eq!(p.days_left, 108); // Sept 15 .. Dec 31 inclusive
        assert_eq!(p.required_per_day, 34); // ceil(3668 / 108)
        assert!(!p.past_target);
        assert!((p.trailing_per_day - 11.0).abs() < 1e-9);
        assert!((p.touched_pct - 23.02).abs() < 0.01);
        // ceil(3668 / 11) = 334 days → 2027-08-15
        assert_eq!(p.projected_finish, Some(d(2027, 8, 15)));
        assert_eq!(p.days_ahead, Some(108 - 334));
    }

    #[test]
    fn zero_trailing_pace_has_no_projection() {
        let p = compute_progress(100, 10, 0, d(2026, 9, 15), d(2026, 12, 31), 0, 0);
        assert_eq!(p.trailing_per_day, 0.0);
        assert_eq!(p.projected_finish, None);
        assert_eq!(p.days_ahead, None);
        assert_eq!(p.required_per_day, 1); // ceil(90 / 108)
    }

    #[test]
    fn target_in_the_past_reports_remaining_and_flags() {
        let p = compute_progress(100, 40, 14, d(2026, 9, 15), d(2026, 9, 1), 0, 0);
        assert_eq!(p.days_left, 0);
        assert!(p.past_target);
        assert_eq!(p.required_per_day, 60);
    }

    #[test]
    fn target_today_counts_today_as_a_day() {
        let p = compute_progress(100, 40, 14, d(2026, 9, 15), d(2026, 9, 15), 0, 0);
        assert_eq!(p.days_left, 1);
        assert!(!p.past_target);
        assert_eq!(p.required_per_day, 60);
    }

    #[test]
    fn deck_complete_is_finished_today() {
        let p = compute_progress(100, 100, 0, d(2026, 9, 15), d(2026, 12, 31), 0, 0);
        assert_eq!(p.required_per_day, 0);
        assert!(!p.past_target);
        assert_eq!(p.projected_finish, Some(d(2026, 9, 15)));
        assert_eq!(p.days_ahead, Some(108));
        assert!((p.touched_pct - 100.0).abs() < 1e-9);
    }

    #[test]
    fn required_per_day_rounds_up() {
        // 10 remaining over 3 days → 4/day, not 3.
        let p = compute_progress(20, 10, 0, d(2026, 9, 15), d(2026, 9, 17), 0, 0);
        assert_eq!(p.days_left, 3);
        assert_eq!(p.required_per_day, 4);
    }

    #[test]
    fn touched_above_deck_total_clamps_remaining_to_zero() {
        // Deck regenerated smaller than what was already touched.
        let p = compute_progress(50, 60, 0, d(2026, 9, 15), d(2026, 12, 31), 0, 0);
        assert_eq!(p.required_per_day, 0);
        assert!((p.touched_pct - 100.0).abs() < 1e-9);
    }
}
