//! Active-days summary for the dashboard strip.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub streak: i64,
    pub active: i64,
}

/// `days` is oldest → newest with the last element being today. The streak
/// counts consecutive active days ending today, or ending yesterday when
/// today is not active yet (so the number does not reset every morning).
pub fn summarize(days: &[bool]) -> Summary {
    let active = days.iter().filter(|d| **d).count() as i64;
    let mut idx = days.len();
    if idx > 0 && !days[idx - 1] {
        idx -= 1; // skip a not-yet-active today
    }
    let streak = days[..idx].iter().rev().take_while(|d| **d).count() as i64;
    Summary { streak, active }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_active_and_streak_ending_today() {
        let d = [false, true, true, true];
        assert_eq!(summarize(&d), Summary { streak: 3, active: 3 });
    }

    #[test]
    fn today_not_yet_active_counts_from_yesterday() {
        let d = [true, true, true, false];
        assert_eq!(summarize(&d), Summary { streak: 3, active: 3 });
    }

    #[test]
    fn gap_before_yesterday_breaks_streak() {
        let d = [true, true, false, true, false];
        assert_eq!(summarize(&d), Summary { streak: 1, active: 3 });
    }

    #[test]
    fn all_inactive_is_zero() {
        assert_eq!(summarize(&[false, false, false]), Summary { streak: 0, active: 0 });
        assert_eq!(summarize(&[]), Summary { streak: 0, active: 0 });
    }

    #[test]
    fn streak_can_span_the_whole_window() {
        let d = vec![true; 28];
        assert_eq!(summarize(&d), Summary { streak: 28, active: 28 });
    }
}
