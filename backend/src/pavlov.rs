//! Pavlov cue mining: seat planning, TF-IDF term filtering, and LLM polish
//! (docs/superpowers/specs/2026-07-21-pavlov-cues-design.md).

use crate::blend::{sampling_kind, split_seats, SamplingKind, TARGET_WEIGHTS};
use crate::routes::mock_test::apportion;

pub const TOTAL_SEATS: i64 = 1500;
pub const MIN_FREQ: i32 = 5;

#[derive(Debug, Clone)]
pub struct SeatPlan {
    pub category: String,
    pub canon: i64,
    pub recency: i64,
}

pub fn seat_plan(total: i64) -> Vec<SeatPlan> {
    let dist: Vec<(String, i64)> = TARGET_WEIGHTS
        .iter()
        .map(|(c, w)| (c.to_string(), *w))
        .collect();
    apportion(&dist, total)
        .into_iter()
        .map(|(category, seats)| match sampling_kind(&category) {
            SamplingKind::Canon => SeatPlan { category, canon: seats, recency: 0 },
            SamplingKind::Recency => SeatPlan { category, canon: 0, recency: seats },
            SamplingKind::Split => {
                let (canon, recency) = split_seats(seats);
                SeatPlan { category, canon, recency }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_for(cat: &str, plan: &[SeatPlan]) -> (i64, i64) {
        let p = plan.iter().find(|p| p.category == cat).expect("category present");
        (p.canon, p.recency)
    }

    #[test]
    fn seat_plan_covers_all_categories_and_sums_to_total() {
        let plan = seat_plan(1500);
        assert_eq!(plan.len(), TARGET_WEIGHTS.len());
        let sum: i64 = plan.iter().map(|p| p.canon + p.recency).sum();
        assert_eq!(sum, 1500);
    }

    #[test]
    fn canon_categories_get_only_canon_seats() {
        let plan = seat_plan(1500);
        // Literature & Language is 20/100 of 1500 = 300, all canon.
        assert_eq!(plan_for("Literature & Language", &plan), (300, 0));
    }

    #[test]
    fn recency_categories_get_only_recency_seats() {
        let plan = seat_plan(1500);
        // Film, TV & Pop Culture is 10/100 of 1500 = 150, all recency.
        assert_eq!(plan_for("Film, TV & Pop Culture", &plan), (0, 150));
    }

    #[test]
    fn music_splits_seats_with_canon_taking_the_odd_one() {
        let plan = seat_plan(1500);
        // Music & Performing Arts is 6/100 of 1500 = 90 → 45/45.
        let (canon, recency) = plan_for("Music & Performing Arts", &plan);
        assert_eq!(canon + recency, 90);
        assert!(canon >= recency);
        assert!(canon - recency <= 1);
    }
}
